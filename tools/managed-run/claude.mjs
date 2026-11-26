import { randomUUID } from "node:crypto";

const ABORTED = new Set(["aborted_streaming", "aborted_tools"]);
const OTHER_TERMINAL = new Set([
  "max_turns", "tool_deferred", "hook_stopped", "stop_hook_prevented",
  "background_requested", "blocking_limit", "rapid_refill_breaker",
  "prompt_too_long", "image_error", "model_error", "api_error",
  "malformed_tool_use_exhausted", "budget_exhausted",
  "structured_output_retry_exhausted", "tool_deferred_unavailable", "turn_setup_failed",
]);

function matchesPrompt(result, promptId) {
  if (result.type !== "result") return false;
  if (result.user_message_uuids !== undefined) {
    return Array.isArray(result.user_message_uuids) && result.user_message_uuids.length === 1 &&
      result.user_message_uuids[0] === promptId &&
      (result.user_message_uuid === undefined || result.user_message_uuid === promptId);
  }
  return result.user_message_uuid === promptId;
}

function receiptStatus(value, promptId) {
  if (value === undefined) return "acknowledged_without_details";
  if (!Array.isArray(value?.still_queued) || value.still_queued.length > 10000 ||
      value.still_queued.some(id => typeof id !== "string" || id.length > 64)) {
    return "unavailable";
  }
  return value.still_queued.includes(promptId) ? "prompt_still_queued" : "acknowledged";
}

function cancellationResult(terminal, receipt) {
  if (terminal === "completed") return "completed";
  if (ABORTED.has(terminal)) return receipt === "acknowledged" ? "interrupted" : "unknown";
  return OTHER_TERMINAL.has(terminal) ? "other_terminal" : "unknown";
}

/** Run one new, UUID-correlated SDK prompt. The caller owns the cancel signal. */
export async function runClaudeTurn({ query, prompt, report, cancelSignal, onMessage, cwd }) {
  if (typeof prompt !== "string" || !prompt.trim() || Buffer.byteLength(prompt) > 1_048_576) {
    throw new Error("A Claude prompt between 1 byte and 1 MiB is required.");
  }
  cancelSignal?.throwIfAborted();
  const promptId = randomUUID();
  const sdkAbort = new AbortController();
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/^OC_/i.test(key)));
  async function* input() {
    yield {
      type: "user",
      uuid: promptId,
      parent_tool_use_id: null,
      message: { role: "user", content: prompt },
    };
  }
  let session;
  try {
    session = query({
      prompt: input(),
      options: { abortController: sdkAbort, cwd, env, permissionMode: "default" },
    });
  } catch {
    throw new Error("Could not start the Claude Agent SDK query.");
  }
  let publication = Promise.resolve();
  let reportError;
  function record(observation) {
    const at = Date.now();
    publication = publication.then(() => report(
      ["claude-interrupt", promptId, observation.stage],
      { kind: "claude_interrupt_reported", observation },
      at,
    )).catch(error => { reportError ??= error; });
  }

  let pendingReceipt;
  let cancelDeadline;
  let result;
  let active = true;
  function interrupt() {
    if (!active || pendingReceipt) return;
    record({ stage: "requested" });
    const deadline = new Promise(resolve => {
      cancelDeadline = setTimeout(() => {
        resolve("unavailable");
        sdkAbort.abort();
      }, 10_000);
    });
    const response = Promise.resolve().then(() => session.interrupt())
      .then(value => receiptStatus(value, promptId), () => "unavailable");
    pendingReceipt = Promise.race([response, deadline]).then(status => {
      record({ stage: "receipt", status });
      return status;
    });
  }
  cancelSignal?.addEventListener("abort", interrupt, { once: true });
  if (cancelSignal?.aborted) interrupt();

  let streamFailed = false;
  let status;
  try {
    // Keep the SDK iterator open until the interrupt response is read.
    const messages = session[Symbol.asyncIterator]();
    for (;;) {
      const { value: message, done } = await messages.next();
      if (done) break;
      if (matchesPrompt(message, promptId)) {
        result = message;
        active = false;
      }
      await onMessage?.(message);
      if (result) break;
    }
  } catch {
    streamFailed = true;
  } finally {
    active = false;
    cancelSignal?.removeEventListener("abort", interrupt);
    try {
      if (pendingReceipt) {
        const receipt = await pendingReceipt;
        status = cancellationResult(result?.terminal_reason, receipt);
        record({ stage: "result", status });
      }
    } finally {
      clearTimeout(cancelDeadline);
      try {
        session.close();
      } catch {
        streamFailed = true;
      }
      await publication;
    }
  }
  if (reportError) throw reportError;
  if (streamFailed && !pendingReceipt) throw new Error("Claude Agent SDK query ended without a readable result.");
  return { result: result ?? null, cancellation: status ?? null };
}
