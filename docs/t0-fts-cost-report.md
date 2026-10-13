# T0 scored-CTE FTS cost follow-up

Date: 2026-10-13. Production baseline:
`d0521c0935083e71890ab02c100920c2edaa7227`.

**Native cost/ranking matrix: NOT_RUN.** This document freezes the bounded
experiment and describes its prepared harness. Local Python/SQLite fixture
checks are not Turso evidence. Native acceptance requires the exact-head,
release-mode, committed-lock CI artifact and independent report verification.
The existing raw visibility-JOIN zero-score/wrong-winner failures and strict
schema-text failures in [the compatibility report](t0-turso-report.md) remain
unchanged. This follow-up does not choose the production relevance policy.

## Frozen matrix and stopping rule

`tools/t0-turso-probe/fts_cost.py` creates two new temporary databases, using all
six original schema files and all 47 DDL statements without rewrites. It tests
only the chunk FTS branch of the pinned Turso/core/SDK-kit **0.8.2** release.

- Corpus sizes: exactly **1,000 and 10,000** synthetic chunks. No 100k/1M option.
- Queries: `lowtoken`, `mediumtoken`, and `commontoken`. Terms are passed through
  the existing literal-token compiler. Within each source group, the first two
  occur every 100th/every 10th ordinal; the common term occurs in every row.
- Actual lexical counts, fixed before running: **20 / 107 / 1,000** at 1k and
  **108 / 1,008 / 10,000** at 10k. These are intentionally reported as actual
  fractions, not silently rounded to exact 1%/10% densities.
- Eligible source buckets: **1 / 8 / 64 / 512** chunks and a **dense** bucket of
  **900 / 9,000** target-scope chunks. The first four are distinct real source
  events/assets; dense includes all four plus the eligible remainder. Thus the
  512-row and dense buckets fit exactly in the 1k corpus without adding rows.
- Unbudgeted matrix: 2 corpus sizes × 3 queries × 5 buckets × branch K 10/100
  = **60 configurations**.
- Guard matrix: one **dense/K100** configuration for each corpus/query = **6**
  additional configurations. The guard depends on global lexical match count,
  not scope or final K; duplicating it in every source bucket adds no coverage.
- Each of the **66 configurations** runs twice, candidate/exact then
  exact/candidate: **132 ordered pairs / 264 measured paths**.
- One frozen MATERIALIZED-CTE candidate. No retries that increase budgets, change
  the fixture, tune ranking, or turn an observed failure into a passing run.
  A failed process/phase retains its partial observations; dependent unexecuted
  cases stay NOT_RUN. Later independent phases may reopen the same synthetic
  store once. Stop after this bounded pass/fail/reject evidence.

Ten percent of each corpus consists of stronger lexical competitors that must
not escape current authority/visibility: another tenant, another workspace,
withdrawn version root, deleted asset, stale version, unpublished asset, and
withdrawn local-S2 supporting source. Wrong-scope rows deliberately reuse an
eligible chunk ID. Every returned hit carries tenant, workspace, chunk ID,
asset ID, version and root source ID, so ID-only verification cannot conceal
these collisions. Repeated term frequencies/document lengths create score ties;
the tie order is ascending full scoped chunk identity.

## Candidate and independent exact path

The candidate reuses `?1` for both `fts_score` and `fts_match`. Its
`WITH scored AS MATERIALIZED (...)` selects every lexical match and native BM25
score from the table-wide physical index. There is **no inner LIMIT**. It then
joins scored rows by full scoped key into the actual `CURRENT_CHUNK_ROWS` and
`CHUNK_SUPPORT_ACTIVE` SQL from `src/storage/sqlite/visibility.rs`, applies the
optional real source-event filter, orders by descending score/full scoped key,
and only then applies branch K 10 or 100.

The exact arm does not use that CTE as its oracle. It separately enumerates
eligible rows with the authoritative visibility/source SQL, drains standalone
full-index native BM25 rows, independently intersects full scoped identities,
and sorts/truncates in Python. Its enumeration, materialization and sorting
cost are included. A separate post-measurement audit reads standalone native
scores and authority rows again, verifies them against the deterministic fixture,
and independently reconstructs expected IDs, metadata, scores and ranks.
There is no lexical-overlap fallback labeled BM25.

