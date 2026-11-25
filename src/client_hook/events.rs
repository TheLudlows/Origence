//! The client wire schemas meet the closed v1 queue vocabulary here.
use super::{Client, client_id};
use crate::run::{CoverageStatus, RunEvent, RunEventInput, RunOutcome, RunReporter};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use uuid::Uuid;

#[derive(Deserialize)]
pub(super) struct HookInput<'a> {
    pub hook_event_name: String,
    pub cwd: Option<String>,
    pub session_id: Option<String>,
    turn_id: Option<String>,
    prompt_id: Option<String>,
    agent_id: Option<String>,
    tool_name: Option<String>,
    tool_use_id: Option<String>,
    stop_hook_active: Option<bool>,
    duration_ms: Option<u64>,
    is_interrupt: Option<bool>,
    #[serde(borrow)]
    tool_response: Option<&'a RawValue>,
}

impl HookInput<'_> {
    pub fn turn_key(&self, client: Client) -> Option<&str> {
        match client {
            Client::Codex => self.turn_id.as_deref(),
            Client::ClaudeCode => self.prompt_id.as_deref(),
        }
    }

    pub fn observation(&self, client: Client) -> Option<Observation> {
        match client {
            Client::Codex => self.codex().map(Observation::Codex),
            Client::ClaudeCode => self.claude_code().map(Observation::ClaudeCode),
        }
    }

    fn codex(&self) -> Option<Codex> {
        if self.agent_id.is_some() {
            return None;
        }
        Some(match self.hook_event_name.as_str() {
            "UserPromptSubmit" => Codex::PromptSubmitted,
            "Stop" => Codex::Stop {
                continued_before: self.stop_hook_active?,
            },
            "Interrupt" => Codex::Interrupted,
            "PostToolUse" => {
                let tool = self.tool(Client::Codex)?;
                let is_error = if tool.is_mcp() {
                    self.tool_response
                        .and_then(|raw| serde_json::from_str::<McpStatus>(raw.get()).ok())
                        .and_then(|result| result.is_error)
                } else {
                    None
                };
                Codex::ToolCompleted { tool, is_error }
            }
            _ => return None,
        })
    }

    fn claude_code(&self) -> Option<ClaudeCode> {
        match self.hook_event_name.as_str() {
            "SubagentStart" => return Some(ClaudeCode::SubagentStarted),
            "SubagentStop" => {
                return Some(ClaudeCode::SubagentStop {
                    continued_before: self.stop_hook_active?,
                });
            }
            _ => {}
        }
        if self.agent_id.is_some() {
            return None;
        }
        Some(match self.hook_event_name.as_str() {
            "UserPromptSubmit" => ClaudeCode::PromptSubmitted,
            "Stop" => ClaudeCode::Stop {
                continued_before: self.stop_hook_active?,
            },
            "StopFailure" => ClaudeCode::ApiError,
            "PostToolUse" | "PostToolUseFailure" => {
                let outcome = match (self.hook_event_name.as_str(), self.is_interrupt) {
                    ("PostToolUse", _) => ClaudeToolOutcome::Completed,
                    (_, Some(true)) => ClaudeToolOutcome::AbortReported,
                    _ => ClaudeToolOutcome::Failed,
                };
                ClaudeCode::ToolFinished {
                    tool: self.tool(Client::ClaudeCode)?,
                    outcome,
                    duration_ms: self.duration_ms,
                }
            }
            _ => return None,
        })
    }

    fn tool(&self, client: Client) -> Option<Tool> {
        let name = self.tool_name.as_deref()?;
        Tool::ALL
            .iter()
            .copied()
            .find(|tool| tool.allowed(client) && tool.matches(name))
    }
}

#[derive(Deserialize)]
struct McpStatus {
    #[serde(rename = "isError")]
    is_error: Option<bool>,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Tool {
    Bash,
    ApplyPatch,
    Edit,
    Write,
    Agent,
    ContextSearch,
    ContextGet,
    ContextRunCreate,
    ContextRunReport,
    ContextRunPrepare,
    ContextRunGet,
}

impl Tool {
    const ALL: [Self; 11] = [
        Self::Bash,
        Self::ApplyPatch,
        Self::Edit,
        Self::Write,
        Self::Agent,
        Self::ContextSearch,
        Self::ContextGet,
        Self::ContextRunCreate,
        Self::ContextRunReport,
        Self::ContextRunPrepare,
        Self::ContextRunGet,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Bash => "Bash",
            Self::ApplyPatch => "apply_patch",
            Self::Edit => "Edit",
            Self::Write => "Write",
            Self::Agent => "Agent",
            Self::ContextSearch => "mcp__origence__context_search",
            Self::ContextGet => "mcp__origence__context_get",
            Self::ContextRunCreate => "mcp__origence__context_run_create",
            Self::ContextRunReport => "mcp__origence__context_run_report",
            Self::ContextRunPrepare => "mcp__origence__context_run_prepare",
            Self::ContextRunGet => "mcp__origence__context_run_get",
        }
    }

    fn is_mcp(self) -> bool {
        self.name().starts_with("mcp__")
    }

    fn matches(self, name: &str) -> bool {
        self.name() == name
            || self
                .name()
                .strip_prefix("mcp__origence__")
                .is_some_and(|suffix| {
                    name.strip_prefix("mcp__plugin_origence_origence__") == Some(suffix)
                })
    }

    fn allowed(self, client: Client) -> bool {
        match self {
            Self::ApplyPatch => client == Client::Codex,
            Self::Edit | Self::Write | Self::Agent => client == Client::ClaudeCode,
            _ => true,
        }
    }
}

