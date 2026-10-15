# Issue #60 storage field/table audit

Baseline: main `4c0862e8`, tree `7e73c2`, reviewed before implementation. This is a fresh-schema design review, not permission to transform an existing database. Read AGENTS.md, docs/STATUS.md, the Turso/Lance integration contract and S2 contract; historical upgrade language does not override current fresh-database scope.

## Decision summary

- **Merge candidate: oc_memory_identities into oc_assets.** Creation is transactionally 1:1, asset identity is already unique, and canonical identity_key is written to assets.fact_key. Add nullable identity_key and identity_json on assets, preserve the separate identity binding key, legacy-vs-structured discriminator and canonical key collision behavior, use assets.id for asset_id, and keep the existing scoped memory fact-slot unique index plus scoped structured identity uniqueness. The normal writer puts the same key into both slots, but that is not proof that their independently persisted meanings and corruption behavior may be collapsed. The identity table's created_at is written at the same creation operation but is not an independent read contract; coalescing creation provenance onto assets.created_at is justified for fresh schemas. Do not silently reinterpret any existing database.
- **Retain all other tables** in this bounded first change. Distinct multiplicity, lifecycle, authorization, replay or preservation obligations prevent a mechanically safe merge. Detailed choices follow, including a complete column inventory.
- No independent field deletion is approved merely because it is absent from SELECT statements. Creation metadata and model provenance can be write-only audit facts. Explicitly questionable fields are separated below from demonstrated redundancy.
- Keep source events versus asset versions, commands versus jobs, artifact owners versus publication ledger, the user-defined memory property catalog, and publication.receipt_metadata. These are not interchangeable duplicates.

## Common field meanings (applies to every listed occurrence)

| Fields | Decision and meaning |
|---|---|
| tenant_id + workspace_id (native Lance: tenant + workspace) | Retain. Authorization and isolation scope; repeated keys are deliberate, not redundant decoration. Graph uses text UUIDs while authoritative tables use BLOBs. |
| id / asset_id / source_event_id / source_id / job_id / improve_id / learning_id / principal_id / turn_id / session_id | Retain the explicitly described identity at its own cardinality and lifecycle. Do not collapse merely because one current workflow creates the objects together. |
| created_by / actor / principal_id | Retain provenance and/or exact principal identity; jobs.created_by specifically drives fresh creator authorization on replay. |
| created_at / updated_at / completed_at / published_at | Retain creation and lifecycle timestamps unless an explicit exception is stated. updated_at on pending ledger rows drives timeout reconciliation, not just diagnostics. |
| request_hash / content_hash / fingerprint / hash / token_hash | Retain distinct hashes: idempotency request, version content, candidate dedup, original blob content address, and authentication respectively. They are not interchangeable. |
| version / current_version / restored_from / generation / run_token | Retain: immutable historical version, current publication pointer, restoration ancestry, explicit retry generation, and execution fencing token respectively. |

## Table-specific decision matrix and concrete paths

Storage source paths in this matrix are relative to src/storage/ (for example sqlite/identity.rs means src/storage/sqlite/identity.rs); documentation paths are repository-relative. Line anchors refer to the baseline. “W” denotes producer/mutator; “R” denotes consumer. An absent ordinary reader is stated explicitly, not inferred as deletion permission.