Both measured arms execute the real `oc_api_keys` token-hash/unrevoked lookup,
check the exact target scope, retrieve, commit, then start a fresh read transaction
and recheck both key authority and actual visibility of the returned full metadata.
This is an executed synthetic SQL path, **not** a claim that public-service
permissions, key ownership, identity/route filtering, full Learning closure, or
concurrent revocation integration have been completed. No other writer changes
the private fixture during a paired phase; both arms see the same frozen data
and global index statistics.

## Read budget is separate from top K

The guarded candidate first issues the indexed match query with `LIMIT 513`.
A result of 513 means **RESOURCE_EXHAUSTED** for the frozen 512-row logical read
budget. It does not run the candidate CTE, does not return an empty successful
answer, and has ranking **NOT_RUN** and recall **null**. The exact arm still runs
and records its expected answer and cost.

The guard accepts 20/107/108-match configurations and rejects
1,000/1,008/10,000-match configurations. Consequently, if all executable gates
succeed, expected matrix coverage is **63 PASS and 3 RESOURCE_EXHAUSTED**, with
zero FAIL/NOT_RUN matrix cells. These are expected counts, not native results.
Unit tests additionally check the exact **512 accepted / 513 rejected** boundary.

`LIMIT 513` bounds guard result rows and the logical enumeration decision. It is
**not a hard engine CPU or memory budget**, and does not establish how much
internal index work the engine performs before returning those rows. Guard wall
cost and actual bridge calls are retained. The separate unbudgeted 10k common-term
cases still execute the all-match CTE, exposing the cost of logically scoring
and materializing all 10k matches before visibility/top K. Their 10k logical input
count comes from independent native match evidence; it is not a measurement of
engine peak memory or internal allocation count.

## Measurement boundary and query accounting

A fresh native bridge is opened for each corpus/query phase. Before any measured
pair, one recorded `SELECT 1` initializes its connection and implicit
`PRAGMA foreign_keys=ON`. This is setup, outside both arms, and performs no FTS
query. Fixture insertion/index creation is separately timed. There is no forced
FTS warmup or discarded first retrieval.

Reference-score reads, eligibility audits, physical-index metadata and every
EXPLAIN are collected **after all measured pairs in that phase**. Both AB/BA
orders are retained. Repeated queries naturally warm the same file/index/OS
caches; this is not a controlled cold-cache experiment. Neither balanced order
nor reopening proves production cache behavior.

Measured bridge request sequences:

| Path | Request sequence | Requests |
| --- | --- | ---: |
| Unbudgeted candidate | BEGIN, authority, CTE, COMMIT, fresh BEGIN, authority, visibility, COMMIT | 8 |
| Exact | BEGIN, authority, eligible enumeration, direct full-index scores, COMMIT, fresh BEGIN, authority, visibility, COMMIT | 9 |
| Guard accepted | Candidate sequence with preflight guard before CTE | 9 |
| Guard rejected | BEGIN, authority, guard, COMMIT | 4 |

For a fully executed frozen matrix, this is **2,226 measured bridge requests**:
**1,182 queries + 1,044 transaction execute requests**. The six phase-setup
requests and six implicit connection PRAGMAs, fixture/schema/index work, and
post-pair reference/EXPLAIN/audit requests are outside that measured total and
reported separately. `native_calls` means bridge requests, not an assertion that
one Rust SDK call performs only one internal engine operation.

Every measured path records:

- Python wall time from before BEGIN through final COMMIT, including authority,
  eligibility/scoring, Python selection, fresh verification, IPC, bridge
  serialization and measurement bookkeeping. This is **not production Rust
  end-to-end latency**.
- Each actual bridge request's label/type, result row count, exact
  wire-request bytes, re-encoded response JSON bytes (not an exact wire-response
  measurement), wall duration, and `engine_elapsed_ms`.
