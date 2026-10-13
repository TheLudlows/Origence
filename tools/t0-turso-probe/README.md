# T0: Turso 0.8.2 compatibility probe

This is an isolated, synthetic experiment for [issue #52](https://github.com/TheLudlows/Origence/issues/52).
It does not link Turso into Origence, change the default backend, edit existing
databases, migrate schemas, call a model, or establish production readiness.

## Version and execution

- Declared toolchain: repository Rust **1.98.0**.
- SDK candidate: **turso = 0.8.2**, default features disabled, only `fts` enabled.
- Runtime opt-in: `Builder::experimental_index_method(true)`.
- No libSQL, Cloud, Sync, MVCC, second Worker or new graph engine. The SDK's
  `io_memory_yield` feature is enabled only as a dev dependency for a separate
  forced-Pending cancellation unit test. It never changes the file-backed
  bridge's VFS or stands in for disk recovery evidence.
- This nested workspace has its own Cargo lock. The initial bootstrap must use
  Cargo to generate it; its exact resolved core/sdk-kit versions are evidence,
  not inferred from an SDK version string. Final acceptance requires that
  generated lock committed and all commands repeated with `--locked`.

Run from the repository root:

```sh
# Fixture/driver checks require only Python. This is NOT a Turso pass.
python3 -m unittest discover -s tools/t0-turso-probe -p 'test_*.py' -v
python3 tools/t0-turso-probe/verify.py --sqlite-baseline --output /tmp/t0-sqlite-new

cargo generate-lockfile --manifest-path tools/t0-turso-probe/Cargo.toml # bootstrap only
cargo fmt --manifest-path tools/t0-turso-probe/Cargo.toml -- --check
cargo clippy --locked --manifest-path tools/t0-turso-probe/Cargo.toml --all-targets -j 2 -- -D warnings
cargo build --locked --manifest-path tools/t0-turso-probe/Cargo.toml -j 2
cargo test --locked --manifest-path tools/t0-turso-probe/Cargo.toml -j 2
python3 tools/t0-turso-probe/verify.py \
  --binary tools/t0-turso-probe/target/debug/origence-t0-turso-probe \
  --output /tmp/t0-turso-new
```

Output directory must not exist. Databases are created under a new temporary
directory. `create` refuses an existing file; `open` requires an existing file.
The bridge is not Origence's proposed format discriminator. See the contract
document for the future strict fresh/open policy.

## What the executable checks

The driver reads all six **unmodified production DDL files**, including the
ownership triggers, identity, semantic, Session, Learning and graph schema.
It executes the actual job enqueue/RETURNING/publication JSON SQL and the
current/source-support visibility fragments read from production Rust source.
It compiles every directly usable literal relational/graph SQL string with
`EXPLAIN`; formatted queries are not exhaustively covered and are labeled so.

The shared SQLite baseline validates fixture values and expectations. The Turso
execution additionally exercises deferred SDK transaction-drop cleanup,
unconsumed/partially consumed RETURNING, a once-polled then dropped write future,
connection reuse, and two-connection read snapshots. The cancellation result
records whether the future actually yielded; completion before cancellation is
not reported as evidence of cancellation safety at an arbitrary execution point.
The separate Rust unit test reopens real jobs-schema bytes in the released SDK's
deferred-completion memory VFS, requires an observed Pending, drops the update,
explicitly rolls back and checks connection reuse. That test is a deterministic
SDK-control-flow check, not an on-disk crash/power-loss claim.

FTS probes use the real composite UUID chunks/summaries tables: physical index
creation, EXPLAIN index-method use, explicit AND, cross-summary term union by
full chunk key, Unicode/identifiers/version terms, positive BM25 descending
order, source-filtered top-k, own/cross-connection visibility, rollback,
update/delete and explicit optimize. The follow-up diagnostics compare raw JOIN
scores with same-snapshot direct BM25 and test plain/materialized scored CTEs
without any inner LIMIT. Equal scope-correct result IDs alone do not pass scoring.

Schema diagnostics keep the original strict comparison separate from the
explicit T0 candidate. The candidate only normalizes observed `key`/`action`
identifier quoting and a CREATE-prefix `IF NOT EXISTS` guard; quoted values,
other identifiers and all constraints remain byte-sensitive. Reopen metadata
retains raw hashes and ignores only `PRAGMA index_list` display sequence/order,
while comparing all schema SQL, columns, FKs, index names and flags. Mutation
tests reject changed defaults, missing indexes and altered uniqueness. These
diagnostics do not implement the production storage-format discriminator.

## Deliberate crash subprocesses and bounds

The bridge is a separate executable exclusively to establish process termination
boundaries. It does not re-enter the application CLI. Each request has a
30-second driver deadline; each bridge lifetime is limited to 180 seconds;
result rows are capped at 10,000 and request lines at 4 MiB. CI supplies an outer
job timeout. This is not a hard Rust future-cancellation claim.

For 999, 1000 and 1001 chunk rows the driver kills the process after an
acknowledged pre-COMMIT, post-COMMIT or post-checkpoint barrier. It verifies
chunks/summaries, both FTS indexes, graph nodes/edges, job state and publication ledger together after a new process
opens the database. Each case then stops cleanly, copies the entire fixture
directory and verifies same-version backup restoration. It never assumes a
single database file is sufficient.

These are nine deterministic process-kill cases, **not arbitrary mid-I/O power
loss, disk-full, or filesystem fault injection**. Mid-OPTIMIZE/COMMIT/checkpoint
interruption and production service recovery are explicitly NOT_RUN until an
appropriate follow-on harness is executed. The original graph recursive CTE
prepared successfully in the bootstrap; the follow-up executes directed,
cyclic and scoped 0–3-hop fixtures. Bounded same-snapshot BFS is a fallback
adaptation if needed and is not implemented by this probe.

## Evidence and failure behavior

Every attempted case reports PASS/FAIL, measured wall time and observed values.
Downstream checks skipped after unmodified DDL/fixture failure are NOT_RUN, never
PASS. SQL errors are local synthetic diagnostics, not application error text.
On assertion failure the driver attempts explicit rollback before independent
checks. A failed schema or authorization invariant blocks default switching.
Keep the report, generated lock, `cargo metadata`, feature tree, Rust version,
runner information and exact tested commit together. See
[`T0_STORAGE_CONTRACT.md`](../../docs/T0_STORAGE_CONTRACT.md).

## Optional bounded scored-CTE cost follow-up

`fts_cost.py` is separate from the historical compatibility gate. It measures
60 unbudgeted and six guarded configurations on new 1k/10k synthetic stores,
with current scoped visibility/authority SQL and a full-index BM25 oracle.
No arguments prints its frozen plan. After building the isolated release binary:

```sh
cargo build --locked --release --manifest-path tools/t0-turso-probe/Cargo.toml
python3 tools/t0-turso-probe/fts_cost.py \
  --binary tools/t0-turso-probe/target/release/origence-t0-turso-probe \
  --output /tmp/t0-fts-cost-new
python3 tools/t0-turso-probe/fts_cost.py --report /tmp/t0-fts-cost-new/report.json
```

CI additionally wraps this entire driver/process group in a 900-second SIGKILL
deadline; the 17-minute step limit is only a runner backstop. The output directory
must be new. The driver preserves partial evidence and
explicit failures/rejections; a 512-row logical guard rejects its 513th result
rather than silently truncating relevance. This is not a hard internal engine
CPU/memory budget. Python wall time includes IPC, serialization, filtering and
fresh verification. Optional bridge `timing: true` returns `engine_elapsed_ms`
for parameter conversion, SDK execution, row draining and JSON-value building;
it excludes connection setup and wire serialization/IPC, and is not pure engine
CPU time. See [the frozen follow-up contract](../../docs/T0_STORAGE_CONTRACT.md#7-bounded-follow-up-frozen-before-native-execution-2026-10-13)
and [the cost report](../../docs/t0-fts-cost-report.md) for scope and actual results.

## Staged single-table FTS (new authorized direction)

The CTE/JOIN cost round ended at `380fedeffad2425c96715994dfdecd4c27e54f6c`.
Do not interpret those cost/diagnostic scripts as the current candidate design.
The new test-only `staged_fts.py` uses a scoped single-table FTS stream, batched
application authority checks in one read snapshot, deterministic refill, explicit
resource rejection and separate fresh final validation. Native results are
NOT_RUN at source preparation; the exact-head result lives in PR #53/issue #52.

```sh
python3 -m unittest discover -s tools/t0-turso-probe -p 'test_*.py' -v
cargo build --locked --manifest-path tools/t0-turso-probe/Cargo.toml --release -j 2
timeout --signal=KILL 900s python3 tools/t0-turso-probe/staged_fts.py \
  --binary tools/t0-turso-probe/target/release/origence-t0-turso-probe \
  --output /tmp/t0-staged-fts-new-output
python3 tools/t0-turso-probe/staged_fts.py --report /tmp/t0-staged-fts-new-output/report.json
```

The output directory must not exist. Run only the frozen 1k/10k matrix and focused
cases; do not add scales, variants or providers after observing a failure. The
external process-group timeout is the hard whole-driver bound. Bridge request/
process deadlines, row/page limits and logical resource rejection are distinct.

See [contract section 8](../../docs/T0_STORAGE_CONTRACT.md#8-staged-single-table-fts-follow-up-authorized-2026-10-13)
and [the staged report](../../docs/t0-staged-fts-report.md). SQLite fabricated-FTS
harness checks do not prove native scoring. Summary AND and full Context route/
identity/generation/ledger/Learning coverage remain outside this raw-chunk slice.
Unchanged ANN jobs may be skipped by exact-input selection; a skip is NOT_RUN,
not a transferred pass from a prior head.
