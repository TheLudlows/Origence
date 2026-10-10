# Origence implementation constraints

Read the relevant design, `docs/STATUS.md`, and current code before changing an
implementation. Prefer the smallest change that meets the documented behavior.

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
