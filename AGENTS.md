# Origence implementation constraints

Read the relevant design, `docs/STATUS.md`, and current code before changing an
implementation. Prefer the smallest change that meets the documented behavior.

## Current-stage database scope

- Target fresh databases for current design, implementation, testing, and
  acceptance. Version upgrades, backward compatibility with older database
  schemas, and data migrations are out of scope; do not track their absence as
  current gaps or add upgrade/migration work to current milestones.
- Reconsider this scope only after an explicit future requirement change.
  Existing compatibility checks, historical tools, and validation records do
  not create a requirement to extend upgrade or migration support.
- This scope does not relax durability, restart/retry recovery, same-version
  backup/restore, source preservation, or incompatible-schema rejection.
  A fresh-database development target is not permission to delete, overwrite,
  or silently convert existing data.

## SQL query constraints

- All application-owned queries must be JOIN-free, including tests and generated
  SQL. Do not replace an explicit JOIN with comma-separated FROM sources,
  self-joins, correlated cross-table subqueries, or CTEs that reconstruct the
  same association. Query one relation at a time and combine scoped results in
  Rust inside the existing transaction; retain authorization, source/version/
  generation checks, ordering, multiplicity, NULL handling and limit semantics.
- Keep relational query composition inside storage adapters. Splitting a query
  must not introduce an intermediate commit, weaken snapshot consistency, move
  native/model IO into a transaction, or skip final publication validation.
- Review SQL execution inputs after constant/format/concat/include expansion,
  including test batches and triggers. Review new execution APIs, generated SQL,
  dynamic inputs, and unsupported constructs manually; do not treat a keyword
  grep as proof. Preserve historical SQL in non-executed documentation as
  evidence. Verify query changes with real Windows/Linux behavior tests.
- The automatic SQL guard and its tests have been removed. The JOIN-free coding
  constraints and manual review requirements above remain in force.

## Module boundaries and dependencies

- Keep `src/main.rs` limited to CLI/configuration, startup, and dispatch. Put
  parsing and other domain logic in library modules callable by any host.
- Prefer an existing library API over a custom parser, shell command, or hidden
  CLI protocol. Library code must not depend on `current_exe()` or re-enter the
  application's CLI to perform an internal operation.
- A subprocess is appropriate only for a documented requirement such as hard
  cancellation, crash isolation, or an external executable. Explain the need,
  alternatives, lifecycle, resource limits, and regression coverage in the PR.
  Do not add it as a workaround for module boundaries or blocking work.
- Dependency replacement includes removing the old implementation and unused
  direct/transitive dependencies, updating Cargo.lock with Cargo, and checking
  build/CI/container configuration. Enable only the needed crate features.

- External evaluation protocol rules belong in API adapters, never storage
  schemas, generic job types, or authorization decisions. New lower-level
  capabilities must also be verified through a non-AML public-library caller.

## Blocking work and resource limits

- Run synchronous CPU-heavy parsing/file IO on `spawn_blocking`, outside async
  executor threads and storage transactions. Await completion in the serialized
  durable worker; do not create a second independent worker or retry loop.
- `timeout(spawn_blocking(...))`, aborting a JoinHandle, and dropping its future
  do not stop a running blocking closure. Never claim hard cancellation from
  these mechanisms. If hard deadlines are required, design explicit isolation.
- Enforce file, page, and extracted-text limits at the parsing boundary, even
  when upload validation already checked them. Bound actual reads, not only
  metadata. Application output limits are not a hard parser-memory budget.
- Record any change in timeout, cancellation, shutdown, and isolation guarantees
  in operations documentation; do not silently weaken them during refactoring.

## Evidence and validation

- Preserve source bytes, immutable historical locators, one-based PDF pages,
  page-local UTF-8 byte ranges, and parser provenance. Version the parser marker
  when extraction behavior changes. Never silently publish only the pages that
  succeeded or reinterpret historical byte offsets with a new parser.
- Invalid/unsupported documents are input errors; IO and task failures are
  internal errors. Do not expose source text, paths, tokens, or raw library
  diagnostics in client error messages or logs.
- Verify replacements through the public library API and the upload/job/search
  path. Cover multilingual text, page boundaries, malformed input, and limits.
  Tests must not rely on the test executable implementing application CLI commands.
- Run formatting, Clippy with warnings denied, and relevant tests under the
  declared Rust toolchain. Record exact results and limitations in
  `docs/VALIDATION.md`; keep status current and historical evidence unchanged.
