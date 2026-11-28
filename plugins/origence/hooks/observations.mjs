import { clientId, uuid } from "./protocol.mjs";

const TOOLS = {
  bash: "Bash", apply_patch: "apply_patch", edit: "Edit", write: "Write", agent: "Agent",
  ...Object.fromEntries(["search", "get", "run_create", "run_report", "run_prepare", "run_get"]
    .map(suffix => ["context_" + suffix, "mcp__origence__context_" + suffix])),
};

function toolFor(name, client) {
  if (typeof name !== "string") return null;
  const normal = name.replace(/^mcp__plugin_origence_origence__/, "mcp__origence__");
  const entry = Object.entries(TOOLS).find(([, wire]) => wire === normal)?.[0];
  if (entry === "apply_patch" && client !== "codex" ||
      ["edit", "write", "agent"].includes(entry) && client === "codex") return null;
  return entry;
}

export function observation(input, client) {
  const codex = client === "codex";
  const kind = input.hook_event_name;
  if (input.agent_id != null && (codex || !["SubagentStart", "SubagentStop"].includes(kind))) return null;
  const session = clientId(input.session_id);
  const turn = codex ? clientId(input.turn_id) : uuid(input.prompt_id);
  if (!session || !turn) return null;
  let source = "";
  let event = kind;
  let detail;
  switch (kind) {
    case "UserPromptSubmit":
      detail = { kind: "prompt_submitted" }; break;
    case "Interrupt":
      if (codex) detail = { kind: "interrupted" };
      break;
    case "StopFailure":
      if (!codex) detail = { kind: "api_error" };
      break;
    case "Stop":
      if (typeof input.stop_hook_active !== "boolean") return null;
      detail = { kind: "stop", continued_before: input.stop_hook_active };
      source = input.stop_hook_active ? "after-continuation" : "first";
      break;
    case "SubagentStart":
    case "SubagentStop":
      if (codex || !(source = clientId(input.agent_id))) return null;
      if (kind === "SubagentStart") detail = { kind: "subagent_started" };
      else if (typeof input.stop_hook_active === "boolean")
        detail = { kind: "subagent_stop", continued_before: input.stop_hook_active };
      break;
    case "PostToolUse":
    case "PostToolUseFailure": {
      const tool = toolFor(input.tool_name, client);
      if (!tool || !(source = clientId(input.tool_use_id))) return null;
      if (codex) {
        if (kind !== "PostToolUse") return null;
        const error = input.tool_response?.isError;
        detail = { kind: "tool_completed", tool,
          is_error: tool.startsWith("context_") && typeof error === "boolean" ? error : null };
      } else {
        event = "tool-finished";
        detail = { kind: "tool_finished", tool,
          outcome: kind === "PostToolUse" ? "completed" : input.is_interrupt === true ? "abort_reported" : "failed",
          duration_ms: Number.isSafeInteger(input.duration_ms) && input.duration_ms >= 0 ? input.duration_ms : null };
      }
      break;
    }
  }
  if (!detail) return null;
  return { session, turn, event, source,
    stored: { client: codex ? "codex" : "claude_code", observation: detail } };
}

export function runSummary(client) {
  return client === "codex" ? "Codex turn (hook metadata only)" : "Claude Code prompt (hook metadata only)";
}

// Decode both newly captured and previously persisted v1 observations into the
// original closed wire vocabulary. No arbitrary queued text is forwarded.
export function report({ event_id, observed_at_ms, observation: saved }) {
  const codex = saved?.client === "codex";
  if (!codex && saved?.client !== "claude_code") throw new Error("invalid client observation");
  const observed = saved.observation;
  const client = codex ? "Codex" : "Claude Code";
  const coverage = summary => ({ kind: "coverage", status: "partial",
    component: codex ? "codex_hook" : "claude_code_hook", summary });
  const result = summary => ({ kind: "result_reported", outcome: "unknown", summary });
  const tool = summary => {
    if (!Object.hasOwn(TOOLS, observed.tool)) throw new Error("invalid queued tool");
    return { kind: "tool_reported", tool: TOOLS[observed.tool], summary };
  };
  let event;
  switch (observed.kind) {
    case "prompt_submitted":
      event = coverage(client + " UserPromptSubmit observed. Only supported hooks and allowlisted tools are collected. Omitted activity is unknown.");
      break;
    case "stop":
      event = result(client + (observed.continued_before
        ? " reached Stop after a previous continuation. The turn may continue again. Business outcome unknown."
        : " reached Stop. Other hooks can continue the turn. Business outcome unknown."));
      break;
    case "interrupted":
      event = result("Codex reported an interruption. Business outcome unknown."); break;
    case "api_error":
      event = result("Claude Code reported an API error at StopFailure. Business outcome unknown."); break;
    case "subagent_started":
      event = coverage("Claude Code SubagentStart observed in this prompt context."); break;
    case "subagent_stop":
      event = coverage(observed.continued_before
        ? "Claude Code SubagentStop observed after a previous continuation. The subagent may continue again."
        : "Claude Code SubagentStop observed. Other hooks can continue the subagent.");
      break;
    case "tool_completed":
      event = tool("Codex PostToolUse observed. " + (observed.tool.startsWith("context_") && typeof observed.is_error === "boolean"
        ? observed.is_error ? "The MCP result reported a tool error." : "The MCP result reported no tool error."
        : "Tool status unknown."));
      break;
    case "tool_finished": {
      const summary = {
        completed: "the tool completed successfully.",
        failed: "a tool failure.",
        abort_reported: "a tool failure as an abort.",
      };
      if (!Object.hasOwn(summary, observed.outcome)) throw new Error("invalid queued tool outcome");
      const duration = Number.isSafeInteger(observed.duration_ms) && observed.duration_ms >= 0
        ? " Tool duration: " + observed.duration_ms + " ms." : "";
      event = tool("Claude Code reported " + (observed.outcome === "completed" ? "that " : "") + summary[observed.outcome] + duration);
      break;
    }
    default: throw new Error("invalid queued observation");
  }
  return { event_id, reporter: "client", observed_at_ms, event };
}
