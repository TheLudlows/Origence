# Origence · Unified data foundation for AI agents

**English** · [简体中文](agent-data-foundation.md)

The first-release contract baseline is v0.8 (2026-11-16). This document describes the target architecture and initial acceptance.

See the [roadmap](../ROADMAP.en.md) for product evolution, [platform design](current-implementation.en.md) for implementation, [API](../API.en.md) for interfaces, and [STATUS](../STATUS.en.md) for progress.

## 1. Positioning and first-release scope

**Origence is the unified data foundation for AI agents, covering memory, knowledge, Skills, and Trace.** Context assembly selects data the agent is authorized to use for the task. Trace records execution evidence in private Runs for review and experience distillation.

The first release targets enterprise engineering and business teams, piloting on intranets through local **Claude Code and Codex** CLIs. Users select evidence from memory and execution experience to form reviewable Skill candidates. Publication requires approval by a user with publishing rights. Session archives and Runs are private by logical key. Each logical key binds one session within a workspace. Published memory, knowledge, and Skills belong to a workspace, are reused according to permissions, and are isolated across workspaces. Sessions support platform archiving and authorized reads, without restoring client work environments.

## 2. Overall architecture

```mermaid
flowchart TB
    CLIENT[Initial hosts Claude Code / Codex] --> ACCESS[Origence MCP, Skill distribution, and events]
    USER[Users submit material and review Skills] --> ACCESS
    ACCESS --> AUTH[Workspace and user-key ID permissions]
    subgraph DATA[Unified data foundation]
        PRIVATE[Private archives bound to logical key]
        ASSET[Workspace memory, knowledge, and published Skills]
        RUN[Trace private Runs, events, and verification]
    end
    subgraph SERVICES[Data services]
        SERVE[Context selection, read, and delivery]
        LEARN[Experience distillation into Skill candidates]
        REVIEW[Selected review package, review, and approval]
        VIEW[Task tracing, feedback, and collection gaps]
    end
    ASSET --> SERVE
    ASSET --> LEARN
    PRIVATE --> SERVE
    PRIVATE --> LEARN
    LEARN --> REVIEW
    REVIEW -->|Publish approved Skill| ASSET
    AUTH --> SERVE
    AUTH --> REVIEW
    AUTH --> RUN
    AUTH --> VIEW
    RUN --> VIEW
    SERVE --> SEARCH[AnySeek full-text and vector search]
    SERVE --> STORE[SQLite authority, permissions, versions, and jobs]
    ASSET --> BLOB[Blob original files]
    SERVE -. Context preparation and precise citations .-> RUN
    ACCESS -. Client reports where supported .-> RUN
    RUN -->|Experience and feedback| LEARN
    RUN --> STORE
```

The diagram shows the target. Solid arrows represent business calls or data dependencies, and dashed arrows observation evidence. Event ingestion and reads check permissions. The host owns model calls, task orchestration, and tool execution. AnySeek is the sole target path for full-text and vector retrieval. SQLite holds business authority, while Blob holds original files.

## 3. Core data and context

| Type | Content |
| --- | --- |
| Memory | Facts, preferences, confirmed decisions, and experiences with subject, project, and applicability conditions |
| Knowledge | Enterprise documents, standards, interface definitions, product rules, and authorized business materials |
| Skill | Instructions, steps, procedures, templates, and references distilled from memory and Agent experience |
| Trace | Run-organized preparation records, Skill load and tool usage reports, task outcomes, feedback, external verification, and capture gaps |

Record task status in explicitly submitted turns, task summaries, or event reports. Run artifact reports keep external artifact IDs and digests. Uploaded originals and published Skill packages each have a dedicated read path. Published data uses exact version and source references. Observation and report records retain reporter and time. Source authorization must cover the sharing scope.

### Ownership and isolation

| Scope | Data and access |
| --- | --- |
| Private records | Submitted turns and feedback, Runs and context snapshots, and unpublished candidates, isolated by logical key |
| Workspace assets | Published long-term memory, facts, knowledge, and Skills available to authorized workspace users and Agents |

In a workspace, stable logical `key_id` maps to a unique `session_id`. Credentials and logical identity are managed separately. Plaintext keys are not storage identities. New independent keys create independent sessions. Explicitly authorized rotation retains the logical key and session. Usernames do not merge sessions automatically. Revocation blocks corresponding access. Session history, published assets, and source withdrawals follow their own lifecycles and are not removed on credential revocation. Claude Code and Codex with the same logical key access the same Origence archive. Use separate keys for independent Agent records. One session can contain multiple tasks and Runs. Ownership is derived from server authentication and cannot be changed by passing another session ID. Private endpoints and context assembly verify logical ownership and current permissions. Authorized reviewers see only expressly disclosed content in a selected review package. `GET /v1/session` returns authenticated `{session_id,key_id}` with no caller-selected ownership. `POST /admin/keys/{id}/rotate` permits the current admin to rotate credentials in their workspace, returns a replacement, and immediately revokes the old credential. Explicit SessionTurn routes still require writer and also match the current key's session. Read permissions and ownership are separate. The platform preserves submitted records and provides queries without syncing or restoring client chats, context windows, tool/process/environment state, or replaying actions.

