# Storage invariants

**English** · [简体中文](storage.md)

This page defines the data semantics the current code must preserve. See [implementation architecture](../design/current-implementation.en.md) for component responsibilities and [OPERATIONS](../OPERATIONS.en.md) for initialization, failures, and backups. The complete fields and constraints are defined by the [authority schema](../../src/storage/sqlite/schema.rs) and its DDL. This page does not duplicate each field or physical table count.

## AnySeek connection boundary

Local mode uses the Rust SDK to open `OC_DATA_DIR/anyseek` in the host process. Cloud mode configures a remote endpoint and service credential. Both route from authenticated `Scope` to the tenant_id canonical UUID string and a `ws_<workspace_id.simple()>` collection. There is no default namespace, collection setting, or cross-workspace fallback. Remote data requests use `X-AnySeek-Tenant = tenant_id` and `apikey`. Startup opens the local Catalog or checks remote connectivity and credentials, and verifies workspace collections with existing publications. On first use, a new workspace creates a collection using fixed schema. Verify unknown creation outcomes through strict readback. A missing or incompatible published collection or changed analysis digest fails rather than serving an empty collection. Origence still authoritatively checks candidate scope, source, and version. The `origence-content-backend-v2` binding pins target, routing, model space, and analysis digest on first use. It contains no credential. Refuse to open a directory with publications and no initialized digest. AnySeek neither stores nor issues tenant keys.

## SQLite and minimal metadata

Authority data and jobs use bundled SQLite 3.54.1 (rusqlite 0.43.0, defaults off and bundled enabled), with no ORM or separate async database dependency. `SqliteStore` / `SqliteTx` are the only local relational implementation. Engine marker is `origence-sqlite-v2`. The old engine has no adapter, alias, or fallback path.

SQLite is optional in the library. Under `--no-default-features` the core, parsing, models, Blob storage, and remote AnySeek adapter do not compile or access SQLite. `--features sqlite` enables the authority adapter. `local-storage` builds the complete host using remote AnySeek and SQLite, and default `embedded-anyseek` adds the local SDK. The full host keeps original source events, permissions, ownership, version pointers, idempotency, durable jobs, and recovery facts in SQLite. Published text and summary text live in AnySeek.

Requests read metadata that participates in the operation. Empty batches and empty search words return empty. Without upstream candidates, downstream indexes, Summary, sources, and owners are not queried. Missing Learning, failed preconditions, and cancelled parent jobs also stop unrelated reads. Nonempty evidence and required reads/writes still use strict validation. Open and explicit health checks validate full DDL. Business no-ops do not probe the whole schema.

The service transaction performs one initial identity/permission lookup in the same snapshot and records commit requirements. Permissions are checked again before commit and are not cached across requests. Keyword requests reuse the initial transaction for candidate reads. Vector and hybrid close it before model I/O and retain a fresh later snapshot and final publication, withdrawal, and authorization checks.

Persist only business facts, authorization evidence, and recovery state that cannot safely be reconstructed.

- The adapter calculates and stores version and chunk `content_hash` from the original UTF-8 bytes, binding immutable AnySeek text. The host rehashes on read. A version joins chunks in ordinal order without another complete text copy. HTTP and Skill evidence hash fields are unchanged.
- The publication ledger uses its existing complete primary key for idempotent registration. It no longer stores a concatenated `idempotency_key` or makes the Worker carry the redundant string. External publication retains frozen plans and original operation epoch.
- Command request digests, private Run/Skill snapshot digests, original-file hashes, parser/model provenance, frozen retrieval terms, and owner/index/ledger each support validation or recovery and cannot be removed as duplicates.

Writable connections explicitly use WAL, `synchronous=FULL`, foreign keys, and a 5-second busy timeout. Each pool has at most five exclusive leases. Synchronous SQLite I/O runs on ordered blocking connection threads. Cancelling an await cannot hard-stop SQL. Later operations reject that lease, while queued rollback runs after the original operation and returns the connection. Shutdown waits for leases, rollback, and handles before releasing the host lock, with no additional durable Worker. An issued COMMIT may complete without caller confirmation. Check the original idempotency key and durable state to resolve uncertainty. Await cancellation cannot undo committed data.

With the host stopped and no nonempty WAL, read-only admission uses an immutable connection and creates no sidecar. A committed WAL must be read without immutable mode. SHM read marks are transient SQLite coordination state and are not backup business facts. A read-only access can update them without rewriting the database or committed WAL.

## Collection shared by text and indexes

Each `origence-content-sha256-v1` Record represents one chunk. `payload` is stored unindexed JSON with original UTF-8 text, full locator, optional summary, and model version. The same Record's `terms`, vector, and graph retain existing retrieval behavior. No separate whole-text, version, or summary Record alters BM25 corpus count. SQLite no longer saves `oc_versions.content`, `oc_chunks.content`, `oc_summaries.text`, `oc_chunks.search_terms`, or `oc_summaries.model_revision`. Chunk terms are AnySeek-only, summary model versions live in payload, and SQLite retains only terms used by Summary retrieval.

