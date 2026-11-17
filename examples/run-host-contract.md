# Minimal host Run adapter — Issue #87

Frozen against main `293e2bf6af3b180d43c8050731047b552ac07519`.
Use the adjacent Codex/Claude Code stdio configurations with `--server-url` and
an inherited `OC_API_KEY`. Do not commit tokens. The local embedded MCP mode does
not implement Run tools: they return an explicit `--server-url` requirement.
The existing archive tools remain available in both modes.

| MCP tool | Arguments | HTTP mapping |
| --- | --- | --- |
| `context_run_create` | `run_id`, sanitized `summary` | POST `/v1/runs` |
| `context_run_prepare` | `run_id`, `event_id`, `context` (existing EvidenceInput) | POST `/v1/runs/{id}/context` |
| `context_run_report` | `run_id`, `report` (existing RunEventInput) | POST `/v1/runs/{id}/events` |
| `context_run_get` | `run_id`, optional `after` sequence / `limit` (50 default, 100 max) | GET `/v1/runs/{id}` |

HostClient exposes typed `create_run`, `prepare_run_context`, `append_run_event`,
`run_trace` for non-MCP callers. MCP's report envelope decodes the existing strict
RunEventInput; it adds no event/verification type or storage field. HTTP errors
retain the existing safe MCP JSON-RPC error mapping. Retry the identical UUID and
payload; changed payload conflicts. Credential rotation returns a new credential
key ID while preserving logical ownership; revoke the returned replacement key ID
to revoke the new credential. No hidden retry is performed. A transport
failure is an uncertain outcome: read back or retry the same identity, never infer
that a write did not happen. Failed preparation recovery uses a fresh event ID.

Example `context_run_report` arguments (synthetic values only):

```json
{
  "run_id": "11111111-1111-4111-8111-111111111111",
  "report": {
    "event_id": "22222222-2222-4222-8222-222222222222",
    "reporter": "client",
    "observed_at_ms": 0,
    "event": {
      "kind": "coverage",
      "component": "external tools",
      "status": "unknown",
      "summary": "No host observation collected"
    }
  }
}
```

Only report actual observations: existing event kinds are `context_reported`,
`tool_reported`, `skill_loaded_reported` (exact UUID/version/SHA256),
`artifact_reported` (opaque UUID), `result_reported`, `feedback`, and `coverage`.
Reports never establish execution, model use, delivery or verification. Source
withdrawal and exact-version checks remain server authority. `context_prepared` and `context_preparation_failed`
are server-only event kinds, rejected in caller reports. Feedback/Improve HTTP
operations and explicit turn archival are unchanged; Run feedback is a Run claim,
not an implicit invocation of the learning/Improve pipeline.

## Observable event matrix and outstanding real-host acceptance

This matrix describes the observation boundary of these adapters, not proof of a
real host run. Both hosts must explicitly invoke the tools. No hooks intercept
unrelated host activity. Missing observations are unknown.

| Observation | Claude Code through this adapter | Codex through this adapter |
| --- | --- | --- |
| Same logical key archive | explicit session/turn read | explicit session/turn read |
| Independent task identity | explicit create with unique Run UUID | explicit create with unique Run UUID |
| Server preparation / safe failure | authoritative HTTP event | authoritative HTTP event |
| Context received or used by model | unknown without separate report; report is only claim | same |
| External tool invocation/result | explicit `tool_reported`, otherwise unknown | same |
| Native Skill loading | explicit exact-version `skill_loaded_reported`, otherwise unknown | same |
| Artifact / final task outcome | explicit report, not independent verification | same |
| Coverage | explicit unknown/partial/not_collected; omission unknown | same |
| Real host/version/task evidence in this change | not collected (Claude executable absent) | not collected (no real task/model invocation) |

The protocol test launches two real Origence stdio processes against a loopback
HTTP host, fresh temporary SQLite and a synthetic AnySeek protocol fixture, with
models disabled. Those peers are **not** Claude Code or Codex. Real two-host task
acceptance, Windows execution, host-version freezing and enterprise task outcomes
remain outstanding; do not promote this fixture to those acceptance claims.
