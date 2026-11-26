# Managed Run reporting

**English** · [简体中文](MANAGED-RUNS.md)

Applications driving Codex App Server or the Claude Agent SDK can record command completion or cancellation in an existing private Run. Use Node.js 24, a workspace writer credential, and the Run owner's identity. Set `OC_API_KEY`, `OC_WORKSPACE_ID`, and `OC_SERVER_URL` (default `http://127.0.0.1:8080`) for the adapter. Use HTTPS for a remote server. Create the Run with HTTP `POST /v1/runs` or MCP `context_run_create` before binding the adapter.

## Codex App Server

Register the recorder on the existing App Server notification stream. Bind the exact thread and turn from its `turn/start` receipt. Choose an observation timestamp at binding and use the same value when replaying that batch.

```js
import { openRunReporter } from "./tools/managed-run/report.mjs";
import { codexCommandRecorder } from "./tools/managed-run/codex.mjs";

const report = await openRunReporter({ runId });
const record = codexCommandRecorder({
  threadId, turnId, observedAtMs: Date.now(), report,
});
for await (const notification of appServerNotifications) {
  await record(notification);
}
```

You can also pipe the owning application's JSONL notifications into the adapter. Input is capped at 4 MiB per line. Only bound `item/completed` notifications with a `commandExecution` item produce a `codex_command_reported` event. The Run timeline shows status, exit code, and duration when provided. Missing values stay unknown and explicit zero is preserved. Command text, directories, arguments, raw output, and external event IDs are omitted. This adapter has no SDK dependency.

```sh
node tools/managed-run/agent.mjs codex --run-id "$RUN_ID" \
  --thread-id "$THREAD_ID" --turn-id "$TURN_ID" --observed-at-ms "$OBSERVED_AT_MS"
```

## Claude Agent SDK

Run `npm ci` in `tools/managed-run` to install the pinned Claude Agent SDK 0.3.332. Use `runClaudeTurn({ query, prompt, report, cancelSignal, onMessage })` for one new prompt. Pass a signal controlled by the calling application. When the caller aborts it, the adapter invokes SDK `interrupt()`. The `onMessage` callback can display SDK messages locally. The command reads a single prompt from stdin, up to 1 MiB, and accepts a SIGINT addressed to the adapter process.

```sh
node tools/managed-run/agent.mjs claude --run-id "$RUN_ID" < prompt.txt
```

The Run timeline shows the request, the SDK receipt, and the result separately. “Confirmed interrupted” requires a valid receipt that does not list the prompt as queued and a result matching the prompt UUID with an explicit streaming or tool abort. Completion, other terminal reasons, and unknown remain distinct. Missing receipts, missing SDK fields, results for other prompts, and timeouts leave gaps. The adapter waits up to ten seconds after cancellation before aborting the SDK query stream. The child environment excludes Origence `OC_*` variables. Prompts, replies, queued message IDs, and raw SDK errors are omitted from reports.

## Run semantics

Both adapters reuse the private Run API. Retries retain the same event ID and body, with up to two retries per write. A Claude invocation uses a new prompt UUID to distinguish separate executions. These are client reports. Business outcomes still rely on explicit result reports or separate verification evidence. Ordinary terminal sessions use the existing [Hook coverage](CLIENTS.en.md#events-and-correlation-boundaries).

[Codex App Server](https://learn.chatgpt.com/docs/app-server) · [Claude Agent SDK interrupt receipt](https://code.claude.com/docs/en/agent-sdk/typescript#sdkcontrolinterruptresponse)