| Table | Decision, field groups and lifecycle | Concrete W / R paths |
|---|---|---|
| oc_storage_format | Retain id,engine,role,schema_version. Singleton id=1 with exact engine/role/version admission. Exists in each native authority/graph database role; role separation is intentional. | W native_turso.rs:764–775 install_format; R :778–805 check_format; exact DDL audit :937 onward. |
| oc_workspaces | Retain tenant_id,id identity; name label; owner_workspace_id immutable root association; created_at provenance. Root ownership is one level and cannot be changed. | W sqlite/relational.rs:35–49 create_workspace, :154–194 owned_workspace; R sqlite/domain.rs:13–54 live_role_for_principal, sqlite/app.rs:32–45 workspace_scope/scopes. |
| oc_api_keys | Retain id,tenant_id,workspace_id,token_hash,role,allow_owned_workspaces,revoked,created_at. Default-deny capability and fresh revocation checks must survive. No plaintext credential. | W sqlite/relational.rs:55–128 issue_key_with_capabilities/revoke_key; sqlite/app.rs:352–398 scoped issuance/revocation. R relational.rs:16 authenticate; domain.rs:13–54 authorization; :118–141 job creator check. |
| oc_files | Retain id/scope; name/media_type/hash/size original metadata; deleted independent file withdrawal; created_by/created_at provenance. Blob identity cannot merge with events because file-less events and file-linked sources are valid. | W sqlite/provenance.rs:30–52 insert_file_inner, :98–119 delete_file_inner; R :57–80 file_exists/hash, :160–257 event_view/event_file. name,size,created_by,created_at are stored provenance, not current ordinary query projection. |
| oc_events | Retain id/scope,content,kind,file_id,state,created_by,created_at. Immutable original source may support several versions/artifacts; active/retracted is source governance. Session/learning bodies also use events. | W provenance.rs:6–26 create_event_inner,:85–94 retract_event_inner; R :160–257 event_view/content/file, sqlite/visibility*, session.rs:476–544, learning_visibility.rs:216–320. |
| oc_assets | Retain id/scope,kind,title,fact_key,current_version,deleted,created_at; add merged identity_key and identity_json candidates. title is initial/current display metadata, whereas versions.title preserves historical title. fact_key is a slot key with a partial unique memory index, not free text identity content. deleted does not mean source deletion. | W identity.rs:27–66 slot_inner,:96–180 identity_slot_inner; versions.rs:6–24 insert_knowledge_asset_inner,:69–79 delete_asset_inner,:313–330 update_asset_version_inner; semantics.rs:104–108 unnormalized slot. R versions.rs:85–161 asset_view, :202 current version, :227 fallback title; app.rs:165 asset_metadata. |
| oc_memory_identities | Merge into assets: tenant_id/workspace_id -> existing scope; asset_id -> assets.id; identity_key -> new nullable assets.identity_key (retain independently of fact_key); identity_json -> new nullable assets.identity_json; created_at -> shared assets.created_at creation fact. Table removed only once every identity reader and checker is converted. Identity JSON NULL must distinguish legacy slots; do not derive full identity from key alone. | W identity.rs:156–179 writes identical key to fact_key and identity_key; R identity.rs:6–21 identity_for_asset,:70–94 exact lookup,:96–147 binding/collision checks,:186–211 batched identity rows; app.rs:137 candidate lookup; semantics.rs:64 catalog takeover rejection. Also read_model/context filtering consumers use batched identity rows. |
| oc_memory_properties | Retain predicate,definition,created_by,created_at plus scope. User-admin immutable catalog contains cardinality and required context independently of any memory instance. Registration can precede all assets; registered definitions cannot be changed or hijack a previously allocated identity even if unpublished/deleted. | W semantics.rs:42–82 register_memory_property; R :24–40 memory_property,:64 occupied-predicate guard,:85–97 require_legacy_identity. Design docs/superpowers/plans/2026-10-11-s2-memory-semantics.md property catalog. |
| oc_versions | Retain scope,asset_id,version; content/content_hash; source_event_id; restored_from; title; created_by/created_at. Append-only historical source/version content, unlike current pointer on assets; source may be withdrawn without changing original bytes. | W versions.rs:213–255 insert_version_inner; R :85–161 asset_view,:29–65 restore_source_inner; visibility/read_model historical and current routes. |
| oc_chunks | Retain scope,id,asset_id,version,ordinal,content,locator,search_terms. Multiple chunks per version; ordinal uniqueness defines order. locator carries immutable parser, page-local UTF-8 byte offsets, original support, semantic status. search_terms is persisted deterministic preprocessing used independently of source bytes. | W versions.rs:259–284 insert_chunk_inner; R versions.rs:171–188 chunks_for_version; read_model.rs:246 and :1596 candidate metadata; publication.rs:393 exact receipt validation; maintenance.rs remove_unowned_chunks. |
| oc_summaries | Retain scope,id,chunk_id,text,model_revision,search_terms. Multiple summaries per chunk allowed: no UNIQUE(chunk_id). Cross-row AND aggregation is intentional, so cannot flatten to one chunk field without semantic loss. text/model_revision preserve generated output and model provenance even though current summary retrieval consumes search_terms. | W versions.rs:287–311 insert_summary_inner; R read_model.rs:704–734 current_summary_terms group_concat; maintenance.rs removes summaries attached to removed chunks. Contract docs/superpowers/plans/2026-10-13-turso-lance-integration.md Frozen scope. |
| oc_jobs | Retain all: id/scope/created_by; operation,payload; state; run_token,generation,attempt,next_retry_at; asset_id,source_event_id; cancel_requested; outcome,result,error_code; created_at,updated_at. Async execution and retry state cannot replace synchronous command receipt. payload contains accepted immutable input plus persisted stage plans; creator remains original across cross-key receipt replay. | W job_queue.rs:18–80 claim/requeue,:120 settle,:147 enqueue,:232 cancellation,:277 retry action,:360 completion; app.rs:59 recovery,:71 fail_job; R job_queue.rs:250 job_row,:341 active_job; domain.rs:124 creator permission; publication.rs:119–240 live status. |
| oc_commands | Retain scope,principal_id,dedup_scope,operation,key,request_hash,response,created_at. Operation-scoped idempotent accepted response; workspace dedup for message batch, principal dedup for other commands. Works for synchronous actions with no job. principal_id stores original issuer while dedup_scope explicitly permits cross-principal replay. | W domain.rs:162–184 finish_command; R :73–116 begin_command. Created receipt is not overwritten by job retry/completion. |
| oc_audit | Retain scope,id,actor,action,target,details,created_at. Independent append-only audit; absence of a public audit reader is not evidence it can be dropped or synthesized from jobs. | W domain.rs:143–159 audit; called from semantic registration/session/improve and service-domain changes. No current ordinary SELECT projection found. |
| oc_artifact_owners | Retain scope,source_id,version,artifact_type,artifact_id,chunk_id,created_at. Durable support relation, potentially many owners per artifact, intentionally independent of surface/generation. chunk_id is nullable for graph objects, used for cleanup and support association. | W provenance.rs:261–286 register_owner_inner; R session.rs:46–54 archive owner validation; app/maintenance.rs:97–195 detach invalid roots/chunks and unowned artifacts; visibility graph owner paths. D app/maintenance.rs:27, learning_visibility.rs:530 detaches unsupported ownership. |
| oc_index_entries | Retain scope,artifact_id,field,model_id,dimension,generation,state,created_at. Vector profile/dimension eligibility is not the publication ledger's source/surface progress. field currently content on the main writer but remains a keyed API-level index identity; pending/ready/removed differ from ledger states. | W app.rs:183 index_ready; R :191 vector_generations,:201 vector_candidates; publication.rs:506–539 exact index checks; D app/maintenance.rs REMOVE_INDEXES changes state to removed. |
| oc_artifact_ledger | Retain table and scope,source_id,version,artifact_type,artifact_id,surface,generation,state,updated_at. Retain remaining metadata pending explicit separate decision: idempotency_key,attempt,last_error,next_retry_at,created_at (see candidate analysis below). Per-surface/per-generation pending/committed/retry_wait/orphan progress is not ownership. | W ledger.rs:30–57 register_pending,:60–95 confirm_committed; :5–26 reconcile timeout/retry; app.rs:417 resume_pending; maintenance.rs ORPHAN_LEDGER. R publication.rs:544 onward exact source/surface/generation checks; full_text.rs:46–98 eligibility; graph/session visibility. |
| oc_session_turns | Retain scope,session_id,turn_id,source_event_id,request_hash,created_by,created_at. Exact immutable domain archive identity distinct from event and command IDs; independent uniqueness for source_event_id; archive exists without an async job. | W session.rs:548–604 archive_turn; R :476 require_turn_visible_local,:534 require_session_source_visible,:614 session_turn; session/context_reads.rs batched turn dates/body routing. |
| oc_session_turn_evidence | Retain scope,session_id,turn_id,evidence_ref_id,ordinal,evidence. Zero/many exact archived evidence records per turn; stable feedback target and ordering separate from source chunk identities. evidence JSON freezes original versions, locators, graph support and S2 member sources. | W session.rs:595 archive_turn; R :446 turn_evidence,:662 evidence feedback target; improve.rs:351 frozen_turn; session/context_reads.rs:7 bounded ordinal pages. |
| oc_feedback | Retain scope,principal_id,feedback_id,session_id,turn_id,request_hash,feedback,created_at,feedback_sequence. Principal-specific domain idempotency; workspace-monotonic sequence supports exact Improve snapshots. Feedback JSON contains score/target, not replacement archive evidence. | W session.rs:646–703 record_feedback; R :625 feedback_rows; improve.rs:421 snapshot input,:542 require_improve_snapshot_visible. |
| oc_improve_cursors | Retain scope,session_id,feedback_sequence. Durable per-session high-watermark advances only after successful complete run; cannot replace by max accepted run because cancelled/incomplete runs must not consume feedback. | W improve.rs:415 initial row,:744 CAS advance; R :418 accept_improve lower cursor. |
| oc_improve_runs | Retain scope,improve_id,session_id,job_id,request_hash,request,snapshot,lower_sequence,target_sequence,proposal_plan,verification_plan,status,created_by,created_at,completed_at. Parent coordinator persists immutable feedback snapshot and separately frozen model stages; one unfinished run per session. Not 1:1 with output learnings. | W improve.rs:481 accept,:577 save proposal,:606 save verification,:703 publishing,:750 complete,:974 cancel; R :385 replay,:493 lookup,:731–750 completion checks; learning_visibility.rs:51/614 route gate. |
| oc_learnings | Retain scope,id,improve_id,session_id,candidate_index,asset_id,source_event_id,ingest_job_id,candidate,fingerprint,status,created_by,created_at,published_at. At most eight distinct candidates per run; maps independent source/asset/ingest identities; publication status controls visibility and fingerprint is conservative dedup input. | W improve.rs:693 create_learning,:777 mark_learning_published; R :655 replay,:803 improve_view,:842 learning_view,:879/:907 duplicate lookup,:933 publication metadata; learning_visibility.rs:39 onward. |
| oc_learning_support | Retain scope,learning_id,session_id,turn_id,support_ordinal,principal_id,feedback_id. Many supporting feedback records per learning, bounded/order-preserved; exact principal+feedback FK and turn FK are both meaningful. | W improve.rs:698 create_learning; R learning_visibility.rs:440 guard_learning_dependencies; SQL FK constraints protect references on insertion. |
| oc_graph_entities | Retain uid,tenant_id,workspace_id,id,name,entity_type,description,source_id,version. uid is backend-scoped graph identity used by endpoint FKs, id is domain artifact identity. Source attribution supports scoped delete; text/type/description are graph object facts even where current snapshot only returns id/name. | W sqlite_graph.rs:329 upsert_entities; R :72 endpoint_rows,:294 snapshot,:470 list_ids; D :443 delete_source,:500 delete_objects. |
| oc_graph_relations | Retain id,tenant_id,workspace_id,head_uid,tail_uid,predicate,source_id,version. Directed edge identity keyed by both endpoints and id, multiplicity must survive. Endpoint FKs cascade while source-specific deletion remains separate. | W sqlite_graph.rs:392 upsert_relations; R :101 snapshot_relations,:156 frontier_edges,:478 list_ids; D :453 delete_source,:511 delete_objects. |
| Lance vec_<profile hash> | Retain tenant,workspace,id,source_id,version,generation,vector (and nested item floats). Separate physical table per fixed-dimension profile; merge_insert identity uses all six nonvector fields. Native rows are derived and never grant authorization; eligibility rechecks authority. | Schema lancedb.rs:84–100; same module VectorStore write/search/delete, lancedb/ann.rs and eligibility.rs. |
| Lance chunk_terms_v1 | Retain tenant,workspace,id,source_id,version,generation,search_terms. Separate full-text index and lifecycle surface, exact preprocessing marker/index configuration enforced. Same six-part identity avoids replacing other generations. | Schema lancedb/full_text.rs:28–48; same module ensure/check index, full_text write/query/delete. Authority readers sqlite/full_text.rs and startup verification. |

