import { host, HttpStatus, identify, ownsDirectory, pathBytes, rawUuid, scopedId, uuid } from "./protocol.mjs";
import { report, runSummary } from "./observations.mjs";
import { spool, spoolDirectory } from "./spool.mjs";

const initial = () => ({ pending: [], acknowledged: [] });
const valid = state => Array.isArray(state?.pending) && state.pending.length <= 256 &&
  Array.isArray(state.acknowledged) && state.acknowledged.length <= 1024;

export async function recordRun(context, input, observed) {
  if (!observed || !ownsDirectory(context.project, input.cwd)) return;
  const { base, token, workspace, project, data } = context;
  let identity;
  try {
    identity = await identify(host(base, token, workspace, AbortSignal.timeout(2000)), workspace);
  } catch (error) {
    if ([401, 403].includes(error.status)) return;
    throw error;
  }
  if (!identity) return;
  const { workspace: actual, logical_key: key } = identity.target;
  const common = [pathBytes(project), base, rawUuid(key)];
  const run = scopedId("origence-plugin-hook-run-v1", context.client, ...common,
    observed.session, context.client === "codex" ? observed.turn : rawUuid(observed.turn), rawUuid(actual));
  const event = scopedId("origence-client-hook-event-v1", rawUuid(run), observed.event, observed.source);
  const scope = scopedId("origence-plugin-hook-queue-v1", pathBytes(project), base, rawUuid(actual), rawUuid(key));
  const binding = scopedId("origence-client-hook-queue-binding-v1", ...common, "OC_API_KEY", token, rawUuid(actual));
  const directory = spoolDirectory(data, project, "run-" + scope + ".queue");
  const queue = spool(directory, "queue-v1.json", 256 * 1024, initial, valid);
  queue.update(state => {
    if (state.pending.length === 256 || state.acknowledged.includes(event) ||
        state.pending.some(record => record.event_id === event)) return;
    state.pending.push({ binding, run_id: run, event_id: event,
      observed_at_ms: Date.now(), observation: observed.stored });
  });

  await queue.send(async () => {
    const created = new Set();
    const batch = queue.read().pending.filter(item => item.binding === binding &&
      item.observation?.client === observed.stored.client).slice(0, 16);
    for (const item of batch) {
      if (!uuid(item.run_id) || !uuid(item.event_id) || !Number.isSafeInteger(item.observed_at_ms))
        throw new Error("invalid queued event identity");
      try {
        if (!created.has(item.run_id)) {
          const receipt = await identity.host.call("/v1/runs", { run_id: item.run_id, summary: runSummary(context.client) });
          if (receipt.run_id !== item.run_id) throw new Error("invalid Run receipt");
          created.add(item.run_id);
        }
        const receipt = await identity.host.call("/v1/runs/" + item.run_id + "/events", report(item));
        if (receipt.run_id !== item.run_id || receipt.event_id !== item.event_id ||
            !Number.isSafeInteger(receipt.sequence) || receipt.sequence < 1)
          throw new Error("invalid Run event receipt");
      } catch (error) {
        if (!(error instanceof HttpStatus) || ![404, 409, 422].includes(error.status)) throw error;
      }
      queue.update(state => {
        const index = state.pending.findIndex(record => record.event_id === item.event_id);
        if (index < 0) throw new Error("Run spool changed");
        state.pending.splice(index, 1);
        state.acknowledged.push(item.event_id);
        if (state.acknowledged.length > 1024) state.acknowledged.shift();
      });
    }
  });
}
