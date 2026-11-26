import { parseArgs } from "node:util";
import { codexCommandRecorder } from "./codex.mjs";
import { runClaudeTurn } from "./claude.mjs";
import { openRunReporter } from "./report.mjs";

async function* boundedLines(stream, limit) {
  let parts = [];
  let size = 0;
  for await (const chunk of stream) {
    let offset = 0;
    while (offset < chunk.length) {
      const newline = chunk.indexOf(10, offset);
      const end = newline < 0 ? chunk.length : newline;
      size += end - offset;
      if (size > limit) throw new Error("An App Server event exceeded the input limit.");
      parts.push(chunk.subarray(offset, end));
      if (newline < 0) break;
      yield Buffer.concat(parts, size).toString("utf8");
      parts = [];
      size = 0;
      offset = end + 1;
    }
  }
  if (size) yield Buffer.concat(parts, size).toString("utf8");
}

async function boundedPrompt() {
  const limit = 1_048_576;
  const parts = [];
  let bytes = 0;
  for await (const part of process.stdin) {
    bytes += part.length;
    if (bytes > limit) throw new Error("Claude prompt exceeds 1 MiB.");
    parts.push(part);
  }
  return Buffer.concat(parts, bytes).toString("utf8");
}

async function main() {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      "run-id": { type: "string" },
      "thread-id": { type: "string" },
      "turn-id": { type: "string" },
      "observed-at-ms": { type: "string" },
    },
  });
  const [client] = positionals;
  if (positionals.length !== 1 || !["codex", "claude"].includes(client)) {
    throw new Error("Usage: node agent.mjs codex|claude --run-id RUN_ID");
  }
  if (client === "codex" && (!values["thread-id"] || !values["turn-id"] ||
      !/^\d+$/.test(values["observed-at-ms"] ?? ""))) {
    throw new Error("Codex requires --thread-id, --turn-id and --observed-at-ms.");
  }
  if (client === "claude" && ["thread-id", "turn-id", "observed-at-ms"].some(key => values[key] !== undefined)) {
    throw new Error("The Claude SDK driver binds its own prompt UUID and observation times.");
  }
  const report = await openRunReporter({ runId: values["run-id"] });
  if (client === "codex") {
    const record = codexCommandRecorder({
      threadId: values["thread-id"],
      turnId: values["turn-id"],
      observedAtMs: Number(values["observed-at-ms"]),
      report,
    });
    let matched = 0;
    for await (const line of boundedLines(process.stdin, 4 * 1024 * 1024)) {
      if (!line.trim()) continue;
      let notification;
      try {
        notification = JSON.parse(line);
      } catch {
        throw new Error("Invalid App Server JSONL notification.");
      }
      if (await record(notification)) matched++;
    }
    process.stdout.write(JSON.stringify({ matched }) + "\n");
    return;
  }
  let query;
  try {
    ({ query } = await import("@anthropic-ai/claude-agent-sdk"));
  } catch {
    throw new Error("Install the pinned Claude Agent SDK with npm ci in tools/managed-run.");
  }
  const prompt = await boundedPrompt();
  const cancel = new AbortController();
  const interrupt = () => cancel.abort();
  process.on("SIGINT", interrupt);
  try {
    const { result, cancellation } = await runClaudeTurn({
      query, prompt, report, cancelSignal: cancel.signal,
    });
    if (!result && cancellation === null) throw new Error("No matching Claude SDK result was received.");
    process.stdout.write(JSON.stringify({ result_received: result !== null, cancellation }) + "\n");
  } finally {
    process.off("SIGINT", interrupt);
  }
}

main().catch(error => {
  process.stderr.write((error instanceof Error ? error.message : "Managed Run aborted.") + "\n");
  process.exitCode = 1;
});