## Demonstrated merge requirements

1. Fresh assets.identity_key and assets.identity_json should be nullable together for knowledge, legacy memory and unnormalized evidence. Retain identity_key separately from fact_key: the former selects the structured binding, the latter maintains legacy fact-slot collision semantics. Structured identity belongs only to kind=memory. Extra constraints that reject corrupt pairs earlier are a deliberate fail-closed behavior change and need the agreed acceptance decision, rather than being described as automatically equivalent.
2. Preserve separate scoped identity_key uniqueness and scoped memory fact_key uniqueness. Scope plus fact_key uniqueness must continue rejecting both legacy-to-structured and structured-to-legacy collisions. Existing code explicitly rejects a legacy slot whose canonical key collides with a structured identity; don't make the new query skip that row and accidentally attempt a duplicate insertion.
3. Identity lookup must continue comparing decoded identity to the request, distinguishing corrupt/mismatched binding from absent identity. Deleted/unpublished identities still block predicate catalog registration.
4. Convert all identity consumers together: exact lookup, legacy slot guards, batched identity rows, candidate lookup, semantic property registration occupancy checks, current/context/search metadata, schema admission and tests. Keep bounded read batches and authorization/visibility before decoding where currently required.
5. Removing a table changes exact expected schema. Old layouts must fail admission without DDL repair, deletion or implicit migration. Same-new-version reopen and backup/restore remain mandatory.

