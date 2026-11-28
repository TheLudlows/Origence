import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { redact } from "../plugins/origence/hooks/archive.mjs";
import { observation, report } from "../plugins/origence/hooks/observations.mjs";
import { spool } from "../plugins/origence/hooks/spool.mjs";

const initial = () => ({ pending: [] });
const valid = state => Array.isArray(state?.pending);
const modulePath = new URL("../plugins/origence/hooks/spool.mjs", import.meta.url).href;

test("private spool serializes senders and recovers after a killed process", async () => {
  const data = mkdtempSync(join(tmpdir(), "origence node hook "));
  const queue = spool(data, "state.json", 1024, initial, valid);
  queue.update(state => state.pending.push("kept"));
  const child = spawn(process.execPath, ["--disable-warning=ExperimentalWarning", "--input-type=module", "-e", `
    import { spool } from ${JSON.stringify(modulePath)};
    const queue = spool(process.argv[1], "state.json", 1024, () => ({pending:[]}), x => Array.isArray(x?.pending));
    await queue.send(async () => {
      console.log("locked");
      await new Promise(resolve => setTimeout(resolve, 20000));
    });
  `, data], { stdio: ["ignore", "pipe", "inherit"] });
  const exited = once(child, "exit");
  try {
    const ready = await Promise.race([
      once(child.stdout, "data").then(([text]) => String(text)),
      exited.then(() => { throw new Error("sender exited before acquiring its lock"); }),
    ]);
    assert.equal(ready.trim(), "locked");
    let entered = false;
    await queue.send(() => { entered = true; });
    assert.equal(entered, false);
    queue.update(state => state.pending.push("while sending"));
    child.kill();
    await exited;
    await queue.send(() => { entered = true; });
    assert.equal(entered, true);
    assert.deepEqual(queue.read(), { pending: ["kept", "while sending"] });
  } finally {
    if (child.exitCode === null && child.signalCode === null) child.kill();
    await exited;
    rmSync(data, { recursive: true, force: true });
  }
});

test("legacy v1 observations produce bounded reports for both clients", () => {
  const base = {
    event_id: "2c971331-182f-8cda-a353-58a3e14a889d",
    observed_at_ms: 111,
  };
  const prompt = {
    hook_event_name: "PostToolUse",
    session_id: "session-1",
    turn_id: "turn-1",
    tool_use_id: "tool-1",
    tool_name: "mcp__plugin_origence_origence__context_search",
    tool_response: { isError: false, payload: "raw secret" },
  };
  const seen = observation(prompt, "codex").stored;
  const legacy = { client: "codex", observation: { kind: "tool_completed", tool: "context_search", is_error: false } };
  assert.deepEqual(seen, legacy);
  const result = report({ ...base, observation: legacy });
  assert.match(result.event.summary, /no tool error/);
  assert.equal(result.event.tool, "mcp__origence__context_search");
  const claude = observation({
    ...prompt, hook_event_name: "PostToolUseFailure", prompt_id: base.event_id,
    is_interrupt: true, duration_ms: 100,
  }, "claude-code");
  assert.match(report({ ...base, observation: claude.stored }).event.summary, /abort\. Tool duration: 100 ms\.$/);
  assert.equal(observation({ ...prompt, tool_name: "READ_PRIVATE_DATA" }, "codex"), null);
});

test("private spans, credentials and key blocks are redacted before persistence", () => {
  const plain = "你好 <PRIVATE note=\"x\">hide <private>nested</private> too</PRIVATE>\n"
    + "OPENAI_API_KEY = raw-key\nprofile sk-proj-abcdefghijklmnop\nAuthorization: Bearer abc\n"
    + "-----BEGIN OPENSSH PRIVATE KEY-----\nprivate-key-bytes\n-----END OPENSSH PRIVATE KEY-----\n"
    + "More context active-oc-secret";
  const filtered = redact(plain, "active-oc-secret");
  for (const secret of ["hide", "nested", "too", "raw-key", "abcdefghijklmnop", "Bearer abc", "private-key-bytes", "active-oc-secret"])
    assert.ok(!filtered.includes(secret), "credential material escaped the redactor");
  assert.match(filtered, /你好 \[redacted\]/);
  assert.match(filtered, /More context \[redacted\]/);
  assert.equal(redact("<private>unfinished", "token"), null);
  assert.equal(redact('{"password":"example"}', "token"), "[redacted]");
});