Promoting private records explicitly creates a workspace asset with selected shared content and controlled source associations. The original session and trace stay private. Memory, facts, and knowledge require their respective write and publication permissions. Skills also need user approval. Unpublished candidates are not shared long-term assets. Sharing private content requires read, disclosure, and publication authority. Reviewers can only see the selected review package, and a workspace publisher has no automatic access to original sessions. A review package separates proposed artifacts (body, manifest, references) from necessary redacted evidence. Approval binds exact revision, all artifact digests, evidence digest, and target workspace/version/scope. Only approved manifest artifacts are published, while review evidence is private by default. Changing any bound item requires new review. Recheck permissions and sources before publication. Share only approved content and retain original session and citation privacy.

`EvidenceBundle` assembles memory, knowledge, and Learning by task, permission, and budget. Callers may include authorized archive turns. Preparation records exact references, gaps, and truncation. [Context assembly contract](../contracts/evidence-bundle.md)

Hosts obtain valid published packages from dedicated Skill download or export routes and load them by their own mechanism. Run load events are client reports. Candidates use a dedicated user review path.

## 4. How Skills are distilled from experience

```mermaid
flowchart LR
    M[Authorized memory] --> D[Distill a reusable method]
    E[Private Agent actions, outcomes, corrections, and feedback] --> D
    D --> P[Skill candidate]
    P --> B[Review package with text and redacted evidence]
    B --> U[Authorized user review and edit]
    U -->|Publish after user approval| S[Pinned Skill version in workspace]
    S --> A[Agent reuse]
    A -->|New results and feedback| E
```