- The latter is the release bridge's request span around parameter conversion,
  SDK execution/row draining and JSON Value construction. It excludes input-line
  parsing, connection initialization/foreign-keys setup, output serialization,
  wire writing and IPC. It is **not pure engine CPU time**.
- Exact-path eligible enumeration size, full-index score-row count and Python
  selection duration; actual guard rows; raw result metadata/scores; fresh
  verification metadata; and all request counts.

The report verifier independently recomputes expected ranks, score equality
(absolute tolerance 1e-9), identity/metadata/visibility, outcome labels, call
sequences/counts, actual match density and complete matrix coverage. Every
candidate/exact/direct/guard plan must contain native `QUERY INDEX METHOD fts`
evidence, with the physical index metadata also retained. Successful execution
alone, an index name, or a faster timing is insufficient.

No latency threshold, speedup gate, P95/P99 claim or production resource SLO is
inferred from two observations per configuration. Costs are descriptive evidence
for the next design decision.

## Resource boundaries and failure reporting

Existing bridge bounds remain: 30 seconds per request, 180 seconds per process,
10,000 output rows, and 4 MiB input. The driver additionally checks monotonic time
between operations and uses main-thread Unix SIGALRM to interrupt startup/setup
as well as query waits at 900 seconds. It kills an active child before timeout
cleanup; constructor failure uses the existing bridge's child cleanup. Native
execution fails closed on platforms without the Unix deadline mechanism.

The CI command separately uses `timeout --signal=KILL 900s`, covering the entire
Python/native process group, including startup and report postprocessing. The
17-minute workflow step is a secondary backstop. Cleanup after a standalone
internal timeout may take bounded additional time; the external process-group
cap is the hard whole-process guarantee. A completed report exceeding 900 seconds
is rejected. No RAM hard cap is claimed.

All 66 cells begin as NOT_RUN. Reports are atomically saved after measured arms
and phases. Native errors, request/process/deadline timeouts, failed audits and
missing evidence are failures/incomplete coverage; none is RESOURCE_EXHAUSTED
unless the successful guard actually returned its 513th sentinel row. Prior
atomic reports survive external termination and cannot pass completeness checks.
Bounded bridge stderr is retained, and partially collected audit rows remain
available when an audit assertion fails. Temporary data is private and fresh;
no existing store is deleted, converted, opened, or repaired.

## Reproduce and verify

Use the existing committed isolated lock and repository Rust 1.98.0. Build the
same bridge in release mode; preserve its exact-head build/formatter/Clippy logs,
runner information and binary/source/manifest/lock hashes beside the report.
The binary hash alone does not prove which source produced it.

```sh
python3 tools/t0-turso-probe/fts_cost.py
python3 -m unittest discover -s tools/t0-turso-probe -p 'test_*.py' -v
cargo build --locked --release --manifest-path tools/t0-turso-probe/Cargo.toml -j 2
timeout --signal=KILL 900s python3 tools/t0-turso-probe/fts_cost.py \
  --binary tools/t0-turso-probe/target/release/origence-t0-turso-probe \
  --output "$RUNNER_TEMP/t0-turso/fts-cost"
python3 tools/t0-turso-probe/fts_cost.py \
  --report "$RUNNER_TEMP/t0-turso/fts-cost/report.json"
```

No-argument invocation prints the frozen plan without building or executing
native code. An existing output directory is rejected. A standalone saved report
can be reverified without running the engine. Mutation tests reject changed
ranks/scores, wrong-scope same-ID hits, altered asset/version/source metadata,
missing fresh verification, omitted cases/phases/trials, missing native timings,
incorrect query counts, fake empty-success budget rejections and invalid plans.
SQLite and fabricated-score unit cases prove only harness invariants.

## Remaining boundaries

This result will not replace the failed raw-JOIN regression or accept production
ranking. Global-versus-scoped relevance selection, cross-scope corpus
perturbation, summary-score aggregation, representative corpora/hardware,
production public-service paths and a production adapter remain NOT_RUN here.
No Cargo dependency, production schema/backend/default, old data, provider,
paid compute, PR merge or T1–T5 work is authorized by this probe.