// The v1 queue has no client-provided free-text fields. Keep these wire
// mappings stable so a queued retry can reproduce its original Run payload.
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "client",
    content = "observation",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum Observation {
    Codex(Codex),
    ClaudeCode(ClaudeCode),
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Codex {
    PromptSubmitted,
    ToolCompleted { tool: Tool, is_error: Option<bool> },
    Stop { continued_before: bool },
    Interrupted,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ClaudeCode {
    PromptSubmitted,
    ToolFinished {
        tool: Tool,
        outcome: ClaudeToolOutcome,
        duration_ms: Option<u64>,
    },
    Stop {
        continued_before: bool,
    },
    ApiError,
    SubagentStarted,
    SubagentStop {
        continued_before: bool,
    },
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum ClaudeToolOutcome {
    Completed,
    Failed,
    AbortReported,
}

impl Observation {
    pub fn client(self) -> Client {
        match self {
            Self::Codex(_) => Client::Codex,
            Self::ClaudeCode(_) => Client::ClaudeCode,
        }
    }

    pub fn event_key<'a>(self, input: &'a HookInput<'_>) -> &'a [u8] {
        match self {
            Self::ClaudeCode(ClaudeCode::ToolFinished { .. }) => b"tool-finished",
            _ => input.hook_event_name.as_bytes(),
        }
    }

    pub fn source_id<'a>(self, input: &'a HookInput<'_>) -> Option<&'a [u8]> {
        match self {
            Self::Codex(Codex::ToolCompleted { .. })
            | Self::ClaudeCode(ClaudeCode::ToolFinished { .. }) => {
                client_id(input.tool_use_id.as_deref()?)
            }
            Self::ClaudeCode(ClaudeCode::SubagentStarted | ClaudeCode::SubagentStop { .. }) => {
                client_id(input.agent_id.as_deref()?)
            }
            Self::Codex(Codex::Stop { continued_before })
            | Self::ClaudeCode(ClaudeCode::Stop { continued_before }) => {
                Some(if continued_before {
                    b"after-continuation"
                } else {
                    b"first"
                })
            }
            _ => Some(b""),
        }
    }

    pub fn report(self, event_id: Uuid, observed_at_ms: i64) -> RunEventInput {
        let event = match self {
            Self::Codex(observation) => observation.event(),
            Self::ClaudeCode(observation) => observation.event(),
        };
        RunEventInput {
            event_id,
            reporter: RunReporter::Client,
            observed_at_ms,
            event,
        }
    }
}

impl Codex {
    fn event(self) -> RunEvent {
        match self {
            Self::PromptSubmitted => coverage(
                "codex_hook",
                "Codex UserPromptSubmit observed. Only supported hooks and allowlisted tools are collected. Omitted activity is unknown.",
            ),
            Self::ToolCompleted { tool, is_error } => tool_report(
                tool,
                match is_error.filter(|_| tool.is_mcp()) {
                    Some(true) => {
                        "Codex PostToolUse observed. The MCP result reported a tool error."
                    }
                    Some(false) => {
                        "Codex PostToolUse observed. The MCP result reported no tool error."
                    }
                    None => "Codex PostToolUse observed. Tool status unknown.",
                }
                .into(),
            ),
            Self::Stop { continued_before } => stop("Codex", continued_before),
            Self::Interrupted => {
                result("Codex reported an interruption. Business outcome unknown.".into())
            }
        }
    }
}

impl ClaudeCode {
    fn event(self) -> RunEvent {
        match self {
            Self::PromptSubmitted => coverage(
                "claude_code_hook",
                "Claude Code UserPromptSubmit observed. Only supported hooks and allowlisted tools are collected. Omitted activity is unknown.",
            ),
            Self::ToolFinished {
                tool,
                outcome,
                duration_ms,
            } => {
                let mut summary = match outcome {
                    ClaudeToolOutcome::Completed => {
                        "Claude Code reported that the tool completed successfully."
                    }
                    ClaudeToolOutcome::Failed => "Claude Code reported a tool failure.",
                    ClaudeToolOutcome::AbortReported => {
                        "Claude Code reported a tool failure as an abort."
                    }
                }
                .to_owned();
                if let Some(duration) = duration_ms {
                    summary.push_str(&format!(" Tool duration: {duration} ms."));
                }
                tool_report(tool, summary)
            }
            Self::Stop { continued_before } => stop("Claude Code", continued_before),
            Self::ApiError => result(
                "Claude Code reported an API error at StopFailure. Business outcome unknown."
                    .into(),
            ),
            Self::SubagentStarted => coverage(
                "claude_code_hook",
                "Claude Code SubagentStart observed in this prompt context.",
            ),
            Self::SubagentStop { continued_before } => coverage(
                "claude_code_hook",
                if continued_before {
                    "Claude Code SubagentStop observed after a previous continuation. The subagent may continue again."
                } else {
                    "Claude Code SubagentStop observed. Other hooks can continue the subagent."
                },
            ),
        }
    }
}

fn coverage(component: &str, summary: &str) -> RunEvent {
    RunEvent::Coverage {
        component: component.into(),
        status: CoverageStatus::Partial,
        summary: summary.into(),
    }
}

fn tool_report(tool: Tool, summary: String) -> RunEvent {
    RunEvent::ToolReported {
        tool: tool.name().into(),
        summary,
    }
}

fn stop(client: &str, continued_before: bool) -> RunEvent {
    let summary = if continued_before {
        format!(
            "{client} reached Stop after a previous continuation. The turn may continue again. Business outcome unknown."
        )
    } else {
        format!(
            "{client} reached Stop. Other hooks can continue the turn. Business outcome unknown."
        )
    };
    result(summary)
}

fn result(summary: String) -> RunEvent {
    RunEvent::ResultReported {
        outcome: RunOutcome::Unknown,
        summary,
    }
}
