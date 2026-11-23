# Local operations guide

**English** · [简体中文](OPERATIONS.md)

Applies to a single host using SQLite as the authority, Blob storage, and AnySeek retrieval. Jump to [startup and credentials](#initialization-and-credentials) · [AnySeek](#anyseek-full-text-and-vector-retrieval) · [models](#local-embedding-service) · [failure recovery](#failures-and-recovery) · [backup](#backup-and-restore). See [STATUS](STATUS.en.md) and [VALIDATION](VALIDATION.en.md) for progress and tested boundaries.

## Current scope

This stage targets newly created databases. Startup requires the complete current schema and does not add missing tables, upgrade, or migrate existing data. Same-version durability, restart and retry, and backup and restore remain required. Incompatible schemas cannot be opened. Retain original sources and backups, and keep old data intact when using a new directory. See [storage invariants](contracts/storage.en.md) for transaction queries, identity reservations, and publication plans.

## Directories and processes

`OC_DATA_DIR` defaults to `.data` and contains the following.

| Path | Contents |
| --- | --- |
| `context.db` and SQLite WAL | Permissions, original source events, version pointers and content hashes, locators, summary retrieval terms, idempotency, jobs and recovery plans, owners, ledger, and audit |
| `context.db.lock` | OS lock file for exclusive host access. The file can exist when the lock is no longer held. |
| `anyseek/` | Default local AnySeek Catalog and workspace collections, including text, summaries, and full-text, vector, and graph indexes. A standalone deployment is backed up separately according to AnySeek's rules. |
| `full-text-backend.json` | AnySeek endpoint, automatic tenant/workspace routing rules, and embedding profile and dimension binding. Contains no key and is backed up with the entire directory. |
| `blobs/<tenant>/<workspace>/uploads/<hash>` | Original file contents, located by scope and content hash |

Lock file names and database paths follow the configured startup implementation. Never edit business records directly in either database. Vector and graph indexes are not authoritative for source state. Grant data directory access only to the host OS identity. API keys cannot restrict users who already have local file access.

`origence serve` (compatibility alias `api`) runs HTTP and a single Worker. Ctrl+C requests graceful shutdown. HTTP stops accepting requests, and storage closes after the Worker finishes its current job. After a forced exit, the OS releases the lock automatically. The next startup advances the run_token and replays unfinished jobs. Do not remove lock files to bypass a running host.

`/health/live` indicates process liveness. `/health/ready` requires a running Worker and successful storage checks. Model services do not affect readiness. Individual model failures appear in job state. Business endpoints stay closed if startup reconciliation or storage checks fail.

## PDF parsing

Text PDFs use pdf_oxide 0.3.78. Each PDF is opened once and text is extracted page by page. The single Worker waits serially for synchronous I/O and parsing in spawn_blocking. Library callers can invoke the parsing module directly.

| Check | Boundary |
| --- | --- |
| Input and output | Actual bytes read ≤10 MiB, 1–200 pages, and total extracted text ≤1,000,000 UTF-8 bytes. These are input/output bounds, independent of parser memory use. |
| Completeness | Each page must contain at least 3 letters or digits. Blank pages, scanned pages without text, encryption, excess content operands, or premature object EOF fail the entire document. Partial text is never published. |
| Features and errors | No OCR or password decryption. I/O failures are internal errors, and unsupported or invalid PDFs are input errors. |
| Source location | Page numbers start at 1. Byte and line numbers refer to extracted text within a page, not PDF binary offsets. The new parser marker is pdf-oxide-0.3.78-v1. Keep old versions and markers unchanged. Only re-imports and updates use the new parser. |
| Logs | Raw library logs remain disabled even at higher RUST_LOG verbosity. The Worker logs only job IDs and safe error codes. |

Cargo.lock pins compatible office_oxide 0.1.9. The DocumentIR field change in 0.1.13 prevents pdf_oxide 0.3.78 from compiling. Run locked builds and parser regressions before upgrading. Keep the lockfile.

The old implementation used a subprocess to terminate parsing after a hard 30-second timeout. That hard deadline has been removed. Cancelling the future or JoinHandle cannot stop the synchronous closure in `spawn_blocking`, and input/output bounds do not impose hard limits on the parser's internal memory or CPU. Graceful shutdown waits for the current job. Cancelling a job prevents late publication but cannot immediately stop parsing. If untrusted documents later require hard time or memory limits or crash isolation, use a dedicated constrained parser process with its own lifecycle, outside `main`. The current version makes no guarantee about parsing time for arbitrary PDFs.

## Initialization and credentials

Run `origence --offline workspace-create NAME`, save the returned token, and then start the host. Online `key-create` and `key-revoke` use `/admin/keys` within the token's workspace and require admin. Offline administration uses OS authorization, requires no API token, and needs exclusive access to the data directory.

Origence reads process environment variables. It does not load `.env` automatically. Compose reads `.env`. Keep model credentials, API tokens, and `.env` out of Git. See origence --help for command arguments and the next section for local model setup.

## AnySeek full-text and vector retrieval

Full-text, vector, and one-hop graph candidates come only from AnySeek. SQLite decides versions, permissions, sources, and visibility. Published text and summary text are persisted in the same AnySeek collection. Claude Code and Codex need only an Origence key. See [client integration](CLIENTS.en.md).

### Origence local mode

```sh
origence serve --local
```

Origence opens `OC_DATA_DIR/anyseek` in its own process through the AnySeek Rust SDK. AnySeek uses local disk and creates workspace collections on first use. Origence exposes its own HTTP API after startup recovery and storage verification. Local mode uses a distinct backend binding and a fresh Origence data directory. An existing embedded binding with a missing Catalog prevents startup.

On shutdown, Origence stops accepting requests, lets the Worker finish, and waits for the SDK to drain accepted AnySeek operations and release the directory lock. SDK queries share the process and use AnySeek admission control and resource budgets. After an abnormal exit, the next startup recovers from the durable log. Forced termination can interrupt graceful shutdown. HTTP request timeouts apply only to the standalone service connection.

### Cloud deployment with standalone AnySeek

The cloud image builds only the remote AnySeek client. Its default entry point, `origence serve`, connects to standalone AnySeek using `OC_ANYSEEK_URL` and `OC_ANYSEEK_API_KEY`. Startup fails if either setting is missing. Use an HTTPS origin reachable from the Origence container. The deployment environment injects the service-to-service credential.

```sh
export OC_ANYSEEK_URL=https://anyseek.example.internal
# 通过部署环境注入 OC_ANYSEEK_API_KEY。
docker compose up --build -d host
```

On bare-metal cloud hosts, build the remote-only binary with `cargo build --release --locked --no-default-features --features local-storage --bin origence`. The standalone AnySeek revision checked by CI is `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`.

Each request takes `tenant_id` from the authenticated server-side Scope and sends it as `X-AnySeek-Tenant`. The `workspace_id` maps to `ws_<去掉连字符的 UUID>` (the UUID without hyphens). Each workspace has one collection. Separate workspaces within one tenant use different collections and separate tenants use different namespaces. Clients and models cannot set AnySeek routing or service credentials. Origence user keys, private sessions, candidate scope filtering, and final authorization, source, and version checks still apply.

At startup, the host validates the service credential and version through `/api/v1/live`. It creates no placeholder collections. At first retrieval or publication, the host reads the workspace collection and creates it from the fixed schema only if absent. After concurrent creation or a lost creation response, strict readback verifies the schema, tokenizer implementation, stop-word digest, capacity, dimensions, and embedding_space_id. An incompatible collection is never overwritten. Inspect the required schema with `origence anyseek-schema [--with-space]`. No manual collection creation is needed.

`OC_ANYSEEK_URL` must be an HTTPS origin, with HTTP allowed on loopback. It must not contain paths, credentials, or query parameters. Container loopback refers to the container itself. The `origence-content-backend-v2` binding records the endpoint, routing, and fixed model profile and dimensions. It stores no service credential. On first collection use, persist the analysis digest before remote publication. Only empty directories with no published full text may lack an initialized digest. On reopening, validate collections associated with authoritative publications. A missing committed collection or configuration drift fails explicitly and never triggers silent recreation. Credential rotation for the same target leaves the binding intact. Older binding formats are not converted automatically. Backups must include the full Origence directory and each tenant/workspace collection according to AnySeek's procedures.

Each publication is limited to 64 chunks and 2 MiB of encoded request data. Text and vectors are written together as one Record group. SQLite publishes the authoritative version only after the remote commit. After an unknown timeout outcome, inspect the current group and recover eligible jobs from the original durable plan. Keep evidence for unresolved outcomes beyond the original operation window instead of guessing with a new ID. Retrieval filters by authorized identity, verifies index candidates, and then uses SQLite for a final visibility check. Withdrawal blocks reads first, then removes index data asynchronously. Local unit tests do not establish real model quality, production capacity, or online cross-system snapshots.

## Local embedding service

This example uses Ollama's BGE-M3 to provide OpenAI-compatible embeddings.

```powershell
$env:OC_ENABLE_MODELS = 'true'
$env:OC_MODEL_BASE_URL = 'http://127.0.0.1:11434/v1'
$env:OC_MODEL_API_KEY = 'ollama'
$env:OC_EMBEDDING_MODEL = 'bge-m3'
$env:OC_EMBEDDING_DIMENSION = '1024'
```

ollama serves as a placeholder key for the local service. The commands configure the current PowerShell session. Repeat them in a new terminal or configure persistent environment variables. A running host does not refresh automatically. Download bge-m3 first and keep Ollama running. The endpoint is POST /v1/embeddings. This configuration supplies no extraction or generation model.

Changing the model or dimension does not rebuild old vectors. Use a separate temporary data directory for evaluation. Re-import or republish business data under the existing profile constraints. See [evaluation instructions](../evals/README.en.md) for entry points. Historical experiments on an old backend are not current AnySeek quality measurements.

## Offline memory identity check

New native SQLite databases have the complete identity schema. Identities are recorded in oc_assets, and deleted assets retain their reservations. See [storage invariants](contracts/storage.en.md).

`identity-check` performs read-only, same-version validation of an existing database, including the format marker and complete authority schema. It does not create or repair tables.

1. Gracefully stop the host and all offline clients, and back up the entire data directory.
2. Run `origence --data-dir DATA_DIR --offline identity-check`.
3. A valid same-version database produces `{"feature":"memory-identity-v1","valid":true}`. Failed checks exit nonzero.

Validation rejects missing files or tables, unmarked legacy databases, weak constraints, and name collisions with views. It installs or converts nothing. The command does not load models, AnySeek, graph, or Blob. It depends on OS directory permissions and an exclusive path lock. A missing context.db is not created. Read-only validation has no upgrade status or write mode. Library callers use `SqliteStore::check_memory_identity(path)`, which returns `()` on success.

## Failures and recovery

| Symptom | Action |
| --- | --- |
| The lock is held | Use the HTTP CLI or MCP. For offline administration, first stop the host gracefully. |
| Model or parsing failure | Fix the configuration or input. A failed or cancelled job with valid sources can be retried explicitly. |
| Transient storage error | The job enters retry_wait and retries up to 5 times. The error is persisted and the job becomes failed if attempts are exhausted. |
| Worker or host exits | Fix the storage error and restart. A processing job is recovered on exclusive startup without waiting for lease expiration. |
| Cancellation or deletion during model processing | Late commits revalidate generation, authorization, and source state. They cannot overwrite or revive data. Serial cleanup removes external orphans. |
| Native write completed before SQLite publication committed | Replay using the stored publication plan and ledger. On startup, first remove artifacts with no owner, then write and publish idempotently. |
| cleanup failed | The tombstone still blocks reads. Fix storage and retry the failed cleanup, or restart to reconcile the full scope. |
| Published text is missing or its hash or locator differs | Stop relying on the data, preserve evidence, and restore a matching complete backup. Do not automatically reparse the source or fall back to empty text. |
| Incompatible schema | Stop and preserve evidence. The host does not run automatic ALTER. Use the matching version or a new directory without overwriting existing data. |

The accepted job's embedding profile must match the Worker configuration. Changing models does not rebuild existing vectors. A forced exit before saving the publication plan can repeat a model call. Publication is idempotent, but exactly-once external provider billing is not guaranteed.

## Graph storage and source boundaries

External hybrid search expands only one hop. AnySeek's `origence-content-sha256-v1` collection provides entity and edge candidates. They attach by source version to an existing chunk Record and publish atomically with that group. The limit remains 64 chunks and the full request byte budget. Oversized requests are rejected without partial publication.

The owner and publication ledger in `context.db` remain the evidence authority. Withdrawing a source immediately blocks its evidence. Residual AnySeek projections cannot revive relationships. A relationship can still be retrieved through another valid source. Graph projections are cleaned up with their source's chunk group. The host no longer creates, opens, or maintains a separate `graph.db`. Existing graph files are not read, migrated, or deleted. Legacy upgrade is unavailable. Do not clear the ledger or index by hand in an attempt to migrate.

## Backup and restore

SQLite is compiled into the program, so a separate SQLite service or client library is unnecessary. Writable connections use WAL, `synchronous=FULL`, and foreign-key enforcement. Graceful shutdown waits for the connection thread to finish SQL, roll back, and close the database. Request cancellation may not immediately stop in-flight SQL. A cancelled lease cannot continue subsequent business operations. An already issued COMMIT may complete without acknowledgment. Reuse the original command's idempotency key when retrying to confirm the result. See [storage contract](contracts/storage.en.md#sqlite-and-minimal-metadata) for metadata reduction, format, and read-only WAL checks.

1. Stop the host gracefully. Confirm the process has exited and no offline client holds the data directory.
2. Copy the **entire** `OC_DATA_DIR` to the backup location, including SQLite and its WAL, the AnySeek binding, blobs, and related files. Record the binary version and non-sensitive model configuration. Include `anyseek/` in embedded mode. For standalone deployments, save a separate same-version backup of the corresponding collections under AnySeek's procedures.
3. Restore to a separate directory. Run the same-version offline check before starting. Verify readiness, sample get/search, and job state.

Avoid copying the databases separately while online or backing up only context.db. No online cross-database snapshot protocol is available. An older dedicated test database passed a full offline backup and restoration. Production backup/restore and power-loss tests still require separate acceptance. Logical deletion does not remove original text, which is also included in backups.

## Common message batches and owning workspaces

A root administrator explicitly grants access to an owning workspace. A root credential with the capability is intended for trusted services. Derived access is restricted to reader or writer. The caller chooses the target workspace through the public Rust API. See [API](API.en.md#common-owning-workspace-and-message-batch-library-apis) for interfaces, idempotency, and authorization.

Message batch submissions require an embedding profile that matches the accepted specification. Batches are indexed using vectors of the original text. The accepted specification is persisted with the job. Parsing and external calls stay outside storage transactions and the existing single Worker performs ingestion. Before rotating a credential, you can wait for its submitted jobs to finish. Revocation blocks publication by unfinished jobs under that credential.

Library callers specify an observation deadline via `wait_message_batch`. After the deadline, the Worker continues processing the durable job. The HTTP host no longer implements long waits for the old evaluation endpoint. Graceful shutdown still waits for the current Worker job and in-flight HTTP requests.

## Retrieval stage diagnostics

Set RUST_LOG=error,origence::search_timing=debug to log authorization_ms, embedding_ms, eligibility_ms, vector_ms, branches_ms, verification_ms, total_ms, and vector_batches/native_count/returned_count for successful searches. Logs omit queries, evidence, user and asset IDs, paths, and credentials. The default info level does not emit these debug events.

branches_ms covers candidate loading, fusion, and enabled auxiliary branches. verification_ms measures the final fresh authorization snapshot and identity check. total_ms covers the period after input validation through part of result serialization. It excludes outer HTTP encoding and network time. Measure end-to-end HTTP latency separately. Failures and early returns for unknown identities produce no completion event, so these events cannot measure error rates. This change adds timing only and preserves Worker, timeout, cancellation, shutdown, isolation, and read-permission guarantees.

## Memory and controlled extraction limits

Administrators register properties before assigning the first related identity. Definitions are immutable, and editing tables cannot be used to bypass the contract. Semantic writes reuse the single Worker, persisted plans, version conflicts, and source checks. Withdrawing any support source removes the entire version from reads. Generic restore cannot bypass that rule. See the [memory contract](contracts/memory.en.md).

`semantic_extract` uses the existing extraction configuration. Each model request is capped at 45 seconds. Model calls, JSON validation, and tokenization run outside transactions. The plan is persisted before embedding. A stored plan reuses its original provenance. Failures before plan commit can repeat model calls, with no exactly-once external billing guarantee. Without a stored plan, the extraction configuration fingerprint must match what was accepted. The embedding profile must always match.

The caller explicitly supplies targets/expected_version/allow_update. There is no automatic listener or additional Worker. Recovery and final commit revalidate creator, generation/run_token, version, and all sources. Cancellation prevents late publication, but may not stop model calls or blocking work already issued. Graceful shutdown waits for the current job. An empty candidate list or duplicate can complete successfully. A superseded job requires a new version read before resubmission. See [controlled extraction](contracts/semantic-capture.en.md) and [API](API.en.md#semantic-conversation-capture) for input and output limits.

## Query diagnostics and Run

Compare the binding's analysis_fingerprint with the AnySeek collection's analysis.fingerprint. Use x-anyseek-request-id from the new API response to correlate with the server's response_prepared debug log. HTTP errors provide a safe x-anyseek-error-code. The log indicates response preparation only, without confirming body delivery. Query diagnostics v2 must return the configured embedding_space_sha256. A space change is rejected even if dimensions match. A successful Run preparation can reuse a private snapshot during a backend outage, but replay still checks current authorization and original sources. The successful receipt does not establish backend recovery. New preparations still need backend access. Run retrieval diagnostics distinguish requests never issued, network failures, HTTP errors, and invalid diagnostics. Raw error bodies are not kept. Backend indexing progress is sampled after the query. Handle AnySeek health first if it reports lag, failure, or recovery flags. Do not advance the consumer sequence by hand or mask the failure with empty retrieval. See [API](API.en.md#retrieval-diagnostics) for fields and collection boundaries.

## Native Windows development launcher

Run `origence.exe serve --local` to start the API and local AnySeek. To also start the Node UI, run [`tools/start-dev.ps1`](WINDOWS-DEV.en.md) in the same PowerShell 7.4+ console. The launcher disables models.