SQL retains source and visibility locators, frozen Summary terms, Run/Skill receipts, and recovery plans.
Private AnySeek collections hold complete Q&A, the turn directory, and search fields. SQLite
holds workspace and logical-key ownership, session sequence allocation, command receipts, and
proofs for actual dependencies. AnySeek deletes each body and its index as one group and retains the
group version to fence delayed writes. Directory binding
and collection schema are versioned together.
[Private archive backup and restore](../OPERATIONS.en.md#private-archive-and-paired-recovery)

Parse AnySeek payload outside transactions and compare chunk hash and full locator. Residual generations may have duplicates only if the same chunk payload is identical. Conflicting content cannot be picked arbitrarily. Check the assembled version hash too. GET, Search, and Context start a fresh transaction after remote I/O to revalidate permissions, source, and publication identity. A nonempty Search takes a final snapshot after reading text. Empty results avoid that read. Historical restore reads the original version's immutable chunks, then uses the original publication guard. Missing, truncated, or mismatched data fails explicitly without reparsing original sources. Internal semantic memory merging reconstructs exact quotes from original historical source byte ranges and verifies hashes without remote I/O inside the transaction.

## Identity reservations

- `oc_assets.identity_key` and `identity_json` are both null or both present on a memory entry. `oc_memory_identity_slot` enforces uniqueness across complete scope and preserves reservations after deletion.
- `fact_key` is a separate fact slot. Normal structured writes set it to the same canonical identity key, but the two columns cannot be merged, and legacy fact_key is not a full identity.
- Identity stays bound to the content item. Historical restore does not change it, and deletion does not release the reservation.
- A selected structured identity must validate type, application fields, canonical key, and text consistency. Corruption is not repaired. Check identity mismatch before tombstones so inconsistent content items do not look like ordinary deletion conflicts. Corrupt structured identity is Unavailable. Native column type errors remain Backend. Absence of an identity means unbound.
- Exact identity lookup depends on the schema-guaranteed `oc_memory_identity_slot` index. No silent fallback if it is missing. Combine batched reads in the same transaction without replacing them with row-by-row queries.

## Independent lifecycles

| Object | Semantics to preserve |
| --- | --- |
| Original event / Blob | Immutable input, file bytes, and independent source withdrawal |
| Content item / version | Stable content item identity, current version pointer, and append-only historical text |
| Command / job | Call idempotency, async work, cancel/retry, and creator reauthorization |
| artifact owner | Exact source/version ownership of an artifact. Shared artifacts may have multiple owners. |
| index entry | Index readiness and dimension for artifact, field, profile, and generation |
| publication ledger | External publication status and recovery identity for source, version, surface, and generation |
| Private session / Run / Skill | Logical-key ownership, immutable events and revisions, review permissions, and published versions |

`index entry` and `publication ledger` have distinct keys, states, and lifecycles. They cannot substitute for each other. ready plus committed does not establish visibility. Reads still check permissions, current content item version, sources, owner, and semantic snapshot or Learning supports. Job generation, business version, model profile, and execution run_token are independent identities.

All publication paths generate text and summary terms once in a blocking task outside transactions and freeze them with the plan. Retry must reuse original terms. Missing fields mean a corrupt plan and do not trigger tokenization. Persist the complete plan and pending ledger first, then write text, summary, locators, and indexes to the same AnySeek Record outside the transaction and revalidate and commit authoritatively. Prepared MessageBatch plans persist with a lightweight receipt digest. Same-generation recovery reuses the plan and verifies the digest. Missing or corrupt data fails explicitly. The digest neither replaces actual index and ledger checks nor rescans all vectors. Job attempts and error text remain with the queue, and the job controls retry limits and execution fencing.

## Sources and validity

Preserve historical locators, parser markers, pages, UTF-8 byte ranges, model provenance, and original support sources. Content item version, original source version, and root source of the snapshot are separate. Never reinterpret historical offsets using the current parser. Withdrawal blocks reads in authority first and then cleans projections. Logical removal does not erase bytes.

All relation composition follows the single-table reads and same-transaction Rust rules in [AGENTS](../../AGENTS.md#sql-query-constraints). Model, parsing, files, and remote index I/O run outside the authority transaction. Recheck authorization before return and commit.

## Initialization and recovery

Create only the complete current schema. A format marker cannot replace full DDL validation. Incompatible databases are rejected without installing tables, migrating, rewriting, or deleting data. Changing models or index targets does not reinterpret durable plans. Same-version reopening, original-plan retries, and offline backup and restore remain. Back up the embedded Catalog with the entire Origence directory. Back up standalone AnySeek collections separately. The Origence directory and remote indexes have no shared transaction or online consistent snapshot.

Field inventory, table-consolidation reviews, and measurements remain at the [pinned historical revision](https://github.com/TheLudlows/Origence/tree/0a1a56c1da74e78be0118782b04a3f5a37251060/docs/contracts) and do not describe current schema or performance.