## Field candidates that need an explicit narrower decision

- ledger.idempotency_key: deterministic worker helper formats LedgerKey; storage only inserts this value, while native upserts use the actual six-part/scoped key. No production storage reader found. This is a plausible removable redundant representation, but LedgerEntry accepts a caller-supplied string through the public library, so do not claim the schema enforces derivability. A separate change should remove the field and typed API only with non-AML public-caller coverage and updated documented key semantics.
- ledger.attempt and ledger.last_error: no production assignment or projection found in the reviewed storage path (defaults and preservation tests exist). ledger.next_retry_at is actually read by reconciliation, but no normal producer was found setting it; retry_wait enum/reconciliation remain public lifecycle contracts. Treat these as an explicit obsolete-retry-model review, not as proof owners and ledger may merge. Keep in the first bounded identity merge unless all public transition semantics are intentionally narrowed together.
- summary.model_revision, graph entity_type/description and broad created_by/created_at fields: currently some are insert-only. They retain generated-object/model or creation provenance. Removing them would discard stored information rather than eliminate an identical representation; not justified by this audit alone.
- index_entries.field: current index_ready hard-codes content, yet field is part of the declared key and exact receipt filters. Removing it changes accepted index identity, so requires explicit single-field contract and public-library tests.
- assets.title versus versions.title is deliberate current/initial versus historical title, with fallback insertion and restoration reads. events.content versus versions.content versus chunks.content is original source versus immutable version representation versus segment; equality in one ingest example does not establish redundancy.