Agents or the platform identify goals, preconditions, parameters, steps, checkpoints, and verification methods from authorized memory and recorded execution. Preserve failures, limits, and evidence gaps. One self-reported success does not establish reliability. In the first release, users select the basis and initiate candidates. See the [roadmap](../ROADMAP.en.md#milestones) for exploration of automatic proposals.

Readable candidate details should let users review the following.

| Area | What the user sees |
| --- | --- |
| Method | Problem, approach, inputs/outputs, checks, and verification steps |
| Basis | Supporting memory versions, execution records, tools and results, feedback, and unproven parts |
| Limits | Conditions, failures or counterexamples, and unverified situations |
| Changes | Differences from the published version, rationale, and review history |

Users can edit or reject candidates, or publish them as a new Skill or version. Published versions retain their evidence and history. Candidates inherit private input scopes. An authorized submitter selects the content and redacted evidence for review. Review permission applies to that package only. **A Skill must be approved by a user with publishing permission.** Approval binds the selected package and destination. The publisher generates artifacts from the approved contents exactly. A publication is immutable `SKILL.md`, manifest, and reference files with version, digests, supporting sources, and review decision. The two adapters distribute and load them. New experience produces later candidates without deleting history. The first release includes text instructions, controlled references, and allowlisted metadata. The publisher validates package contents and allowed client fields, rejecting tool preauthorization, hooks, dynamic commands, and executable scripts. Credentials are excluded. Sharing approval does not authorize real actions. Existing host and business-tool permission and approval rules still govern execution. Managed installation and load lists are isolated by workspace. Withdrawal blocks later distribution and managed loading, without immediate recall of offline or already exported copies.

## 5. Trace and execution evidence

Trace links task summaries, preparation, Skill versions, tools, outcomes, feedback, and coverage through **Run**. Runs and events are isolated under the session's logical key. Authorized results or experiences can separately publish as workspace assets.

| Evidence source | What it records |
| --- | --- |
| Origence server | Context preparation for a specified Run, exact references, diagnostics, and capture boundaries |
| Client adapter | Observed Skill loads, tools, outcomes, artifact references, and gaps according to its capabilities |
| User or test | Its own task feedback and results |
| Run owner | Transcribed external verification of an exact Run prefix, with claimed provenance |

Each event retains Run identity, reporter, time, and a stable event ID. Duplicate reports are idempotent. Server receipts, client reports, and external verification retain separate provenance. Skill approval and publication decisions are stored with review records.

**MCP can observe only requests that pass through it.** Unreported client activities remain unknown or collection gaps. The first release links context, Skills, tools, and outcomes in a readable task trace. Retain references, required status, and redacted summaries by default. Credentials are excluded from context and observations.

## 6. Origence and AnySeek responsibilities

| Work | Origence | AnySeek |
| --- | --- | --- |
| Data preparation | Accept material, keep originals, parse/chunk, and generate query and document vectors | Store text, vectors, and retrieval metadata and maintain indexes |
| Retrieval | Derive authorization scope, issue text/vector queries, check sources and versions, assemble references | Match text, search vector similarity, and rank candidates |
| Assets, context, and Trace | Asset lifecycle, experience distillation, Skill review/publication, context assembly, and execution evidence | Preserve published retrieval text and provide candidates |
| Persistence | SQLite stores permissions, version metadata, Skill packages, Trace, and durable jobs. Blob preserves original files. | Persist published text, summaries, and indexes by stable Record identity |

Origence selects and pins embedding model/profile and dimension. AnySeek performs vector search. Retrieval metadata contains full workspace, asset and source versions, and generation. Candidate filters and final authoritative checks agree. Private material is excluded from workspace-wide retrieval by default. Owner key and session are also verified when searching or reading it. The local host opens a disk Catalog through the AnySeek SDK. In standalone deployment, Origence holds the AnySeek URL and service credential. Both modes derive namespace and collection from authenticated Scope. Standalone mode sends tenant UUID as `X-AnySeek-Tenant`. Each workspace uses a separate `ws_<无连字符 workspace UUID>` collection (hyphens removed), initialized on first use with a fixed schema. Startup checks published collections and rejects missing or incompatible configurations. Storage isolation does not replace user authorization, private session checks, or source/version checks. Revocation or source withdrawal blocks future reads and distribution before index cleanup. Snapshots and Skills cannot bypass it. Dependency failure returns unavailable without pretending retrieval is empty.

Reuse the existing publication ledger, single durable Worker, original-plan recovery, and source locators. Model and index I/O stay outside transactions. There is no second full-text or vector engine. Build dependencies and deployment follow the unified AnySeek boundary.

## 7. Implementation order and acceptance

| Order | Delivery | Acceptance |
| --- | --- | --- |
| 1. Unified retrieval and both clients | AnySeek text/vector integration, logical key to archive binding, and workspace asset isolation | Same key reads the same archive across clients, separate keys are isolated, authorized rotation retains access, and workspace retrieval returns valid citations |
| 2. Skills from experience | Use memory and execution evidence to create candidates and let users review, edit, and publish | One engineering and one business Skill have traceable evidence, are published by authorized approval, and can be reused in both clients |
| 3. Trace and task verification | Pin context list and associate Skills, observable tools, outcomes, and feedback | Real task links all collected evidence readably and explicitly shows gaps |

Add Run and result recording through integration for Skill distillation. Step 3 completes the unified tracing experience.

Each step also verifies isolation, source update and withdrawal, precise version references, retries, and reopen recovery. Cross-key direct reads, Bundle/search, Run queries, artifact access, and source links must not leak private content. Workspaces do not share assets. Skills also check invalidation of approval after editing and denial of reads or distribution after support invalidation. Verify unauthorized submission/review denial, approval invalidation after digest or destination changes, rejection of execution extensions, and isolation of local loading across workspaces. Session acceptance covers archive and querying, authorized rotation, revocation, and retention. Client environment restoration is outside acceptance. Retrieval replacement uses public library, HTTP/MCP, and real AnySeek tests. Client outcomes use frozen task comparisons. Documentation and protocol fixtures do not show real-task success.

See [STATUS](../STATUS.en.md) for current code and open work and [VALIDATION](../VALIDATION.en.md) for results.

## 8. Confirmed decisions and pilot preparations

Initial pilot and acceptance boundaries follow.

- **Deployment and integration**. Intranet deployment with local Claude Code/Codex CLIs as the first baseline.
- **Skill publication**. A user with publishing rights must approve. Candidate generation does not publish.
- **Pilot scenarios**. Engineering tests database troubleshooting and business tests expense auditing. See [P0 acceptance #91](https://github.com/TheLudlows/Origence/issues/91) for material, versions, and independent sign-off.
- **Data ownership**. The same user API key accesses the same private session. Long-term memory, facts, Skills, and knowledge belong to the workspace. Publication does not expose original sessions.
- **Session boundary**. Independent keys start separate. Authorized rotation preserves continuity. Archive and query platform records without restoring the client environment.
- **Sharing and execution**. Approval governs selected packages. The initial Skill contains text and references only. Publication and tool execution have separate authorization.

Break implementation contracts down by section 7. Finalize candidate generation details in implementation. Existing domain contracts remain maintained in their own documents.

## 9. Design references

These sources were consulted on 2026-11-16 to compare mechanisms. They do not establish Origence implementation or outcomes.

| References | Adopted boundary |
| --- | --- |
| [LangGraph Persistence](https://docs.langchain.com/oss/python/langgraph/persistence), [LangMem](https://langchain-ai.github.io/langmem/concepts/conceptual_guide/) | Separate private task state from cross-task assets. Represent facts, experience, and methods separately. |
| [Letta Skills](https://docs.letta.com/configuration/skills) | Distill reusable methods from execution and publish by workspace with human approval. |
| [Agent Skills](https://agentskills.io/specification), [Claude Code](https://code.claude.com/docs/en/skills), [Codex](https://learn.chatgpt.com/docs/build-skills) | Verify the common text package separately from client loading. Publication does not authorize tool execution. |
| [Langfuse Sessions](https://langfuse.com/docs/observability/features/sessions), [LangSmith Threads](https://docs.langchain.com/langsmith/threads) | Relate evidence by session and Run, distinguish captured evidence from gaps, and omit environment restoration. |
