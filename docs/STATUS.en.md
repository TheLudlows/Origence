# Origence · Current status

**English** · [简体中文](STATUS.md)

Updated 2026-11-27. The local implementation is based on main `a542ff0`. Origence is the unified data foundation for AI agents, covering memory, knowledge, Skills, and Trace. See the [product roadmap](ROADMAP.en.md) and [architecture plan](design/agent-data-foundation.en.md) for first-release goals.

AML evaluation-protocol HTTP endpoints and startup switches have been removed. Owning workspaces and message batches remain available via the [common Rust library APIs](API.en.md#common-owning-workspace-and-message-batch-library-apis).

Claude Code and Codex passed cross-client reads of session records under the same key and independent Run acceptance. Bidirectional Skill reuse worked on public synthetic tasks. The official pilot with unseen tasks and independent business sign-off remain open. [#87 Two-client integration](https://github.com/TheLudlows/Origence/issues/87) · [#91 Pilot and verification records](../evals/pilot/README.en.md)

| Area | Implemented | Pending |
| --- | --- | --- |
| Memory | Explicit writes and versioned corrections, identity conditions and exact filtering, controlled extraction, source references and withdrawal. Opposing extracted values become separate evidence. Explicit confirmation can update a published single value. See the [memory contract](contracts/memory.en.md). [Long-term memory](contracts/memory-consolidation.md) accepts fact and experience sources through one interface and recalls both together. | P0 real-task acceptance, consolidation quality with a real model |
| Knowledge | Text, Markdown, and PDF ingestion, version reads and comparison, publication and withdrawal. Failed updates retain the original published version. See [API](API.en.md#endpoints). | P0 real-task acceptance |
| Skill | [Draft and submit selected evidence from a Run](SKILL-CANDIDATE-CLI.md#draft-from-a-run-in-the-console), [selected-experience CLI](SKILL-CANDIDATE-CLI.md), selected review packages, exact approval, immutable publication, online downloads, and exports for both client directory formats. Writers can review and publish. A public Windows example verified native loading in both clients and refusal of new exports after source withdrawal. [Test record](../evals/pilot/native-skill.windows.json) | Cross-client reuse and independent verification with unseen tasks in both scenarios, managed online reload |
| Trace | Private Runs, preparation receipts, explicit HTTP/MCP reporting, default Run hooks in the Codex and Claude Code plugins, owner-transcribed external verification, exact version citations, and archived PDF page locators. [Run contract](contracts/runs.md) | Full event coverage on both clients, end-to-end evidence verification, independent sign-off |
| Retrieval and context assembly | AnySeek full-text, vector, and one-hop graph candidates with authoritative revalidation. Default keyword search requires all query terms. Local real-engine regressions passed. Regular context includes budgeted consolidated facts with business dates, timelines, and fixed citations that the console can verify. [EvidenceBundle contract](contracts/evidence-bundle.md) | Model retrieval quality, capacity, and latency acceptance on the current backend |
| Sessions and feedback | AnySeek holds new private turn bodies and directory data by logical key. **Sessions** shows saved questions and answers in the console. Supports ID and ordered reads, keyword and vector recall, and explicit feedback. Membership-based workspaces allow authorized deletion, and single-key workspaces retain original conversations. Experience can use human confirmation, a pinned document version, or Improve. | P0 session and feedback evidence in both scenarios |
| Node.js management UI | Workspace key sign-in, deployment administrator inspection across workspaces and global knowledge review, scoped catalogs, asynchronous ingestion, pinned versions and sources, and Run lists and creation trends. The Run overview shows the active workspace ID as selectable text. Account sign-in remains available for existing membership-based workspaces. [#76](https://github.com/TheLudlows/Origence/issues/76) is closed. | End-to-end traceability on frozen tasks is part of [#91](https://github.com/TheLudlows/Origence/issues/91). |

See [VALIDATION](VALIDATION.en.md) for exact environments, commits, and results.

## Publication and experience copies

Ordinary model-free publication persists only its generation, operation epoch, and digest for retry. It rebuilds from fixed inputs and verifies the original output before writing. Jobs that use models retain their exact pending outputs and drop them in the SQLite publication transaction. Message batches retain exact receipt metadata. Completed Improve runs retain the proposal count, verification decisions, and child receipts. New experience source events render text from the immutable candidate. See the [publication and cleanup design](design/current-implementation.en.md#storage-consistency).

Run replay reconstructs context text from the fixed evidence and verifies the original digest. Run owners can store an external verification reference by itself. Skill publication points to the same files approved for review.

SQLite still stores immutable Learning candidates and the regular evidence items needed for exact Run replay. Replay reads private conversation text from AnySeek.

Standalone AnySeek deployments accept collection Info after removal of the Record count limit and still validate advertised legacy limits and the query candidate limit. See [VALIDATION](VALIDATION.en.md#2026-11-27--standalone-anyseek-collection-limit-compatibility) for pinned versions and evidence.

## Single-key workspaces

A fresh database supports offline provisioning of single-key workspaces. The credential signs into both Agents and the console, provides workspace content access, and can delete memories. Original conversation turns support append and read. Successful archival queues automatic extraction on the durable worker. Replays keep the same job, including after memory deletion. Workspace key rotation preserves historical data and accepted jobs. The administrator account can create peer workspaces, rotate their keys, inspect sessions and Runs, and create, update, or delete memories and knowledge in the selected workspace. Administrators can also moderate fixed versions of global knowledge. Approved copies are recalled alongside workspace results. See the [deployment management and global knowledge APIs](API.en.md#deployment-administrator).

## Origence local and cloud startup

`origence serve --local` opens the AnySeek Catalog at `OC_DATA_DIR/anyseek` in the same process through the Rust SDK. The host handles startup recovery, verification of published collections, and graceful shutdown. The Windows development script uses this entry point and optionally starts the Node UI. The cloud image runs `origence serve` against standalone AnySeek. The deployment environment provides the address and credential. [Operations guide](OPERATIONS.en.md#anyseek-full-text-and-vector-retrieval)

`origence-setup` connects Claude Code and Codex to an existing API with a workspace key. `origence-deploy` manages the local API, workspace initialization, and console. Each command rolls back its own changes and preserves data. [Installation guide](INSTALLER.en.md)

## Streamable HTTP MCP

The Claude Code and Codex plugins use Origence's `/mcp` endpoint for retrieval and other explicit tool calls without a local Origence executable. The server checks the key and workspace permissions on every request and uses the key's default workspace when none is specified. The plugin's automatic Run hook still reports through a local command. See [client setup](CLIENTS.en.md#direct-http-mcp) and [verification](VALIDATION.en.md).

Workspace keys and membership-based workspace administrators can open **Agent connections** to see their workspace's in-flight MCP
requests and activity from the last five minutes, grouped by credential with
the client kinds seen in handshakes. Observations are local to the serving
process. See the [Agent connections API](API.en.md#agent-connections).

## #105 Local client hook implementation

Codex uses `turn_id` to track turns, and Claude Code uses `prompt_id` for individual prompt contexts. Both client plugins install retrieval MCP and a default Run hook. The hook follows the MCP workspace selection and uses a credential that can write. It reports metadata through the same bounded private queue and Run API. MCP shares a Run only when the caller explicitly supplies the same Run identity. See [client integration](CLIENTS.en.md#verify-the-default-run-hook) for setup and coverage. The native Windows runner passed installation, workspace selection, pinned retrieval, and the default hook with synthetic data. See [VALIDATION](VALIDATION.en.md). The formal freeze is blocked on tasks, model authorization, and independent roles.

With a writable key, `origence-setup` [automatically archives sessions for the current project](CLIENTS.en.md#automatic-session-archiving-by-project) by default.

Applications driving either client can use [managed Run reporting](MANAGED-RUNS.en.md)
for Codex App Server command metadata and Claude Agent SDK cancellation.

## Legacy multi-workspace credentials

Administrators use the console to rotate the single key of the selected workspace. Existing member accounts list workspaces from directly configured credentials. Integrations that rely on legacy multi-workspace credentials retain direct HTTP and Rust library access. Sessions remain scoped by workspace and logical key. See [identity and credential management](API.en.md#identity-and-credential-management).

## AnySeek service credential and workspace collections

Configure only `OC_ANYSEEK_URL` and `OC_ANYSEEK_API_KEY`. The authenticated tenant UUID is the AnySeek namespace, and the workspace UUID maps to a `ws_<无连字符 UUID>` collection (UUID without hyphens). No separate tenant or collection setup is needed. First use initializes the fixed schema and strictly verifies it by readback after concurrent creation or lost responses. Startup checks previously published collections and does not silently create replacements for missing collections or configuration drift. Binding `origence-content-backend-v2` pins text format, routing, and model configuration. The analysis digest is persisted on first collection use. AnySeek does not issue tenant keys. Origence user permissions and private sessions retain their original semantics. See VALIDATION.

## Unified text and index storage in AnySeek

Original and summary text and their provenance share one chunk Record with full-text, vector, and graph indexes. SQLite removed version/chunk and summary text columns. Follow-up review removed unused chunk terms and redundant summary model versions, and simplified candidate reads to ID sets. Source status, publication pointers, content hashes, and recovery state remain in SQLite. Summary terms live in a dedicated field on the same AnySeek chunk Record and are queried by the hybrid summary branch. Historical reads and recovery use immutable text in AnySeek and fail explicitly if missing or rewritten. They do not silently recompute from original events. Reads hold no SQL transaction during retrieval and recheck permissions, sources, and all native ranking participants before return. New directories and collections use `origence-sqlite-v2` / `origence-content-sha256-v1`. See VALIDATION for exact tests.

## SQLite authority and minimal metadata

SQLite and file-lock dependencies became optional. The core library works without default features, and pure core and Blob/PDF tests load no metadata database. The SQLite authority is verified separately with `--features sqlite`. The host consolidates repeated authorization queries at transaction entry. Keyword retrieval reuses its authorization and candidate snapshot. Empty read sets and subsequent no-candidate paths do not touch additional metadata tables. Current authorization and publication identity are still checked before commit and return.

Local relational data and durable jobs use only rusqlite and bundled SQLite. The old engine, dependencies, and public aliases are removed. Driver defaults are disabled without adding an ORM or asynchronous database library. The lockfile has 112 fewer packages net. The ledger retains composite-primary-key idempotency. After moving text out, persistent version hashes were restored and chunk content hashes added for cross-store verification. Authorization, sources, version history, frozen publication plans, and recovery status remain durable. See [storage contract](contracts/storage.en.md). Linux and Windows CI in the PR verified locks, WAL, transaction cancellation, and recovery using real SQLite. Details and unverified boundaries are in the PR and VALIDATION.

## #76 Related Runs local implementation

Synced with main `f62168b` (#81 SQLite). Content details can read Run references recorded by the current logical key for a specific content item version. The bounded index commits with events. Reads recheck authoritative references, content item, source, and ownership and return no Run body. Pinned v1 round trips, isolation from another administrator, withdrawal, pagination, and reopening are verified in [VALIDATION](VALIDATION.en.md). The index retains proof of the first reference and does not establish complete collection or trace readability. The new schema is fresh-only and rejects legacy databases without migrating them. The change merged in #83. Subsequently #99 added PDF page text locators, and #100 added Run creation counts and trends.

## Retrieval module separation (merged in PR #79)

Synced with main `3d474d9` with #77 UI and #78 diagnostics. Ranking and fusion for individual lanes are in `retrieval::rank`. SQLite transaction methods own the final publication and pre-return checks. Search and EvidenceBundle retain their separate retrieval, original snapshot boundaries, and external I/O order, as well as mainline diagnostic collection and Run replay protections. See [VALIDATION](VALIDATION.en.md) for post-merge and historical results.

## Retrieval configuration and diagnostics

This implementation is based on main `44e06df`. It checks analysis configuration and embedding space and durably binds digests. Search/Context return bounded query diagnostics and Run preparation stores success and failure observations. Success replay checks original authorization and sources before reading the private snapshot, without depending on models or AnySeek. Query diagnostics v2 checks the embedding space digest each time and rejects changes after startup. The latest 128 attempts and explicit omissions do not constitute full client collection or a cross-storage snapshot. [API fields](API.en.md#retrieval-diagnostics) CI retains the Node UI build, real UI gate, and pinned fixture cache. Cache and checkout use the same precise SHA for the diagnostics v2 backend. Final checks and real AnySeek integration results are in VALIDATION. Historical CI results remain associated with their original commits.

## Targeted local Run UI verification

Local changes based on `01d4f82` distinguish preparation success, server-observed preparation failure, and client reports. Real Chromium verified Run → pinned v1 → source withdrawal, clicking an old reference again, and denial for another key. See [VALIDATION](VALIDATION.en.md) for the scope of that local check.

## Next steps

Freeze clients, models, and task materials under [#91](https://github.com/TheLudlows/Origence/issues/91). Verify archives, Skill reuse, and traceability from Run through context or Skill to result, then obtain independent sign-off.

## Scope and evidence

Normal startup requires the current schema. Same-version durability, restart, retry, and [paired backup and restore](OPERATIONS.en.md#private-archive-and-paired-recovery) remain required. Original sessions and unpublished candidates are private to their logical key. Workspace admins can view Run metadata, reported outcomes, creation statistics, and recorded traces for their workspace. Run writes, replay, and private source reads retain owner checks. Authorized members in the same workspace can reuse published memory, knowledge, and Skills while supporting sources remain valid. Private session evidence can limit Skill review or distribution to other keys. See the [Run contract](contracts/runs.md). Enterprise identity and per-document ACLs are not implemented. The platform supports archiving and query without restoring a client's work environment. Publishing a Skill conveys no tool execution permission.

## Local Windows development launcher

Based on main `c70a1aa`, added a PowerShell 7.4+ foreground launcher for AnySeek → Origence → optional Node UI using an initialized directory and existing environment credentials. First-time initialization is documented separately. Linux pwsh mock-process regressions and real service startup, shutdown, and reopening were verified. **Native Windows was not tested.** See [Windows development](WINDOWS-DEV.en.md) and VALIDATION for commands, readiness, and stop limits. This local delivery does not change deployment, production recovery, or existing CI scope.