## JSON fields that must not be erased under “schema cleanup”

jobs.payload.publication is the persisted expensive plan, including embedding arrays reused on retry. jobs.payload.publication.receipt_metadata is an intentionally duplicated small projection (format plus exact chunk ID JSON tokens and dimensions), derived and stored atomically with the plan in sqlite/publication.rs:304–375. message_batch_status reads it without transferring the embeddings (:119–159), validates count/shape/exact IDs and checks live authority, index and both vector/full_text ledgers (:207–240,:377–587). It is not a success cache, and dropping it reintroduces heavyweight retry/status reads. Keep semantic_capture_plan (semantics.rs:14–22), accepted_spec and operation-specific input, and Improve proposal/verification stages for deterministic resume. Historical locator/provenance JSON and archived evidence are not redundant with mutable current rows.

## Validation and limits

This is a source audit, not a passing implementation/test claim. Actual acceptance must run fresh initialization/incompatible-schema rejection, same-version reopen/recovery, identity collisions and deleted/unpublished catalog occupancy, semantic source withdrawal, session evidence/feedback and learning visibility, same-generation saved plan reuse, bounded lightweight receipt reads, public-library and HTTP paths. Run no-JOIN static checker and Python suite, rustfmt, warning-denied Clippy and relevant Linux/Windows tests. This audit records the pre-merge schema; implementation and measured results are tracked separately.

## Complete authoritative DDL column/constraint inventory

This mechanically extracted appendix covers every SQL table declaration in the six active schema files plus the format marker above. The table-specific matrix governs each listed field, with the common groups covering repeated scope/identity/provenance semantics. Constraints are included to make the merge reviewable rather than relying on conceptual table names.

### oc_improve_cursors

Source: `src/storage/learning-schema.sql:1`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| session_id | BLOB NOT NULL |
| feedback_sequence | INTEGER NOT NULL DEFAULT 0 CHECK (feedback_sequence >= 0) |

Constraints: PRIMARY KEY (tenant_id, workspace_id, session_id); FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id).

### oc_improve_runs

Source: `src/storage/learning-schema.sql:7`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| improve_id | BLOB NOT NULL |
| session_id | BLOB NOT NULL |
| job_id | BLOB NOT NULL |
| request_hash | TEXT NOT NULL |
| request | TEXT NOT NULL |
| snapshot | TEXT NOT NULL |
| lower_sequence | INTEGER NOT NULL CHECK (lower_sequence >= 0) |
| target_sequence | INTEGER NOT NULL CHECK (target_sequence >= lower_sequence) |
| proposal_plan | TEXT |
| verification_plan | TEXT |
| status | TEXT NOT NULL DEFAULT 'accepted' CHECK (status IN ('accepted','publishing','completed','cancelled')) |
| created_by | BLOB NOT NULL |
| created_at | INTEGER NOT NULL |
| completed_at | INTEGER |

Constraints: PRIMARY KEY (tenant_id, workspace_id, improve_id); UNIQUE (tenant_id, workspace_id, job_id); FOREIGN KEY (tenant_id, workspace_id, session_id) REFERENCES oc_improve_cursors(tenant_id, workspace_id, session_id); FOREIGN KEY (tenant_id, workspace_id, job_id) REFERENCES oc_jobs(tenant_id, workspace_id, id) DEFERRABLE INITIALLY DEFERRED.

### oc_learnings

Source: `src/storage/learning-schema.sql:22`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| id | BLOB NOT NULL |
| improve_id | BLOB NOT NULL |
| session_id | BLOB NOT NULL |
| candidate_index | INTEGER NOT NULL CHECK (candidate_index >= 0 AND candidate_index < 8) |
| asset_id | BLOB NOT NULL |
| source_event_id | BLOB NOT NULL |
| ingest_job_id | BLOB NOT NULL |
| candidate | TEXT NOT NULL |
| fingerprint | TEXT NOT NULL |
| status | TEXT NOT NULL DEFAULT 'accepted' CHECK (status IN ('accepted','published')) |
| created_by | BLOB NOT NULL |
| created_at | INTEGER NOT NULL |
| published_at | INTEGER |

