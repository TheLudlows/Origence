# Private Run trace

This contract defines the private evidence path for the [architecture plan](../design/agent-data-foundation.md).
Run creation and append are authorized relational transactions. Each Run is bound
to a server-assigned private session and stable logical key. Workspace admin is
not an implicit private archive reader. Credential rotation preserves logical
ownership; revocation prevents replay as well as new writes and readback.

`run_id` and `event_id` are durable retry identities. Equal payloads return the
original receipt; changed payloads conflict. Events are append-only, monotonically
sequenced per Run and capped at 10,000. Readback is paginated, at most 100 events.
A digest addresses an exact immutable prefix, not the mutable latest Run.

Client/user/test labels are claims recorded beside the authenticated credential
identity, never server attestations. Reports support tool calls, context references,
results, feedback, and explicit unknown/partial/not-collected coverage. An omitted
event means unknown. A reported successful outcome is not independently verified.
MCP cannot observe unrelated client or tool activity.

The caller-inaccessible context preparation path records a SHA256 digest of the
exact serialized server bundle, immutable citations, route accounting, warnings,
and omission/budget information. It attests only that the server prepared that
bundle, not that a client received it or a model used it. Source visibility and
private session ownership are checked again inside the append transaction and
on trace/prefix readback. Withdrawal fails closed rather than exposing stale
private source evidence. Empty prepared bundles are valid and preserve warnings.
No automatic capture of every context request, external tool call, or result is
claimed by this interface. Routes must check Run ownership before retrieval IO.

Selected Skill review can use the internal trusted-owner prefix digest helper,
not raw Run content or a caller-provided owner ID. Source guards remain in force;
private session-backed citations may therefore deny a different reviewer rather
than expand private read access.

Explicit `skill_loaded_reported` events associate an exact Skill UUID, positive
version and canonical SHA256 digest with a Run. `artifact_reported` associates an
opaque output UUID and bounded summary. Both are reports only: they neither prove
execution nor grant access, and do not imply that the referenced object exists.
Run readback never recursively resolves Skill packages (a Skill may itself cite
an immutable Run prefix). The owner-only prefix endpoint supplies the exact digest
needed for that nonrecursive source reference.

## Retrieval observations

Prepared context includes a request-local bounded retrieval trace and its checked
analysis/embedding configuration identity. The exact serialized bundle, including
observations, is hashed. Successful idempotent replay retains the first preparation's
IDs/timing and returns that same bundle; a context request digest prevents reusing
an event ID with another request even if both produce empty context.

The successful event stores a private copy of the prepared bundle for retries.
Replay checks the event and request digest before model/retrieval IO, revalidates
current permission, ownership and every original citation, and returns the original
bundle/receipt even during a backend outage or after corpus updates. Paginated
traces omit this internal snapshot. The append transaction repeats the replay
check so a concurrent successful preparation also wins over a later failed attempt.
Withdrawn sources still block replay; the snapshot never grants access to them.

After owner/input checks, failed preparation attempts may append a server-observed
failure event with only a safe error code and content-free observations. Failed
attempts are immutable; recovery uses a new event ID. Trace and prefix reads retain
all existing owner and source guards. Backend progress is sampled after retrieval;
it is not an atomic read of the result, an Origence transaction, delivery proof, or
independent verification of the evidence. See [API](../API.md#检索诊断).
Query diagnostics v2 include the current embedding-space SHA256 (or null), checked
against the admitted space on every response in addition to the analysis digest.
