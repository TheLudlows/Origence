# Origence API and library interfaces

**English** · [简体中文](API.md)

Covers the SQLite control state of a local host and AnySeek projections for text, summaries, full text, vectors, and one-hop graphs. This page defines the HTTP v1 and public library contracts. See [STATUS](STATUS.en.md) for progress, [OPERATIONS](OPERATIONS.en.md) for procedures, and [VALIDATION](VALIDATION.en.md) for exact revision evidence.

Jump to [library interfaces](#rust-library-interfaces) · [endpoints](#endpoints) · [job and read visibility](#jobs-and-read-visibility) · [retrieval](#retrieval) · [explicit identity](#explicit-memory-identity).

Every `/v1/*` request requires `Authorization: Bearer TOKEN`. Business requests can send `X-Origence-Workspace: <UUID>` to select a workspace authorized for that logical key. Omission selects the workspace set at issuance. An unauthorized explicit selection returns 403. An unavailable default or invalid header returns 422. Use `GET /v1/workspaces` to list the key's current grants.

Existing command-style mutations need a 1–200 ASCII character `Idempotency-Key`. Run and Skill use the stable IDs, revisions, and digests below. The idempotency scope is workspace + key principal + operation/target + idempotency key. Repeating an identical request reuses the result. The same key with different contents returns 409. Idempotent responses include identifiers and state only. Replaying after deletion neither re-writes data nor returns deleted text.

## Private sessions, Trace, and Skills

`GET /v1/session` returns `{workspace_id,key_id,session_id}` and resolves a separate private session for each workspace and logical key. Existing `/v1/sessions/{session_id}/turns`, feedback, and Improve endpoints accept only the current key's archive and retain the writer requirement. Published Learning text and conditions are available for retrieval in the same workspace. Private archive, Improve, and support record pointers are returned only to the original logical key. Admins can rotate credentials in their issuing workspace. Rotating another key also requires admin access in every additional workspace granted to that key. The replacement retains the original logical key and its grants and revokes the old credential. Clients use the same private boundary through [`context_session` / `context_turn_archive` / `context_turn_get`](CLIENTS.en.md).

Trace is stored as private Runs. Run entry points derive the logical key from authentication and apply current permissions and source validity checks.

| Run endpoint | Input and result |
| --- | --- |
| `POST /v1/runs` | Writer supplies a stable `run_id` and redacted `summary`, bound to the current archive |
| `POST /v1/runs/{id}/events` | Stable `event_id`, timestamp, reporter, and named event. An identical retry returns the original sequence. |
| `POST /v1/runs/{id}/context` | `event_id` and `context` (EvidenceBundle request). Returns the bundle and a server preparation receipt. |
| `GET /v1/runs/{id}?after=0&limit=50` | Paginated trace for the current key, including explicit collection boundaries |
| `GET /v1/runs/{id}/prefix?through_sequence=N` | Immutable prefix digest for the current key, usable as Skill evidence |

The server receipt proves preparation of the exact context, without proving client receipt or model use. The client reports tools, loads, results, feedback, and artifact references. Unreported activities remain unknown. When any source becomes invalid, reference verification prevents further delivery. The Run owner submits external verification over HTTP for an exact prefix, retaining the reported transcript source and verifier identity status. See the [Run contract](contracts/runs.md) and [external verification contract](contracts/run-verification.md).

| Skill endpoint | Input and result |
| --- | --- |
| `PUT /v1/skill-candidates/{id}` | `{expected_revision,candidate}`. Use 0 to create. Changes generate a new revision. |
| `GET /v1/skill-candidates/{id}` | Only the creator can read the candidate, private notes, and exact basis. |
| `POST /v1/skill-candidates/{id}/reviews/{review}` | Select reviewer, revision, publishing target, and redacted basis. |
| `GET /v1/skill-reviews/{review}` | Selected review package and decision, without access to original sessions or Runs. |
| `POST /v1/skill-reviews/{review}/decision` | The designated reviewer uses `{digest,approve}` to approve or reject the exact contents. |
| `POST /v1/skill-reviews/{review}/publish` | A principal with publishing permission publishes the approved immutable version using `{digest}`. |
| `GET /v1/skills/{id}/versions/{version}/package` | Verifies scope, version, and source and returns a package containing only approved artifacts. |
| `DELETE /v1/skills/{id}/versions/{version}` | A role with publishing permission withdraws the version and blocks later online export. |

Changing the candidate revision, the review package contents, or the destination requires a new approval. Original sessions and review evidence are not distributed with packages. The caller submits a candidate, or binds a manually drafted method to an exact source using the selected-experience CLI. See the [Skill contract](contracts/skills.md) and [selected-experience CLI](SKILL-CANDIDATE-CLI.md) for package format and boundaries.

## Retrieval diagnostics

Search, Resolve, and EvidenceBundle return `retrieval:{requests,omitted_requests}`. Each row represents one real AnySeek query. It includes Origence and backend request IDs, route, actual mode (browse/lexical/vector/hybrid), elapsed call time, safe error code, returned candidate count, and available backend diagnostics. Standalone service queries record HTTP status. For embedded SDK queries, `http_status` is null, and SDK failure is recorded as `sdk_error`. Browse includes candidate preflight.

Backend diagnostics v2 include service version, analysis configuration digest, embedding space SHA256 (null if unset), query time, `visible_seq`, and separately sampled committed/lexical/vector progress with failure and recovery flags. Configuration also records verified capacity limits and the embedding space digest. Each query compares returned analysis and space digests and rejects post-start space changes. Sequences are per AnySeek collection, with no workspace sequence, SQLite version, or cross-store snapshot semantics. Client round trips and server timing cover different intervals. Neither proves client consumption or model use.

Each call retains only the 128 most recent attempts and explicitly returns the earlier omission count. Diagnostics omit raw queries, vectors, filter values, text, URLs, keys, and raw backend errors. Unknown or missing fields remain null. Malformed or mismatched backend diagnostics are rejected. Deterministic context content and references exclude changing request IDs and timing.

EvidenceBundle `retrieval` may include `context_query_embedding`. It captures only input usage for embedding that Origence context query. Fields are `model_config_sha256` (the configured embedding profile digest) and `provider_reported_prompt_tokens` (an unsigned 32-bit integer from the provider response's `usage.prompt_tokens`). If the provider explicitly reports 0, keep 0. Missing, invalid, or unavailable values are null. Do not substitute `total_tokens`, output tokens, or UTF-8 bytes. An absent field means the observation was not collected, for example for keyword-only queries or older records. Do not infer overview, whole-turn Agent usage, or cost. A standalone context preview may return the observation without attaching it to a Run.

`POST /v1/runs/{id}/context` writes successful diagnostics and the exact bundle digest to the private Run. Preparation failures after authentication, ownership, and input checks record `context_preparation_failed` as server_observed. The first success also stores a private context snapshot. For identical replay, validate the event and request digest, recheck current permission, original references, and private session validity, then return the original bundle and receipt without model or AnySeek calls. Temporary backend outages and corpus changes do not rewrite it. The snapshot is omitted from paginated Run traces. A changed request digest conflicts. Source withdrawal or lost authorization blocks replay. Failure events are also immutable, so a new preparation after recovery needs a new event_id. Ordinary MCP/Search calls do not auto-attach to a Run. Call preparation explicitly. Uncollected activity remains unknown.

## Unified AnySeek retrieval

A workspace's full-text, vector, and one-hop graph candidates use one AnySeek collection. The adapter encodes Origence's pretokenized text as fixed SHA-256 terms and preserves AND semantics. Origence generates vectors, and the directory binding fixes the model profile and dimension. AnySeek stores published chunk text and summaries. SQLite holds permissions, published versions, sources, and content hashes. Blob preserves original files. Origence verifies text, permissions, versions, and sources before return.

Before external I/O, the publication plan persists the original operation epoch. Each immutable publication is limited to 64 chunk Records and 2 MiB of encoded request data. Graph metadata attaches to one deterministic existing chunk and adds no Record. The publication commits as one group. A new business version uses a separate group. On unknown network outcomes inspect the complete group, and jobs reuse the durable plan. Residual indexes cannot revive withdrawn sources. Queries filter by scope, full source identity, and authorized chunk IDs and recheck authoritative state before return. Vector candidates also check the model profile and dimension. A failed dependency returns unavailable and never becomes empty retrieval.

See [OPERATIONS](OPERATIONS.en.md#anyseek-full-text-and-vector-retrieval) for deployment and directory binding and [VALIDATION](VALIDATION.en.md) for results.

## Rust library interfaces

- Core without a metadata database. `default-features = false` provides types, parsing, models, Blob, and the AnySeek interface.
- SQLite authority adapter. `default-features = false, features = ["sqlite"]`.
- The full HTTP/MCP host defaults to `embedded-anyseek`, including `local-storage` and `sqlite`. For the remote cloud host, disable default features and enable `local-storage`.

Use `cargo test --no-default-features` to test the standalone core. The authority-only check now explicitly uses `cargo test --no-default-features --features sqlite` and still runs the full authorization, queue, and recovery regressions.

The local embedded host is built through `Service::open_embedded(dir, models)`. A host with remote AnySeek uses `Service::open_with_anyseek(dir, models, config)`. Generic callers use the public Service and DTOs, while transactions and related queries stay in storage adapters. See the exports in [lib.rs](../src/lib.rs) and source for complete signatures.

The old standalone graph storage API and graph traversal were removed. `StorageEngine::new` no longer accepts graph storage, and `SqliteStore::cleanup_plan` returns only chunk IDs for cleanup. The existing HTTP/MCP graph-retrieval contract remains.

- `SqliteStore::check_memory_identity(path)` performs a full same-version schema check on an existing path under a read-only exclusive lock. It neither creates nor repairs tables. CLI `--offline identity-check`.
- Command idempotency, job execution, and artifact owner/index/ledger responsibilities are distinct. See [storage invariants](contracts/storage.en.md).
- The owning workspace and message batch APIs are below. External protocol fields cannot enter generic storage or authorization decisions.

## Roles

| Role | Permissions |
| --- | --- |
| reader | get/search/resolve on published content |
| writer | reader plus upload, original download, knowledge ingestion, structured memory publication, capture, archive and feedback, job read, and cancellation or retry of own jobs |
| reviewer | writer plus version restore and operations on other principals' jobs |
| admin | reviewer plus content item deletion, source withdrawal, file deletion, and retries of failed cleanup |

Original files and jobs can expose unpublished data, so reader cannot access them. Writer authorizes direct memory and capture publication. Publication rechecks permissions and sources before commit.

## Endpoints

A content item here is a versioned memory or knowledge entry identified by `asset_id`. Published Skills use separate endpoints.

### Read-only management catalogs

- `GET /v1/assets?kind=memory|knowledge&limit=25&after=<UUID>`. Reader and above, with workspace from authentication. Scan current published nondeleted candidates by ascending `asset_id` and reuse the full ordinary source, semantic, and visibility checks for each. Returns `{items,next_after}`. Items contain metadata only, without text or semantic quotes. Invalid sources are excluded and other storage errors fail explicitly.
- `GET /v1/runs?limit=25&after=<UUID>`. An admin lists Runs in the selected workspace (`scope:"workspace"`). Other roles list their own Runs (`scope:"current_logical_key"`). Each row includes `run_id`, `owner_key_id`, `is_owner`, `created_at_ms`, `status`, and `summary`. The latest `result_reported` event supplies `reported_success`, `reported_failure`, or `unknown`. Other keys' summaries are `null`. Details and evidence endpoints still verify ownership. Authorized rotation preserves logical ownership.

Both endpoints default to limit=50, range 1–100. With no after, begin at the start. Unknown parameters fail. Ordering is by UUID instead of creation time. `next_after` is the last scanned candidate, not a total. Pages may have fewer than limit or be empty if sources are invalid. Continue while next_after is non-null. Each page is a live authorized read without a frozen snapshot across requests. Reads create neither sessions nor publishing jobs.

Overview uses separate `GET /v1/runs/overview?window=24h|7d`, default `24h`. It counts durable Run creations in the caller's Run catalog scope. Returns `scope`, `window`, `timezone:"UTC"`, `start_ms`, `end_ms`, `created_runs`, `series` and `metering`. `series` contains 24 one-hour buckets or 7 24-hour buckets in ascending time. Each bucket has `start_ms`, `end_ms`, and `created_runs`, including zero counts. Bounds are Unix milliseconds. Windows and buckets are half-open and roll backward from the current server read time. Counts survive authorized rotation. Events, retries, and evidence withdrawals do not add or remove creations. Window parameters cannot override authorization scope. These queries use scope/time indexes in the SQLite authority database. See [Run overview](contracts/runs.md#run-creation-overview) and [whole-Run metering reports](contracts/runs.md#whole-run-metering).

Memory catalogs support `memory_filter=<URL 编码 JSON>` (URL-encoded JSON), for example `{"subject":{"kind":"service","stable_id":"Atlas"},"predicate":"release.rule","context":{"env":"production"}}`. The library entry point is `Service::list_memories(auth, &MemoryCatalogFilter, after, limit)`.

- `subject` is optional. If supplied, it must include an existing `kind` and precise application `stable_id`. Names and keys are not inferred.
- `predicate` is optional. If supplied, validate under the identity contract and match exactly.
- Omitting `context` leaves it unrestricted. If supplied, compare the entire set of string keys and values, ignoring key order while preserving case and Unicode. `{}` matches only empty conditions, without wildcard or subset semantics.
- Omitting `identified` leaves it unrestricted. `true` matches stored explicit identities only. `false` matches content items without identity. `false` cannot be combined with subject, predicate, or context. Unknown identities are not inferred. Reject unknown fields and invalid combinations.
- Only `kind=memory` accepts this field. The JSON string limit is 16,384 bytes.

Filtered catalogs are bounded live candidate cursors without full counts. Each call checks up to 100 candidates (plus one lookahead) after the scoped primary key cursor, including non-memory, unpublished, or hidden candidates. Apply exact filters and ordinary source and semantic visibility checks before return. `limit` caps results and pages can be short or empty. Continue while `next_after` is non-null and reset the cursor after changing filters. Scanning never grows unbounded to fill a page. Omitting `memory_filter` preserves existing catalog behavior. Memory UI uses server-side filtered pagination rather than page text filters or search hits. The knowledge page's “filter loaded content items” remains an explicit local filter.

The Node.js UI uses these and the HTTP endpoints below. It never opens databases or runs CLI commands directly.

| Method and path | Request / result |
| --- | --- |
| `GET /health/live` | Process liveness, unauthenticated |
| `GET /health/ready` | Single Worker running and local storage checks passed. External model health is separate. |
| `POST /v1/memories` | `{fact_key,content}` (`publish_if_authorized` is deprecated and accepted without effect) → `asset_id`, `source_event_id`, and `job_id`, `state:"accepted"`. `conflict` indicates append to an existing fact. |
| `POST /v1/memories/identified` | `{identity:{subject:{kind,stable_id},predicate,context:{}},content}` → `asset_id` / `source_event_id` / `job_id`. The same scoped identity reuses the content item. `conflict` indicates an existing version. |
| `POST /v1/memories/lookup` | Complete `{subject,predicate,context:{}}`, readable by reader. Returns the same current published view as `GET /v1/assets/{id}`. No Idempotency-Key or content item creation. |
| `POST /v1/captures/semantic` | `{content,targets:[{identity,expected_version,allow_update:false}]}` → source/job. Bounded model classification reuses memory semantics, described below. |
| `POST /v1/captures` | `{content}` → source/job. Extracted facts publish individually. See job `result.memories`. |
| `POST /v1/captures/identified` | `{identity,content}` → `asset_id` / `source_event_id` / `job_id`. Caller supplies the complete identity and the model only extracts source spans. Requires an extraction model. |
| `POST /v1/knowledge` | `{title,content或file_id,format:"text"或"markdown"或"pdf",asset_id:null或UUID,expected_version:null或整数}` |
| `POST /v1/files?name=...&format=...` | Raw binary body without multipart. Returns file_id. Filename is metadata only. |
| `GET /v1/files/{id}` | Verifies hash and returns attachment/octet-stream. Storage paths derive from scope and content hash. |
| `GET /v1/assets/{id}?version=N` | Current or historical published version with content/hash/title/source/restored_from |
| `POST /v1/assets/{id}/restore` | `{target_version,expected_version,reason}` → restore job. Completion adds a new version. |
| `DELETE /v1/assets/{id}` | Permanent logical tombstone blocks reads, including historical versions. |
| `GET /v1/events/{id}` | Writer and above reads valid original source `{event_id,kind,content,file_id,created_at}`. Reader gets 403. Cross-scope, withdrawn, and file-deleted sources return 404. |
| `DELETE /v1/events/{id}` | Withdraws source and blocks all referencing versions without automatically falling back. |
| `DELETE /v1/files/{id}` | Blocks the original file and withdraws all events referencing it. |
| `GET /v1/jobs/{id}` | operation/state/generation/outcome/result/error_code |
| `POST /v1/jobs/{id}/cancel` | Marks cancelled and advances generation/run_token so old execution cannot commit. Completed jobs and cleanup cannot be cancelled. |
| `POST /v1/jobs/{id}/retry` | Only failed/cancelled with valid source and content item. Atomically enqueues new generation. Cleanup retry needs admin. |
| `POST /v1/sessions/{session_id}/turns` | `{turn_id,question,answer,used_evidence:[]}` → synchronous archive ID. Writer and above. See session archiving. |
| `GET /v1/sessions/{session_id}/turns/{turn_id}` | Writer and above reads the complete valid question and answer, archive references, and feedback. |
| `POST /v1/sessions/{session_id}/turns/{turn_id}/feedback` | `{feedback_id,target,rating,comment?}` → synchronous record ID, with no weight changes or model trigger. |
| `GET /v1/sessions/{session_id}/turns/{turn_id}/feedback` | Writer and above reads the valid turn's feedback list. |
| `POST /v1/search` | `{query,limit:10,mode:"keyword",allow_partial:false}`. mode supports keyword/vector/hybrid. |
| `POST /v1/resolve` | `{query,budget_tokens:2000,mode:"keyword",allow_partial:false}` → rendered_context and sources |

Knowledge updates change the current version and title only after asynchronous publication succeeds. Supply either a file or text. PDFs require file_id. Each historical version keeps its own title. Restore copies content and title and adds `restored_from` without overwriting history or skipping source checks.

## Identity and credential management

| Method and path | Semantics |
| --- | --- |
| `GET /v1/whoami` | Returns credential id, tenant_id, selected workspace_id, and effective role. |
| `GET /v1/workspaces` | Lists explicit active grants with workspace_id, name, role, and is_default. |
| `GET /admin/keys` | An admin lists unrevoked credentials issued by the selected workspace. Returns credential and logical IDs, role, default access, owned-workspace capability, and creation time. |
| `POST /admin/keys` | Admin sends `{workspace_id,role}`. The workspace must match the request's selected workspace and becomes the new key's default. Returns `{key_id,token}`. |
| `POST /admin/keys/{id}/rotate` | The issuing workspace's admin rotates the current credential ID, subject to authorization for its additional grants. Keeps the logical ID and returns a new `{key_id,token}`. |
| `GET /admin/workspace-key-grants` | An admin lists explicit additional grants in the selected workspace, with each logical ID, role, and credential activity status. |
| `PUT /admin/workspace-key-grants/{logical_key_id}` | A workspace admin assigns `{role}` in the selected workspace to a logical key from the same tenant. |
| `DELETE /admin/workspace-key-grants/{logical_key_id}` | A workspace admin removes access to the selected workspace. |
| `DELETE /admin/keys/{id}` | An admin in the issuing workspace revokes the credential for all workspaces. |

Grant endpoints use the stable `key_id` returned by `GET /v1/session`. Trusted service access to owned workspaces remains available through the dedicated library interface.

Both admin lists take `limit` (1–100, default 50) and a UUID `after` cursor and return `{items,next_after}`.

`/admin/keys` also needs a Bearer token. It omits the business idempotency cache to avoid storing plaintext tokens. Issuance returns the token once, and repeated POST creates a different key. After a lost response, check database key IDs through offline administration and revoke redundant credentials. Offline workspace and key commands use `--offline` and OS directory permissions and fail to open while the host is running.

## Version conflicts

Admission records the current content item version, which the Worker rechecks before commit. If another version publishes first, the job becomes superseded and cannot overwrite it. Read again before resubmission. Publication plans and ledger make replay idempotent. Explicit identity also supports [caller-supplied version preconditions](#caller-version-preconditions-for-explicit-writes).

## Jobs and read visibility

`pending → processing → completed/failed/superseded`. Transient storage failure follows `processing → retry_wait → processing`. Cancellation is `cancelled`. Only completed + outcome=published indicates atomic commit of that version and index. capture result.memories lists each extracted fact's content item and version. Later updates, deletions, or source withdrawals can still occur.

A deletion response is `{id,blocked:true,cleanup_job_id,originals_retained:true}`. Once the logical deletion commits, new reads are blocked regardless of index cleanup. Already delivered data cannot be recalled. Requests started before deletion can finish first.

Business write transactions start with SQLite `BEGIN IMMEDIATE` and atomically commit business state, idempotency, audit, and enqueue. Model/file/native index I/O runs outside the relational transaction. Final commit revalidates permissions, tombstones, sources, expected version, generation, and run_token. Key revocation serializes with write transactions. This local adapter has no RLS and binds scope explicitly.

Save the publication plan and pending ledger in SQLite first. After full-text/vector/graph writes, commit versions, owners, ready indexes, committed ledger, and job completion together. External artifacts alone are not published evidence. Transient storage failures enter `retry_wait` for up to 5 attempts. Model and input failures need explicit retry. Exclusive host restart recovers processing jobs and advances run_token. Reconcile and clean up before accepting requests.

For cleanup, `completed + outcome=ok` means this round of derived-data cleanup finished. Original events, versions, and files remain. This is not physical erasure of source text.

## Retrieval

Hits contain asset_id/version/chunk_id/source_event_id/locator/score. Locators identify UTF-8 byte ranges and line numbers of original text, plus PDF page numbers. Only currently published versions are returned. Use get for explicit historical versions.

For a pinned PDF citation in a private Run, use `GET /v1/runs/{id}/pdf-source?sequence=N&evidence_ref_id=UUID` to read verified page text. Requires Run ownership and existing access to the original file. Returns the exact content item version, known page and verifiable text, or explicitly indicates no page locator. `original=true` retrieves the corresponding original. [Run contract](contracts/runs.md#pdf-page-preview-from-a-private-run)

Vectors must match the configured profile. Models and dimensions cannot be mixed. On embedding query failure, fallback to keyword requires `allow_partial=true` and returns `effective_mode` and warnings. Otherwise return 503. Historical keyword-only publications have no vectors and are absent from vector results, but the keyword lane of hybrid can retrieve them. There is no automatic profile backfill or reranker. Vector queries rank authorized candidates in AnySeek. Ranking equivalence and comparable performance with the former backend or production distribution are not guaranteed.

resolve returns `tokenizer=utf8-bytes-upper-bound-v1` and `count_is_estimate=true`. count is the UTF-8 byte count of rendered_context. The budget is 0–32000. Skip whole chunks exceeding the budget, and do not emit citations without text. Returned text is external evidence and cannot be treated as system instructions.

Original-text keyword search uses encoded terms and AND matching in AnySeek, requiring all query terms and restricting current scope, publication generation, and full source version. Summary keeps AND semantics after aggregating summary lines for a chunk. Missing or inconsistent candidate identity fails explicitly instead of silently lowering recall. Hybrid maps keyword/vector/summary to chunk evidence for RRF and expands one hop from entity-name seeds. Each graph object has `evidence` (asset_id/version/chunk_id/source_event_id/locator). SQLite owner and committed ledger determine evidence validity. Withdrawn source descriptions are excluded.

search/resolve optionally accepts `components:{summaries:true,graph:true}`, both default true, used only with hybrid and no memory_identity restriction. Summary and graph branches can be switched off independently for same-database ablation. Original keyword/vector lanes remain. Omitting components retains existing behavior. `active_components` indicates enabled branches, regardless of whether artifacts or hits exist. resolve uses the same switches and source and byte budget checks. HTTP and MCP share this contract.

The graph lane uses AnySeek metadata filters to find entity names containing query terms, then finds incoming or outgoing edges for any initial entity ID. SQLite rechecks source, version, and generation. Withdrawn or unpublished projections cannot be used as evidence. Relations retain their original direction. Expanded entities do not expand again. Graph fields enter neither lexical terms nor vectors and add no records to BM25 statistics. Exceeding the server candidate cap fails explicitly without silent truncation. Engine-side graph candidate budgets are independent of the response cap of 20 entities and 20 relationships. This path does not change `/v1/context` or add a standalone graph API.

Only knowledge ingest with an extraction model produces summaries and graph projections. Short fact publish and restore do not create graphs automatically. A failure in enabled processing fails the whole job, with no partial graph readiness. Query embedding failure can fall back according to allow_partial. Storage failures cannot bypass authorization or source verification via fallback.

## Errors

Domain errors use `{error:{code,message}}` without exposing SQL, passwords, model responses, or document text. 401 unauthenticated, 403 forbidden, 404 absent or invisible, 409 version or idempotency conflict, 422 invalid input, 503 model or other dependency unavailable, and 500 internal or database error. Before the handler runs, Axum uses framework defaults for JSON, path, and request size errors, for example 400/413/422.

## Explicit memory identity

`POST /v1/memories/identified` example.

```json
{"identity":{"subject":{"kind":"service","stable_id":"billing"},"predicate":"release.approval","context":{"environment":"production"}},"content":"生产发布需要两人审批"}
```

Supported subject kinds are user/agent/project/service/team. stable_id is a stable application identity, distinct from the API key principal. Identity includes authenticated scope, subject, predicate, and sorted context, without content or version. Unknown fields are rejected. Predicates use lowercase ASCII. Context values compare verbatim without alias or semantic normalization. The same identity appends a version. Different subjects, conditions, and workspaces stay separate. Concurrent writes use optimistic expected_version checks, with job state authoritative.

Content item reads add `identity`, null for legacy memory or knowledge. Identity is bound to a content item and immutable, and cannot be recreated after deletion. Legacy fact_key/capture writes cannot modify an identified content item and return 409. If a legacy fact key collides with new identity encoding, the new entry point returns 409 without converting it.

lookup accepts the identity above without content and matches exactly in the key's scope. Aliases and semantic approximations are unsupported. Missing, unpublished, deleted, or source-withdrawn content items return 404, without falling back or creating content items, sources, or jobs. A missing identity table is an error, not a valid miss or empty result. The returned version is not a write lock. The final job version check remains authoritative, and reader cannot access original capture input through lookup.

New databases always contain the complete identity schema. Startup rejects absent tables and incompatible databases. It neither falls back to legacy-only endpoints nor installs or upgrades automatically. See [OPERATIONS](OPERATIONS.en.md#offline-memory-identity-check) for the read-only check. Legacy captures still use fact_key and explicit write/capture uses HTTP. Read-only MCP supports memory_identity filtering. CLI does not set it by default.

## Capture for one identity

`/v1/captures/identified` accepts the same `{identity,content}` as explicit memory writes. content is the original text for extraction. Identity comes from the caller and scope from the key. The model cannot choose subject, predicate, context, or workspace. Admission records the current expected_version. Other publication during extraction supersedes the job under existing version rules, so extraction cannot overwrite a newer version. Missing extraction model returns 503. Startup rejects missing identity schemas.

Model output must include an exact source quote and UTF-8 byte_start/byte_end. The server validates boundaries and content. The published body is that exact span, and search locator source_span=true marks offsets into original text. Equal duplicate quotes merge. Different quotes are ambiguous for one identity, returning job failed/INVALID_ARGUMENT without a version update. No spans completes with result.memories=[] and may leave only an unpublished reserved content item. The source event retains original input.

No automatic subject inference, alias normalization, arbitrary semantic merging, or multi-value property union is provided here. Exact source-span validation establishes traceability without proving model selection or truth. Negation and proposal detection depend on the model and require real evaluation. Existing capture endpoints keep the fact_key model.

## Identity status and source verification

`GET /v1/assets/{id}` returns normalization_status. Explicitly bound memory uses explicit_identity. Conflict, negation, proposal, and ambiguous evidence retained by memory semantics or controlled extraction uses unnormalized. Legacy fact_key memory uses legacy_unidentified, and knowledge uses not_applicable. The status describes identity binding only, without assessing truth or model semantics. Historical versions use the same content item identity. Legacy memory is not normalized automatically.

Writer can use GET /v1/events/{id} to compare a search locator's byte range with the original source, including for failed or zero-result capture. Original input can contain unpublished content and reader cannot access it. Withdrawn sources, deleted file sources, and sources from another workspace are unavailable. Logical deletion retains underlying original text, with no physical erasure feature. Memory semantics supplies a separate status for unnormalized memories. Automatic identity merging is unimplemented.

## Identity annotation in retrieval and context

Each search hit adds identity (null if unbound) and normalization_status (explicit_identity/unnormalized/legacy_unidentified/not_applicable) as in content item reads. Full identity comes from the SQLite content item binding. The system does not trust model or vector store identity claims. Recheck current version, scope, and sources before returning.

resolve chunk citations and their JSON annotations before the text include kind, identity, normalization_status, and context_policy=identity-provenance-v1. Graph citation format is unchanged. Citations, titles, annotations, and text count against the byte budget. Chunks are included or omitted in full, so annotations can reduce text capacity. Status indicates binding and does not express truth, validity period, or confidence. Conflict detection and typed budgets are not implemented in this path.

## Exact identity retrieval filters

search/resolve accepts optional memory_identity as full `{subject,predicate,context:{}}`. Omission retains mixed retrieval. With the filter, only current-scope memory with an exact full identity is kept. Legacy unidentified memories, knowledge chunks, and graph expansion are excluded. Validate before model calls and reject aliases, semantic approximation, partial conditions, and unknown fields. Invalid identity returns 422 and valid misses return empty. The filter does not replace authorization, and versions, sources, and tombstones still apply. A complete current schema without an explicit binding has no match. A missing identity table is a storage failure and is neither an empty result nor automatically upgraded.

The complete identity first locates an authorized content item. Keyword/vector ranking and truncation only consider that content item, then recheck version, source, tombstone, and permissions. Unfiltered search still has branch candidate caps. Identity restriction does not guarantee semantic recall. Use lookup to locate the current content item by itself. Responses echo memory_identity, resolve forwards it, and HTTP/MCP support it. CLI omits the filter by default.

## Caller version preconditions for explicit writes

memories/identified and captures/identified accept optional nonnegative expected_version. 0 requires no published version, while an unpublished reservation may exist. Positive values must equal the current version in the acceptance transaction. A mismatch returns 409 and rolls back without creating source or job. Negative values return 422. Omitted or null uses the current version at admission as before and does not require creation.

Pass lookup's version to the write endpoint to cover the concurrency window between read and admission. After acceptance, the Worker still checks the admitted version. A change fails the job, and admission does not promise synchronous publication. A successful replay of the same idempotency key and request returns the original admission before checking later changes. A different expected_version changes the request and conflicts. Omitted and null are excluded from the canonical payload to preserve replay of existing keys.

## Console child workspaces

- `GET /v1/workspaces/owned?limit=50&after=<UUID>` lists the root's immediate children as `items` and `next_after`. Each item includes `workspace_id` and `name`. The limit ranges from 1 to 100.
- `PUT /v1/workspaces/owned/{id}` accepts `{name}` and returns `{workspace_id}`. Reusing the UUID reads back the existing child. A UUID already owned by a different workspace returns a conflict.
- Both endpoints require a live `admin` role in the selected root, including an explicit workspace grant. The list returns only accessible direct children and includes each child's effective `role`. Creation grants the creator's logical key explicit `writer` access in the same transaction if the key has no derived access. A caller can replay the receipt for an accessible child without changing its name or grants.
- `allow_owned_workspaces` controls derived access for trusted root credentials. Business requests using `X-Origence-Workspace` check explicit grants first. A trusted root credential with that capability can select an existing direct child with its derived reader or writer role. Targets outside that derived scope return 404. Records remain isolated by scope and logical key.

## Common owning workspace and message batch library APIs

The following capabilities are available through the public Rust Service API.

- `issue_key_with_capabilities(auth, role, KeyCapabilities { allow_owned_workspaces: true })`. Only the current admin of the target root may issue it. Existing `issue_key` defaults to false. Admin or a new key in the same root does not inherit it automatically. A key bound directly to a child cannot derive further workspaces.
- `ensure_owned_workspace(auth, target_id, name)`. A root writer or above with the explicit capability creates or verifies one level of owned workspace in the same transaction. Tenant or owner mismatch conflicts, with no rebinding or replacement ID. The returned AuthContext retains the original key and has only reader or writer permissions.
- `lookup_owned_workspace(auth, target_id)`. Check current root identity, capability, and read authorization first. A valid unknown target returns None without creating it. Ordinary key permissions do not expand.
- `submit_message_batch(auth, key, MessageBatch, AcceptedMessageSpec)`. Accepts an external batch_id, session_id, and ordered SourceMessage array. Atomically persists source, content item, job, and `oc_commands` receipt. Each batch uses workspace-scoped `message_batch.ingest` idempotency. Ordinary commands remain principal-scoped. The key is 1–200 ASCII bytes and callers cannot select the deduplication domain.
- MessageBatch batch_id/session_id are 1–4096 UTF-8 bytes without NUL. Each batch has 1–256 messages and a total body limit of 1,000,000 UTF-8 bytes under general text validation. role is a verbatim 1–64 UTF-8 byte label, not blank and containing no controls, for example system/tool, and grants no privilege. timestamp is optional signed i64 Unix milliseconds. Omitted/null values do not use current time, and timestamps do not reorder messages.
- `AcceptedMessageSpec::raw_only(profile)` pins `message-batch-ranges-v1`, parser_version=1, chunking_version=1, raw_only=true, and embedding profile. Admission writes the full specification into the original job payload. The full batch and spec enter the request hash. Reusing a key with changed content/session/order/role/time/parser/profile/mode conflicts without a new job. Recovery and explicit retry retain the original specification. Processing refuses model profile mismatch.
- `IngestReceipt` holds original `asset_id` / `source_event_id` / `job_id`, target version, and specification, not a permanent success. `message_batch_status` / `wait_message_batch` reauthorize and observe that batch, distinguishing Pending, Retrievable, Failed, CancelledOrSuperseded, and Invisible. Retrievable requires the exact target version, a valid source, original chunks indexed for all target profiles, and matching ledger generation.
- A valid new key in the same workspace can find the same receipt, but job.created_by remains. After revocation, demotion, or capability disablement, a new key cannot take over the original message publication job. Worker, retry, final publication, and post-I/O reads revalidate live permissions. Normal rotation drains old jobs before revocation. Existing cleanup is privileged reclamation. A current admin may explicitly retry it after creator revocation without transferring created_by or publishing data.
- Original source JSON keeps complete batch/session/role/timestamp and input order. Chunk locators include source_event_id, external_batch_id, message_index, source_path, UTF-8 byte_start/end within the message, and parser spec. Batches in the same session are not differentiated solely by local message_index. Raw batches do not fabricate SessionTurn or Feedback.

A derivation-capable root key is a trusted server credential with access to all owned workspaces under that root. Callers authenticate end users and select authorized target workspaces. Only trusted services may hold root keys.

## Explicit memory semantics

New databases provide the following endpoints. Existing databases do not install semantic memory tables automatically. All scope comes from authentication. Writes require Idempotency-Key.

- POST /admin/memory-properties. Admin only. Registers an immutable property, for example {"predicate":"database.engine","cardinality":"single","required_context":["env"]}. cardinality is single or multi. required_context has up to 32 valid condition names. Replaying the same definition is allowed. Changing it or taking over a property used by existing explicit identities returns 409.
- GET /v1/memory-properties/{predicate}. Reader can read a registered property in the workspace. An unregistered property returns 404.
- POST /v1/memories/semantic. Writer or above. A duplicate fact synchronously returns `{state:"duplicate",asset_id,version,source_event_id}` and saves this source without creating a version or job. No job_id is present. If publication is needed it returns `{state:"accepted",asset_id,source_event_id,job_id,semantic_status,normalization_status}`. The existing single Worker publishes, and a new publication can only be claimed after job completion. Registered properties cannot be written through /identified or /captures/identified (409). Legacy explicit identities with unregistered properties and legacy fact_key keep their semantics.

Example semantic request.

~~~json
{
  "identity": {"subject":{"kind":"service","stable_id":"Atlas"},"predicate":"database.engine","context":{"env":"production"}},
  "content": "Atlas production uses PostgreSQL.",
  "operation": "assert",
  "expected_version": 0,
  "statements": [{"value":"postgresql","quote":"Atlas production uses PostgreSQL.","byte_start":0,"byte_end":33}]
}
~~~

content is the immutable original source. quote must exactly match the UTF-8 half-open byte range. There are at most 32 statements or set members. value is 1–256 bytes with no surrounding whitespace or control characters. quote is at most 8192 bytes. Existing text limits also apply to content. Values compare as exact strings with no synonym or Unicode folding. The stable subject, predicate, and conditions in identity jointly locate the content item. Missing required conditions or an unregistered property returns 422.

| operation | Behavior |
| --- | --- |
| assert | Creates the first fact. An equal value returns duplicate without a version or job. An opposing single value publishes independent conflict evidence. For an existing multi-value set, a subset qualifies as duplicate. Use supplement to add values. |
| update | Single-value only. Explicitly updates and appends a version. |
| supplement | Multi-value only. Unions by exact value, retaining original sources and locators of existing members. |
| negate / propose / ambiguous | Publishes independent unnormalized evidence without binding or overwriting current identity. |

Multiple distinct values in a single-value request become conflict, without choosing the last. Omission of identity similarly retains ambiguous evidence, while negate/propose keep their specific statuses. expected_version must be 0. For uncertain input with identity, use it only as a suggestion. The registered property and required conditions are still checked, but expected_version does not authorize an override because no target fact is updated. The interface infers no alias and adds no review queue. The caller explicitly classifies operation and value. An exact quote does not establish truth.

Determinate facts require nonnegative expected_version (0 before publication, below i32::MAX), checked at admission and publication. A stale request returns 409. A job that loses a race after acceptance is superseded. The same key and request reuse the original response, and different content returns 409. Duplicates still save original source and audit evidence without adding the new input as extra support for an existing version.

Uncertain evidence has normalization_status=unnormalized in search/resolve/get, and locator.semantic_status is proposed/negated/ambiguous/conflict. Suggested identity appears only in locator.suggested_identity and does not bind the content item. Determinate facts have explicit_identity. Exact memory_identity retrieval excludes evidence with suggested identities.

Semantic locators use parser=memory-semantics:v1 and contain semantic_value, original source_event_id/source_version, byte_basis=source_utf8, byte_start/end, and full support_sources. SearchHit.source_event_id identifies the quote's original event. hit.version is the current content item snapshot version, and locator.source_version gives the original source version. get/lookup semantic_values lists members' sources and never treats concatenated snapshot content offsets as original offsets.

Multi-value state uses whole-snapshot validity. Withdrawing any supporting or operation source immediately excludes the entire snapshot from search/resolve/get/lookup, without historical fallback. A later explicit supplement may rebuild from valid remaining members and creates a publication job even if values duplicate. If the current snapshot's publication source was withdrawn, normal cleanup removes its chunks. Later writes must resubmit facts to keep, without automatically pulling from history. Generic restore cannot restore semantic memories. Submit a new semantic write. Original content and historical locators are not rewritten. The rule is conservative and does not provide per-member independent publication or automatic recovery.

## Semantic conversation capture

New databases can use `POST /v1/captures/semantic` or `Service::semantic_capture`. Writer or above and an extraction model are required. Capture occurs only on explicit requests, without background session monitoring. Callers supply original text preserving speakers and conditions plus a complete list of allowed identities. Source text and models cannot create stable subject IDs, property definitions, or authorizations.

```json
{
  "content": "用户：Atlas 在 production 使用 PostgreSQL。",
  "targets": [{
    "identity": {
      "subject": {"kind": "service", "stable_id": "Atlas"},
      "predicate": "database.engine",
      "context": {"env": "production"}
    },
    "expected_version": 0,
    "allow_update": false
  }]
}
```

Admin first registers the required properties. Each target's property must be registered with all required conditions. expected_version is required, with 0 for no published version. At admission, check all target versions. Any mismatch returns 409 without a source or job. Duplicate identities in targets are disallowed. allow_update defaults false and is only available for single-value properties. Even if enabled, the model must explicitly classify as update. A different value classified as assert remains conflict evidence.

The entire target list is not locked after admission. Deterministic candidates using semantic memory check the target version at routing. After the plan is saved, the Worker revalidates the actual memory entry's version after routing, sources, and visibility on retry/recovery and before final commit, including whether a duplicate is still a valid current fact. Unused targets and targets only retained as suggested_identity in unnormalized evidence are not identity-version preconditions for final commit. Negation, proposal, ambiguity, and update downgraded because allow_update was absent do not modify suggested identity. Deterministic assert checks the target version before routing to conflict, and then publishes independent conflict evidence. A lost version race for an actual determinate fact supersedes the entire job without partial publication.

content is limited to 64 KiB UTF-8 with up to 20 targets. Empty targets are valid but produce only unnormalized evidence. The model returns up to 20 candidates with 32 total statements, at most one candidate per non-null target (merge conflict statements in one candidate). Quotes are limited to 8192 bytes, values to 256 bytes, and final sets to 32 members. The model may return only target index/null, operation, and statements with exact source ranges. Unknown fields, duplicate targets, invalid ranges, and excess bounds reject the entire batch without partial publication. Invalid requests return 422, version/idempotency conflicts 409, and missing model 503. Model failures after acceptance appear in jobs. Error responses and logs omit source text and model diagnostics.

If the model classifies negation, proposal, ambiguity, or no target match, route through unnormalized memory semantics. An update lacking allow_update becomes ambiguous and retains the quote without overwriting the current value. Use supplement to append to multi. Incompatible operations such as single supplement fail the job. Duplicate values retain the submitted source and add no version. Model extraction makes judgments within caller scope. Exact quotes, identity restrictions, and version checks do not prove semantic correctness.

Admission returns `{state:"accepted",source_event_id,job_id}`. On completion, `result.memories` lists asset_id, version, target, requested_operation, state (published/duplicate), semantic_status, identity, and normalization_status in model candidate order. An empty candidate list returns an empty array. All duplicates may complete without creating a version. lookup/get/search/resolve retain their endpoints, and `memory_identity` is the search filter.

Locators use the original source and byte ranges from memory semantics. New extracted members add `extraction:{parser:"semantic-capture:v1",model:"配置的模型名称"}` with the configured model name. Old members preserve their source version and extraction metadata. They are not marked as new model or input. Source withdrawal hides the whole semantic snapshot with no automatic fallback. Published memory can be reused across callers and sessions in the same workspace. This endpoint stores neither structured SessionTurn/Feedback nor Improve/Learning distillation.

## Session archiving and idempotent feedback

SessionTurn is an immutable Q&A archive explicitly submitted after the caller finishes answering. `state:"archived"` means the archive committed and `state:"recorded"` means feedback committed. Learning requires explicitly calling [Improve](#explicit-improve--learning).

Reading and writing archive turns and feedback require writer or above. `session_id` must be bound to the authenticated logical key. Tenant, workspace, and logical key are authentication-derived. Reads, writes, and retries recheck credential validity. session_id, turn_id, feedback_id, and evidence UUIDs must be non-nil. POST requires `Idempotency-Key`. Unknown fields, missing required fields, and unknown enum values are rejected.

`POST /v1/sessions/{session_id}/turns` example.

```json
{
  "turn_id": "11111111-1111-4111-8111-111111111111",
  "question": "Atlas 使用哪个数据库？",
  "answer": "使用 SQLite。",
  "used_evidence": [{
    "kind": "chunk",
    "id": "22222222-2222-4222-8222-222222222222",
    "asset_id": "33333333-3333-4333-8333-333333333333",
    "asset_version": 2,
    "chunk_id": "22222222-2222-4222-8222-222222222222"
  }]
}
```

- question and answer must be nonblank and no longer than 65,536 UTF-8 bytes each. used_evidence is required. An explicit `[]` means no evidence used. Omission is not an empty array. Up to 64 entries are accepted. Exact duplicates are rejected. Submit only the subset actually used.
- Reference kind is chunk/entity/relation. A chunk id must equal chunk_id. A graph reference's id is the entity or relation ID; the caller selects chunk_id for the supporting chunk actually used. asset_version is the exact positive integer historical version, not an alias for current version.
- The server verifies chunk, content item version, root source, all semantic-memory snapshot support sources, and scope. Graph citations also match kind/id/root source/version owner and committed graph ledger. Current graph ownership is at source/version level. The selected chunk must belong to the same support source/version. The API does not prove per-chunk graph extraction alignment.
- Older versions with valid support can be archived after a newer update and need not match current_version. The archive stores the complete original locator, including parser, byte ranges, extraction, and semantic-memory support_sources. It does not reinterpret historic offsets.
- GET used_evidence entries are `{evidence_ref_id,reference,source_event_id,source_version,version_source_event_id,locator}`. reference is the caller input above, and evidence_ref_id is allocated and persisted on first archive. source_event_id/source_version refer to a semantic member's original source. version_source_event_id is the snapshot's root source, and reference.asset_version is the content item version used. These identifiers are distinct.

Successful archive HTTP 200 returns `{session_id,turn_id,source_event_id,state:"archived"}`. The unique domain key is tenant/workspace/session_id/turn_id. Under the same authorized logical key, identical text and ordered `used_evidence` return original IDs. Retry can use a different `Idempotency-Key` or authorized rotated credential. Changed content returns 409 and preserves the original. SQLite commits question/answer source, archive, references, audit, and command receipt atomically. An invalid reference rolls back.

Feedback request examples for the original turn or an exact archived reference returned by GET.

```json
{"feedback_id":"44444444-4444-4444-8444-444444444444","target":{"type":"turn"},"rating":"helpful","comment":"回答解决了当前问题"}
```

```json
{"feedback_id":"55555555-5555-4555-8555-555555555555","target":{"type":"evidence","evidence_ref_id":"66666666-6666-4666-8666-666666666666"},"rating":"unhelpful"}
```

rating accepts only helpful/unhelpful/neutral as the caller's judgment, without inferring truth. comment is optional or null and capped at 4096 UTF-8 bytes. Targets must belong to the archived turn in the path. Feedback may not refer to unused hits, other turns, or other scopes. At most 256 immutable entries per turn. At capacity, new entries return 409 while exact replays can still succeed without truncation. The unique domain key is tenant/workspace/principal/feedback_id. The same semantic request reuses results; changed turn, target, rating, or comment returns 409. Feedback does not overwrite, edit, or delete prior ratings or directly change ranking or weights. POST returns `{session_id,turn_id,feedback_id,principal_id,feedback_sequence,state:"recorded"}`. GET returns `{session_id,turn_id,feedback:[...]}`. Feedback retains the original request and created_by/created_at. Turn GET also includes feedback.

Governance invalidates a whole turn conservatively. Admin `DELETE /v1/events/{source_event_id}` can withdraw Q&A. Deleting a content item actually cited, withdrawing its root source or any semantic-memory support source, or deleting its file immediately blocks the entire turn and feedback reads and writes (404). Original archive source GET also checks dependencies. This applies before cleanup. Historical archives do not depend on removable chunk foreign keys. Reads and replay revalidate sources. The cache stores IDs and statuses only; the same key, a different key, or restart cannot revive invalid archives. Learning only uses archives that pass the same validity checks. The explicit Improve → Learning endpoint follows. Logical withdrawal retains durable records and does not physically erase them.

## Explicit Improve → Learning

New databases only. Existing extraction configuration provides proposals and independent verification without an additional provider, trigger, rank reweighting, or identity override. Call explicitly.

- `POST /v1/sessions/{session_id}/improve` (writer+, Idempotency-Key). `{improve_id,conditions:["仅在 staging 发布时"],max_feedback:32}`. improve_id must be non-nil and body immutable. conditions is required with 1–8 distinct nonblank entries, each at most 256 UTF-8 bytes. max_feedback defaults to 32, range 1–32.
- `GET /v1/improvements/{improve_id}` (writer+). Returns status, parent job, frozen watermark, counts, persisted proposal/verification state, Learning child jobs, and safe errors. Frozen Q&A and raw model responses are omitted. These administrative fields remain readable after dependency withdrawal.
- `GET /v1/learnings/{learning_id}` (original logical key only). Returns still-valid published Learning with public statement, conditions, and private support pointers without original Q&A quotes. Other workspace members reuse public content and conditions through Search/Bundle.

Feedback uses a monotonic workspace-transaction feedback_sequence. Identity remains tenant/workspace/principal/feedback_id. Equal or backward timestamps do not affect batching. A freeze contains up to 32 feedback entries and 128 KiB JSON. An oversized first entry fails without consuming the watermark. Each fully serialized proposal or verification model request is also capped at 128 KiB. A verification request that exceeds it after adding candidates fails before sending and without consuming the watermark. Cancel and retry with a smaller batch. Turns invalid at freeze keep only skip IDs, without hidden text. At most one accepted/publishing run per session. Repeating a command or improve_id reuses its domain result and changed requests return 409.

The Worker proposes, persists, independently verifies, persists, and then registers knowledge items, Learning support, and regular Ingest child jobs in one transaction. Up to 8 candidates. statement/why are limited to 8192/4096 UTF-8 bytes respectively, with 1–32 exact supports per candidate and each quote capped at 4096 UTF-8 bytes. Supports bind the frozen turn_id, principal_id, feedback_id, question/answer field, and UTF-8 quote range. Model conditions must exactly match the caller's entries. Verification reason accepts limited safe codes. helpful does not prove a fact, and unhelpful does not permit negating or overwriting memory.

The completion watermark advances atomically only after all accepted children publish. Empty candidates, rejection, and exact duplicates have explicit outcomes. Exact novelty compares only statement and conditions without semantic deduplication. Cross-session concurrency or persisted verification replay checks active accepted/published reservations in the registration transaction. A conflict rolls back all registration, supersedes the parent job, and leaves the watermark unchanged. Explicitly cancel the unfinished run and restart with a new improve_id. Generic retry cannot bypass reservations. Each Learning publishes atomically, without simultaneous visibility across Learnings. Failed children keep `asset_id`, `source_event_id`, and `job_id` and use existing `/v1/jobs/{id}/retry` with saved stage results. A crash between external model response and local commit can repeat the call. Exactly-once model network calls are not guaranteed.

An Improve run may remain publishing after its parent model job completes. Cancelling the parent also cancels unfinished children and releases the run without advancing the watermark. Already published Learning remains valid and visible. Cancelled runs and child jobs cannot be retried. Submit a new improve_id. Exact published duplicates do not create new content items. Completed runs cannot be cancelled. Permanent source invalidation can use cancel and a new request. A transient failure does not silently consume feedback.

Learning is independent knowledge without a memory identity. Each chunk's locator.learning retains conditions and support IDs. resolve renders the conditions with the chunk and counts them toward budget. Learning generates keyword/vector only, without summaries or graph projections that could lose conditions. Other knowledge settings remain. Generic knowledge update and restore cannot rewrite Learning.

All support turns must be wholly valid, including actual citations not selected by a candidate, exact historical versions, root sources, and all original semantic-memory member sources. Recheck on proposal/plan replay, indexing, final commit, search, get, resolve, graph, and original-source reads. Candidate budgets apply after invalidation filtering. If a Learning is supported by turns referring to another Learning, transitive validation is capped at 64 distinct Learning/turn nodes. Cycles or over-limit chains fail without truncation. Withdrawal hides immediately before asynchronous cleanup. New versions and restore cannot resurrect invalid historical support.

Deterministic fixtures establish engineering flow only. Real verification accuracy, fact quality, and Agent gains require independent acceptance. See EvidenceBundle below for unified context. Real Agent comparisons remain separate.

## EvidenceBundle for unified context

`POST /v1/context` is read-only and takes no idempotency key. It corresponds to MCP `context_bundle` and shares `EvidenceInput` / `EvidenceBundle` with `Service::evidence_bundle`. Existing search and resolve contracts remain. Minimum input is `{"query":"Atlas"}`.

Optional fields include `mode` keyword (default)/vector/hybrid, `allow_partial` default false, and `budget` default 4000, range 0–32000, in **actual UTF-8 bytes**. `routes` is a list of unique route configurations. Each `route` is memory/knowledge/learning/session, with `limit` default 20, range 1–100, and `max_bytes` default 4000, range 0–32000. The first three routes default on and session defaults off. Unknown fields, repeated routes, and invalid budgets are input errors.

`memory_identity` only restricts memory by exact subject, predicate, and conditions. Knowledge and Learning scopes are unaffected. `applicability_context` is up to 32 nonblank caller condition strings, each at most 256 bytes. Every Learning condition must match verbatim to mark `caller_context_exact_match`. Otherwise retain and mark `unverified`. Neither establishes actual applicability.

Session requires writer or above or returns 403, checked before model calls. `session_id` / `exclude_session_id` can restrict sessions only when the session route is enabled. The lane scans up to 512 recent relational records. Full source and transitive evidence validity precede retention limits. It returns `scanned` / `scan_exhausted`, which warns that earlier records may be unscanned. It is not whole-history semantic search and grants no original Q&A access to readers.

Returns `rendered_context`, `items`, `conflicts`, `warnings`, scope, policy, requested/effective mode, total budget and actual bytes, and per-route candidate, kept, and omitted counts. Each item retains type, retrieval lane, conditions/applicability, semantic identity, score kind, and stable citation. `provenance.type=asset_chunk` distinguishes asset_version, original source_version, and version_root_source_event_id. `session_turn` retains real session_id/turn_id/archive source and historic used_evidence without fabricating chunks. Full visibility of sources and referenced material is checked before budgeting and in the final render snapshot. Withdrawal does not wait for physical cleanup. Validity only indicates that the read snapshot visibility check passed.

Deterministic conflict grouping applies only to confirmed/conflict values under the same conditions and explicit single-value identity. All candidates and warnings within a group are retained or omitted together. Prose, different conditions, and multi-value sets are not inferred to be mutually exclusive. Conflict checks cover retrieved candidates only and do not establish absence of cross-document conflict. Deduplicate only exact evidence locations, merging keyword/vector paths without merging similar text.

All titles, citations, conditions, sources, conflict warnings, and separators count toward the actual UTF-8 context budget. Keep whole chunks without splitting Chinese or emoji and leave no unattached citation. The memory, knowledge, and Learning routes share one embedding and have separate candidate filters and budgets. This version uses raw chunks only, without summary/graph lanes. Stable round robin and route caps are neutral engineering defaults, not verified optimal quality weights.

**Archive limitation**. Memory, knowledge, and Learning items can convert to legacy chunk used_evidence. session_turn citations cannot be stored in `SessionTurn.used_evidence` (which supports only chunk/entity/relation). The response and session text mark `unsupported_session_turn_reference`. Callers must not fabricate a chunk or claim complete archive of these references. Learning citations for reader include support IDs and ranges without original supporting-turn Q&A or quotes.

See the [EvidenceBundle contract](contracts/evidence-bundle.md). Run real Agent comparisons independently. Fixtures test the tool/model interaction framework and provide no evidence of task gains.

<a id="recorded-run-references-for-a-pinned-asset-version"></a>
### Recorded Run references for a pinned content item version

`GET /v1/assets/{asset_id}/runs?version=1&limit=25&after={run_uuid}` requires a positive `version`. `limit` is 1–100, default 25. `after` is optional with stable Run UUID pagination. Only the current logical key is visible. Administrators have no privilege over other keys' Runs. Returns `asset_id`, `asset_version`, `items` (only `run_id` and `created_at_ms`), `next_after`, `scope: current_logical_key`, and `coverage: recorded_references`. Empty pages can have a next cursor. Unreadable sources or content items deny access. Indexing locates the first archived reference and revalidates its authoritative event. Unreadable associations are omitted conservatively, and complete capture or readability of other Run references is not guaranteed. [Run contract](contracts/runs.md#recorded-run-associations-for-an-exact-content-version)