Constraints: PRIMARY KEY (tenant_id, workspace_id, id); UNIQUE (tenant_id, workspace_id, improve_id, candidate_index); UNIQUE (tenant_id, workspace_id, asset_id); UNIQUE (tenant_id, workspace_id, source_event_id); UNIQUE (tenant_id, workspace_id, ingest_job_id); FOREIGN KEY (tenant_id, workspace_id, improve_id) REFERENCES oc_improve_runs(tenant_id, workspace_id, improve_id); FOREIGN KEY (tenant_id, workspace_id, asset_id) REFERENCES oc_assets(tenant_id, workspace_id, id); FOREIGN KEY (tenant_id, workspace_id, source_event_id) REFERENCES oc_events(tenant_id, workspace_id, id); FOREIGN KEY (tenant_id, workspace_id, ingest_job_id) REFERENCES oc_jobs(tenant_id, workspace_id, id).

### oc_learning_support

Source: `src/storage/learning-schema.sql:38`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| learning_id | BLOB NOT NULL |
| session_id | BLOB NOT NULL |
| turn_id | BLOB NOT NULL |
| support_ordinal | INTEGER NOT NULL CHECK (support_ordinal >= 0 AND support_ordinal < 32) |
| principal_id | BLOB NOT NULL |
| feedback_id | BLOB NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, learning_id, support_ordinal); FOREIGN KEY (tenant_id, workspace_id, learning_id) REFERENCES oc_learnings(tenant_id, workspace_id, id); FOREIGN KEY (tenant_id, workspace_id, session_id, turn_id) REFERENCES oc_session_turns(tenant_id, workspace_id, session_id, turn_id); FOREIGN KEY (tenant_id, workspace_id, principal_id, feedback_id) REFERENCES oc_feedback(tenant_id, workspace_id, principal_id, feedback_id).

### oc_memory_identities

Source: `src/storage/memory-identity-schema.sql:1`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| identity_key | TEXT NOT NULL |
| asset_id | BLOB NOT NULL |
| identity_json | TEXT NOT NULL |
| created_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, identity_key); UNIQUE (tenant_id, workspace_id, asset_id); FOREIGN KEY (tenant_id, workspace_id, asset_id) REFERENCES oc_assets(tenant_id, workspace_id, id).

### oc_memory_properties

Source: `src/storage/memory-semantics-schema.sql:1`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| predicate | TEXT NOT NULL |
| definition | TEXT NOT NULL |
| created_by | BLOB NOT NULL |
| created_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, predicate); FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id).

### oc_session_turns

Source: `src/storage/session-schema.sql:1`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| session_id | BLOB NOT NULL |
| turn_id | BLOB NOT NULL |
| source_event_id | BLOB NOT NULL |
| request_hash | TEXT NOT NULL |
| created_by | BLOB NOT NULL |
| created_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, session_id, turn_id); UNIQUE (tenant_id, workspace_id, source_event_id).

### oc_session_turn_evidence

Source: `src/storage/session-schema.sql:9`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| session_id | BLOB NOT NULL |
| turn_id | BLOB NOT NULL |
| evidence_ref_id | BLOB NOT NULL |
| ordinal | INTEGER NOT NULL CHECK (ordinal >= 0) |
| evidence | TEXT NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, session_id, turn_id, evidence_ref_id); UNIQUE (tenant_id, workspace_id, session_id, turn_id, ordinal).

### oc_feedback

Source: `src/storage/session-schema.sql:17`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| principal_id | BLOB NOT NULL |
| feedback_id | BLOB NOT NULL |
| session_id | BLOB NOT NULL |
| turn_id | BLOB NOT NULL |
| request_hash | TEXT NOT NULL |
| feedback | TEXT NOT NULL |
| created_at | INTEGER NOT NULL |
| feedback_sequence | INTEGER NOT NULL CHECK (feedback_sequence > 0) |

Constraints: PRIMARY KEY (tenant_id, workspace_id, principal_id, feedback_id); UNIQUE (tenant_id, workspace_id, feedback_sequence).

### oc_graph_entities

Source: `src/storage/sqlite-graph-schema.sql:3`.

| Column | Type/default/nullability/check |
|---|---|
| uid | TEXT PRIMARY KEY |
| tenant_id | TEXT NOT NULL |
| workspace_id | TEXT NOT NULL |
| id | TEXT NOT NULL |
| name | TEXT NOT NULL |
| entity_type | TEXT NOT NULL |
| description | TEXT NOT NULL |
| source_id | TEXT NOT NULL |
| version | INTEGER NOT NULL |

