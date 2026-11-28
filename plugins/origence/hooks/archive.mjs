import { realpathSync } from "node:fs";
import { isAbsolute } from "node:path";
import {
  clientId, digest, host, HttpStatus, identify, pathBytes, rawUuid, scopedId, uuid,
} from "./protocol.mjs";
import { spool, spoolDirectory } from "./spool.mjs";

const TEXT_LIMIT = 64 * 1024;
const TURN_LIMIT = 16;
const RECEIPT_LIMIT = 256;
const initial = () => ({ target: null, prompts: [], pending: [], recent: [] });
const valid = state => Array.isArray(state?.prompts) && state.prompts.length <= TURN_LIMIT &&
  Array.isArray(state.pending) && state.pending.length <= TURN_LIMIT &&
  Array.isArray(state.recent) && state.recent.length <= RECEIPT_LIMIT;
const answerHash = text => [...digest("origence-private-turn-sync-answer-v1", text)];
const sameHash = (a, b) => Array.isArray(a) && a.length === b.length && a.every((value, i) => value === b[i]);
const sameTarget = (a, b) => a?.tenant === b.tenant && a.workspace === b.workspace &&
  a.logical_key === b.logical_key && a.session === b.session;

export function redact(input, bearer) {
  if (typeof input !== "string" || Buffer.byteLength(input) > TEXT_LIMIT) return null;
  let out = "", offset = 0, depth = 0;
  for (const match of input.matchAll(/<private(?=>|[\t\n\v\f\r ])|<\/private>/gi)) {
    if (match[0].startsWith("</")) {
      if (depth > 0 && --depth === 0) offset = match.index + match[0].length;
    } else {
      if (depth === 0) out += input.slice(offset, match.index) + "[redacted]";
      depth++;
    }
  }
  if (depth === 0) out += input.slice(offset);
  if (!out.trim() || out.trim() === "[redacted]") return null;

  let pem = false;
  const lines = out.replaceAll(bearer, "[redacted]").match(/[^\n]*\n|[^\n]+$/g) ?? [];
  out = "";
  for (const line of lines) {
    const lower = line.toLowerCase();
    if (lower.includes("-----begin ") && lower.includes("private key-----")) {
      pem = true;
      out += "[redacted]\n";
    } else if (!pem) {
      out += credential(lower) ? "[redacted]" + (line.endsWith("\n") ? "\n" : "") : line;
    }
    if (pem && lower.includes("-----end ") && lower.includes("private key-----")) pem = false;
  }
  return out.trim() && Buffer.byteLength(out) <= TEXT_LIMIT ? out : null;
}

