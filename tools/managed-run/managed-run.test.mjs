import assert from "node:assert/strict";
import { once } from "node:events";
import { createServer } from "node:http";
import { test } from "node:test";
import { runClaudeTurn } from "./claude.mjs";
import { codexCommandRecorder } from "./codex.mjs";
import { openRunReporter } from "./report.mjs";

test("Codex uses the exact thread and turn, sends only allowlisted fields, and replays the same bytes", async t => {
  const workspaceId = "91919191-9191-4191-8191-919191919191";
  const runId = "a1a1a1a1-a1a1-41a1-81a1-a1a1a1a1a1a1";
  const calls = [];
  const posted = [];
  const server = createServer(async (req, res) => {
    const parts = [];
    for await (const part of req) parts.push(part);
    const body = Buffer.concat(parts).toString();
    calls.push([req.method, req.url, req.headers["x-origence-workspace"], req.headers.authorization]);
    if (req.url === "/v1/whoami") return res.end(JSON.stringify({ workspace_id: workspaceId, role: "writer" }));
    if (req.url.endsWith("/prefix?through_sequence=0")) return res.end("{}");
    posted.push(body);
    res.statusCode = posted.length === 1 ? 503 : 200;
    res.end("{}");
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => { server.closeAllConnections(); server.close(); });
  const report = await openRunReporter({
    runId, workspaceId, token: "oc_synthetic", origin: `http://127.0.0.1:${server.address().port}`,
  });
  const record = codexCommandRecorder({ threadId: "thread-a", turnId: "turn-a", observedAtMs: 42, report });
  const notice = {
    jsonrpc: "2.0", method: "item/completed", params: {
      threadId: "thread-a", turnId: "turn-a", item: {
        type: "commandExecution", id: "sensitive-item-id", status: "failed",
        exitCode: 17, durationMs: 0, command: "sensitive command",
        cwd: "/sensitive/path", aggregatedOutput: "sensitive output",
      },
    },
  };
  assert.equal(await record({ ...notice, id: 1 }), false);
  assert.equal(await record({ ...notice, params: { ...notice.params, turnId: "turn-b" } }), false);
  assert.equal(await record({ ...notice, params: { ...notice.params, threadId: "thread-b" } }), false);
  assert.equal(await record({ ...notice, method: "item/started" }), false);
  assert.equal(await record(notice), true);
  assert.equal(await record(notice), true);
  assert.equal(posted.length, 3);
  assert(posted.every(body => body === posted[0] && !body.includes("sensitive")));
  assert.deepEqual(JSON.parse(posted[0]).event, {
    kind: "codex_command_reported", status: "failed", exit_code: 17, duration_ms: 0,
  });
  const missing = structuredClone(notice);
  Object.assign(missing.params.item, { id: "second", status: "new-status", exitCode: null, durationMs: -1 });
  await record(missing);
  assert.deepEqual(JSON.parse(posted[3]).event, { kind: "codex_command_reported", status: "unknown" });
  assert.notEqual(JSON.parse(posted[0]).event_id, JSON.parse(posted[3]).event_id);
  assert.deepEqual(calls.slice(0, 2).map(call => call[1]), ["/v1/whoami", `/v1/runs/${runId}/prefix?through_sequence=0`]);
  assert(calls.every(call => call[2] === workspaceId && call[3] === "Bearer oc_synthetic"));
});

const cases = [
  ["acknowledged", "aborted_streaming", true, "interrupted"],
  ["acknowledged", "aborted_tools", true, "interrupted"],
  ["prompt_still_queued", "aborted_streaming", true, "unknown"],
  ["acknowledged_without_details", "aborted_streaming", true, "unknown"],
  ["unavailable", "aborted_streaming", true, "unknown"],
  ["acknowledged", "completed", false, "unknown"],
  ["acknowledged", "completed", true, "completed"],
  ["acknowledged", "api_error", true, "other_terminal"],
  ["acknowledged", undefined, true, "unknown"],
];

for (const [receipt, terminal, matches, expected] of cases) {
  test(`Claude cancel with ${receipt}, ${terminal}, match=${matches} reports ${expected}`, async () => {
    const cancel = new AbortController();
    const { promise: delayedReceipt, resolve: resolveReceipt } = Promise.withResolvers();
    let interruptCalls = 0;
    let promptId;
    let closed = false;
    const sent = [];
    function query({ prompt, options }) {
      assert.equal(options.permissionMode, "default");
      assert.equal(Object.keys(options.env).some(key => /^OC_/i.test(key)), false);
      return {
        async interrupt() {
          interruptCalls++;
          const response = await delayedReceipt;
          assert.equal(closed, false, "receipt is awaited before closing the SDK session");
          return response;
        },
        close() { closed = true; },
        async *[Symbol.asyncIterator]() {
          const { value: user } = await prompt.next();
          promptId = user.uuid;
          yield { type: "system", subtype: "init" };
          yield { type: "result", terminal_reason: "completed", user_message_uuid: "another-prompt" };
          const matchingId = matches ? promptId : "unrelated-prompt";
          yield {
            type: "result", terminal_reason: terminal, user_message_uuid: matchingId,
            user_message_uuids: [matchingId], result: "private reply fixture",
          };
        },
      };
    }
    const value = await runClaudeTurn({
      query, prompt: "private prompt fixture", cancelSignal: cancel.signal,
      report: async (identity, event) => sent.push({ identity, event }),
      onMessage(message) {
        if (message.type === "system") cancel.abort();
        if (message.result) {
          setImmediate(() => {
            if (receipt === "unavailable") resolveReceipt({ unknown: "private SDK error" });
            else if (receipt === "acknowledged_without_details") resolveReceipt(undefined);
            else resolveReceipt({ still_queued: [receipt === "prompt_still_queued" ? promptId : "private unrelated UUID"] });
          });
        }
      },
    });
    assert.equal(closed, true);
    assert.equal(interruptCalls, 1);
    assert.equal(value.cancellation, expected);
    assert.equal(value.result !== null, matches);
    assert.deepEqual(sent.map(({ event }) => event), [
      { kind: "claude_interrupt_reported", observation: { stage: "requested" } },
      { kind: "claude_interrupt_reported", observation: { stage: "receipt", status: receipt } },
      { kind: "claude_interrupt_reported", observation: { stage: "result", status: expected } },
    ]);
    assert(!JSON.stringify(sent).includes("private"));
    assert(sent.every(({ identity }) => identity[1] === promptId));
  });
}

test("Claude completion followed by a late cancel does not interrupt another turn", async () => {
  const cancel = new AbortController();
  const result = await runClaudeTurn({
    prompt: "test prompt",
    cancelSignal: cancel.signal,
    report: () => assert.fail("unexpected cancellation report"),
    onMessage: () => cancel.abort(),
    query: ({ prompt }) => ({
      close() {},
      interrupt: () => assert.fail("unexpected interrupt"),
      async *[Symbol.asyncIterator]() {
        const { value: user } = await prompt.next();
        yield { type: "result", user_message_uuid: user.uuid, terminal_reason: "completed" };
      },
    }),
  });
  assert.equal(result.cancellation, null);
});

test("Claude cancellation times out and closes a query without inventing a terminal result", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const cancel = new AbortController();
  const events = [];
  let sdkSignal;
  let closed = false;
  const result = await runClaudeTurn({
    prompt: "test prompt",
    cancelSignal: cancel.signal,
    report: (_, event) => events.push(event.observation),
    onMessage() {
      cancel.abort();
      t.mock.timers.tick(10_000);
    },
    query: ({ options }) => {
      sdkSignal = options.abortController.signal;
      return {
        close() { closed = true; },
        interrupt: () => new Promise(() => {}),
        async *[Symbol.asyncIterator]() {
          yield { type: "system", subtype: "init" };
          assert.equal(sdkSignal.aborted, true);
          throw new Error("sensitive SDK timeout error");
        },
      };
    },
  });
  assert.equal(closed, true);
  assert.equal(result.cancellation, "unknown");
  assert.deepEqual(events, [
    { stage: "requested" }, { stage: "receipt", status: "unavailable" },
    { stage: "result", status: "unknown" },
  ]);
});