Constraints: Inline column constraints only.

### oc_graph_relations

Source: `src/storage/sqlite-graph-schema.sql:19`.

| Column | Type/default/nullability/check |
|---|---|
| id | TEXT NOT NULL |
| tenant_id | TEXT NOT NULL |
| workspace_id | TEXT NOT NULL |
| head_uid | TEXT NOT NULL |
| tail_uid | TEXT NOT NULL |
| predicate | TEXT NOT NULL |
| source_id | TEXT NOT NULL |
| version | INTEGER NOT NULL |

Constraints: PRIMARY KEY (head_uid, tail_uid, id); FOREIGN KEY (head_uid) REFERENCES oc_graph_entities(uid) ON DELETE CASCADE; FOREIGN KEY (tail_uid) REFERENCES oc_graph_entities(uid) ON DELETE CASCADE.

### oc_workspaces

Source: `src/storage/sqlite-schema.sql:14`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| id | BLOB PRIMARY KEY |
| name | TEXT NOT NULL |
| owner_workspace_id | BLOB |
| created_at | INTEGER NOT NULL |

Constraints: UNIQUE (tenant_id, id); CHECK (owner_workspace_id IS NULL OR owner_workspace_id != id); FOREIGN KEY (tenant_id, owner_workspace_id) REFERENCES oc_workspaces(tenant_id, id).

### oc_api_keys

Source: `src/storage/sqlite-schema.sql:45`.

| Column | Type/default/nullability/check |
|---|---|
| id | BLOB PRIMARY KEY |
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| token_hash | TEXT NOT NULL UNIQUE |
| role | TEXT NOT NULL CHECK (role IN ('reader','writer','reviewer','admin')) |
| allow_owned_workspaces | INTEGER NOT NULL DEFAULT 0 CHECK (allow_owned_workspaces IN (0,1)) |
| revoked | INTEGER NOT NULL DEFAULT 0 |
| created_at | INTEGER NOT NULL |

Constraints: FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id).

### oc_files

Source: `src/storage/sqlite-schema.sql:57`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| id | BLOB NOT NULL |
| name | TEXT NOT NULL |
| media_type | TEXT NOT NULL |
| hash | TEXT NOT NULL |
| size | INTEGER NOT NULL |
| deleted | INTEGER NOT NULL DEFAULT 0 |
| created_by | BLOB NOT NULL |
| created_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, id); FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id).

### oc_events

Source: `src/storage/sqlite-schema.sql:66`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| id | BLOB NOT NULL |
| content | TEXT NOT NULL |
| kind | TEXT NOT NULL |
| file_id | BLOB |
| state | TEXT NOT NULL DEFAULT 'active' CHECK (state IN ('active','retracted')) |
| created_by | BLOB NOT NULL |
| created_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, id); FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id); FOREIGN KEY (tenant_id, workspace_id, file_id) REFERENCES oc_files(tenant_id, workspace_id, id).

### oc_assets

Source: `src/storage/sqlite-schema.sql:76`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| id | BLOB NOT NULL |
| kind | TEXT NOT NULL CHECK (kind IN ('memory','knowledge')) |
| title | TEXT NOT NULL |
| fact_key | TEXT |
| current_version | INTEGER |
| deleted | INTEGER NOT NULL DEFAULT 0 |
| created_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, id); FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id).

### oc_versions

Source: `src/storage/sqlite-schema.sql:87`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| asset_id | BLOB NOT NULL |
| version | INTEGER NOT NULL CHECK (version > 0) |
| content | TEXT NOT NULL |
| content_hash | TEXT NOT NULL |
| source_event_id | BLOB NOT NULL |
| restored_from | INTEGER |
| title | TEXT NOT NULL |
| created_by | BLOB NOT NULL |
| created_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, asset_id, version); FOREIGN KEY (tenant_id, workspace_id, asset_id) REFERENCES oc_assets(tenant_id, workspace_id, id); FOREIGN KEY (tenant_id, workspace_id, source_event_id) REFERENCES oc_events(tenant_id, workspace_id, id).

### oc_jobs

