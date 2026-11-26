const statuses = new Set(["completed", "failed", "declined"]);

export function codexCommandRecorder({ threadId, turnId, observedAtMs, report }) {
  if (![threadId, turnId].every(validId) ||
      !Number.isSafeInteger(observedAtMs) || observedAtMs < 0) {
    throw new Error("The Codex thread ID, turn ID and observation time are required.");
  }
  return async (notification) => {
    if (notification?.method !== "item/completed" || Object.hasOwn(notification, "id")) return false;
    const params = notification.params;
    const item = params?.item;
    if (params?.threadId !== threadId || params?.turnId !== turnId ||
        item?.type !== "commandExecution" || !validId(item.id)) return false;
    const event = {
      kind: "codex_command_reported",
      status: statuses.has(item.status) ? item.status : "unknown",
    };
    if (Number.isInteger(item.exitCode) && item.exitCode >= -0x80000000 && item.exitCode <= 0x7fffffff) {
      event.exit_code = item.exitCode;
    }
    if (Number.isInteger(item.durationMs) && item.durationMs >= 0 && item.durationMs <= 0xffffffff) {
      event.duration_ms = item.durationMs;
    }
    await report(["codex-command", threadId, turnId, item.id], event, observedAtMs);
    return true;
  };
}

function validId(value) {
  return typeof value === "string" && value.length > 0 && value.length <= 256;
}