function credential(line) {
  if (line.includes("bearer ") || line.includes("authorization: basic ")) return true;
  for (const word of line.split(/[\s"'\x60,()]/)) {
    if (Buffer.byteLength(word) >= 16 && /^(sk-|ghp_|gho_|github_pat_|akia|xoxb-)/.test(word)) return true;
    if (/:\/\/[^\/@]*:[^\/@]*@/.test(word)) return true;
  }
  return /(?:^|[^a-z0-9_-])(?:[a-z0-9_-]*_)?(?:api_key|api-key|apikey|access_token|token|password|secret)[\s"'\x60]*[=:]/.test(line);
}

function archiveEvent(input, client, token) {
  if (input.agent_id != null) return null;
  const session = clientId(input.session_id);
  if (!session) return null;
  const kind = input.hook_event_name;
  const sessionKey = scopedId("origence-private-turn-sync-session-v1", client, session);
  if (kind === "SessionEnd") return { kind, sessionKey };
  const nativeTurn = clientId(client === "codex" ? input.turn_id : input.prompt_id);
  if (!nativeTurn) return null;
  const turn = scopedId("origence-private-turn-sync-id-v1", client, session, nativeTurn);
  switch (kind) {
    case "UserPromptSubmit":
      if (input.prompt != null && typeof input.prompt !== "string") return null;
      return { kind, turn, sessionKey, text: redact(input.prompt, token) };
    case "Stop": {
      if (input.last_assistant_message == null) return { kind, turn, text: null };
      if (typeof input.last_assistant_message !== "string") return null;
      const answer = redact(input.last_assistant_message, token);
      return answer == null ? { kind: "abandon", turn } : { kind, turn, text: answer };
    }
    case "Interrupt":
    case "StopFailure":
      return { kind: "abandon", turn };
    default:
      return null;
  }
}

function finish(state, turn, answer = null) {
  state.prompts = state.prompts.filter(prompt => prompt.turn !== turn);
  state.recent.push([turn, answer]);
  if (state.recent.length > RECEIPT_LIMIT) state.recent.shift();
}

function observe(state, { kind, turn, sessionKey, text }) {
  if (kind === "SessionEnd") {
    state.prompts = state.prompts.filter(prompt => prompt.session !== sessionKey);
    return;
  }
  const pending = state.pending.find(item => item.turn_id === turn);
  const recent = state.recent.find(([id]) => id === turn);
  if (pending || recent) {
    const before = pending ? answerHash(pending.answer) : recent[1];
    if (before && kind === "Stop" && text && !sameHash(before, answerHash(text)))
      return "completed turn changed; first answer kept";
    return;
  }
  const index = state.prompts.findIndex(item => item.turn === turn);
  if (kind === "abandon" || kind === "UserPromptSubmit" && text == null) {
    finish(state, turn);
    return;
  }
  if (kind === "UserPromptSubmit") {
    if (index >= 0) return state.prompts[index].text !== text ? "prompt changed; first observation kept" : undefined;
    const full = state.prompts.length === TURN_LIMIT;
    if (full) finish(state, state.prompts[0].turn);
    state.prompts.push({ session: sessionKey, turn, text });
    if (full) return "an unfinished turn exceeded the local queue limit";
    return;
  }
  if (!text) return "completed answer was unavailable";
  if (index < 0) return "matching prompt was unavailable";
  if (state.pending.length === TURN_LIMIT) return "completed turn could not enter the full archive queue";
  const [{ text: question }] = state.prompts.splice(index, 1);
  state.pending.push({ turn_id: turn, question, answer: text, used_evidence: [], evidence_use: "unknown" });
}

function scopedHost({ base, token, workspace }) {
  return host(base, token, workspace, AbortSignal.timeout(2000));
}

async function deliver(queue, context, target) {
  return queue.send(async () => {
    if (!queue.read().pending.length) return;
    const identity = await identify(scopedHost(context), context.workspace);
    if (!identity || !sameTarget(identity.target, target)) throw new Error("archive scope changed");
    const path = "/v1/sessions/" + target.session + "/turns";
    let notice;
    for (let index = 0; index < 4; index++) {
      const input = queue.read().pending[0];
      if (!input) break;
      const turn = uuid(input.turn_id);
      if (!turn || typeof input.question !== "string" || typeof input.answer !== "string" ||
          !input.question.trim() || !input.answer.trim() ||
          Buffer.byteLength(input.question) > TEXT_LIMIT || Buffer.byteLength(input.answer) > TEXT_LIMIT)
        throw new Error("invalid queued archive turn");
      try {
        const receipt = await identity.host.call(path, {
          turn_id: turn, question: input.question, answer: input.answer,
          used_evidence: [], evidence_use: "unknown",
        }, "turn-sync-" + turn);
        if (receipt.state !== "archived" || receipt.session_id !== target.session ||
            receipt.turn_id !== turn || !uuid(receipt.source_event_id)) throw new Error("invalid archive receipt");
      } catch (error) {
        if (!(error instanceof HttpStatus) || ![404, 409].includes(error.status)) throw error;
        notice = error.status === 409 ? "archived turn conflict" : "archive source unavailable";
      }
      queue.update(state => {
        if (state.pending[0]?.turn_id !== turn) throw new Error("archive spool changed");
        finish(state, turn, answerHash(state.pending.shift().answer));
      });
    }
    return notice;
  });
}

export async function syncArchive(context, input, configured) {
  if (!configured) return;
  if (!isAbsolute(configured)) throw new Error("invalid archive project");
  if (realpathSync.native(configured) !== context.project) return;
  const { client, token, data, project, base, workspace } = context;
  const event = archiveEvent(input, client, token);
  if (!event) return;
  const scope = scopedId("origence-private-turn-sync-scope-v1",
    client, pathBytes(project), base, rawUuid(workspace ?? undefined), token);
  const directory = spoolDirectory(data, project, "turns-" + scope + ".queue");
  const queue = spool(directory, "turns-v1.json", 16 * 1024 * 1024, initial, valid);
  let target = queue.read().target;
  if (!target) {
    if (event.kind !== "UserPromptSubmit" || !event.text) return;
    target = (await identify(scopedHost(context), workspace))?.target;
    if (!target?.tenant || !target.session) return;
  }
  if (!uuid(target.tenant) || !uuid(target.workspace) || !uuid(target.logical_key) || !uuid(target.session) ||
      workspace && target.workspace !== workspace) throw new Error("invalid archive target");
  let notice;
  try {
    notice = queue.update(state => {
      if (state.target && !sameTarget(state.target, target)) throw new Error("archive scope changed");
      state.target = target;
      return observe(state, event);
    });
  } catch {
    notice = "a turn could not be recorded";
  }
  if (!["Stop", "SessionEnd"].includes(event.kind)) return notice;
  const delivered = await deliver(queue, context, target).catch(() => "delivery deferred");
  return notice ?? delivered;
}