Source: `src/storage/sqlite-schema.sql:97`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| id | BLOB NOT NULL |
| created_by | BLOB NOT NULL |
| operation | TEXT NOT NULL |
| payload | TEXT NOT NULL |
| state | TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','processing','retry_wait','completed','failed','cancelled','superseded')) |
| run_token | INTEGER NOT NULL DEFAULT 0 |
| generation | INTEGER NOT NULL DEFAULT 1 |
| attempt | INTEGER NOT NULL DEFAULT 0 |
| next_retry_at | INTEGER |
| asset_id | BLOB |
| source_event_id | BLOB |
| cancel_requested | INTEGER NOT NULL DEFAULT 0 |
| outcome | TEXT |
| result | TEXT |
| error_code | TEXT |
| created_at | INTEGER NOT NULL |
| updated_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, id); FOREIGN KEY (tenant_id, workspace_id, asset_id) REFERENCES oc_assets(tenant_id, workspace_id, id); FOREIGN KEY (tenant_id, workspace_id, source_event_id) REFERENCES oc_events(tenant_id, workspace_id, id).

### oc_chunks

Source: `src/storage/sqlite-schema.sql:113`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| id | BLOB NOT NULL |
| asset_id | BLOB NOT NULL |
| version | INTEGER NOT NULL |
| ordinal | INTEGER NOT NULL |
| content | TEXT NOT NULL |
| locator | TEXT NOT NULL |
| search_terms | TEXT NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, id); UNIQUE (tenant_id, workspace_id, asset_id, version, ordinal); FOREIGN KEY (tenant_id, workspace_id, asset_id, version) REFERENCES oc_versions(tenant_id, workspace_id, asset_id, version).

### oc_summaries

Source: `src/storage/sqlite-schema.sql:124`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| id | BLOB NOT NULL |
| chunk_id | BLOB NOT NULL |
| text | TEXT NOT NULL |
| model_revision | TEXT NOT NULL DEFAULT '' |
| search_terms | TEXT NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, id); FOREIGN KEY (tenant_id, workspace_id, chunk_id) REFERENCES oc_chunks(tenant_id, workspace_id, id).

### oc_commands

Source: `src/storage/sqlite-schema.sql:132`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| principal_id | BLOB NOT NULL |
| dedup_scope | TEXT NOT NULL |
| operation | TEXT NOT NULL |
| key | TEXT NOT NULL |
| request_hash | TEXT NOT NULL |
| response | TEXT NOT NULL |
| created_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, dedup_scope, operation, key).

### oc_audit

Source: `src/storage/sqlite-schema.sql:140`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| id | BLOB NOT NULL |
| actor | BLOB NOT NULL |
| action | TEXT NOT NULL |
| target | BLOB NOT NULL |
| details | TEXT NOT NULL |
| created_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, id).

### oc_artifact_owners

Source: `src/storage/sqlite-schema.sql:152`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| source_id | BLOB NOT NULL |
| version | INTEGER NOT NULL |
| artifact_type | TEXT NOT NULL CHECK (artifact_type IN ('chunk','summary','entity','relation')) |
| artifact_id | BLOB NOT NULL |
| chunk_id | BLOB |
| created_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, source_id, version, artifact_type, artifact_id).

### oc_index_entries

Source: `src/storage/sqlite-schema.sql:163`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| artifact_id | BLOB NOT NULL |
| field | TEXT NOT NULL |
| model_id | TEXT NOT NULL |
| dimension | INTEGER NOT NULL |
| generation | INTEGER NOT NULL |
| state | TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','ready','removed')) |
| created_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, artifact_id, field, model_id, generation).

### oc_artifact_ledger

Source: `src/storage/sqlite-schema.sql:175`.

| Column | Type/default/nullability/check |
|---|---|
| tenant_id | BLOB NOT NULL |
| workspace_id | BLOB NOT NULL |
| source_id | BLOB NOT NULL |
| version | INTEGER NOT NULL |
| artifact_type | TEXT NOT NULL CHECK (artifact_type IN ('chunk','summary','entity','relation')) |
| artifact_id | BLOB NOT NULL |
| surface | TEXT NOT NULL CHECK (surface IN ('vector','graph','full_text')) |
| generation | INTEGER NOT NULL DEFAULT 1 |
| idempotency_key | TEXT NOT NULL |
| state | TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','committed','retry_wait','orphan')) |
| attempt | INTEGER NOT NULL DEFAULT 0 |
| last_error | TEXT |
| next_retry_at | INTEGER |
| created_at | INTEGER NOT NULL |
| updated_at | INTEGER NOT NULL |

Constraints: PRIMARY KEY (tenant_id, workspace_id, source_id, version, artifact_id, surface, generation).

Inventory totals: 25 schema tables, 233 columns; plus the 4-column oc_storage_format marker in native_turso.rs and two 7-column native Lance schema families listed above. Indexes/triggers remain defined in the source schema files; notably immutable/root workspace ownership, memory fact-slot uniqueness, source/job lookup indexes, ledger state retry index, and one-unfinished-Improve-per-session partial uniqueness.
