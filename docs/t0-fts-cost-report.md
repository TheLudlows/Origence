# T0 scored-CTE FTS cost follow-up

Date: 2026-10-13. Production baseline:
`d0521c0935083e71890ab02c100920c2edaa7227`.

**INITIAL native evidence is available: 63 PASS / 3 RESOURCE_EXHAUSTED across
66 configurations.** Ranking and visibility gates passed, while the current
scored-CTE JOIN shape showed a material dense/common-term cost problem. This is
not final production/adoption acceptance. A final exact-head locked rerun is
still required after the separate ANN formatter import; one explicitly bounded
post-hoc join-order diagnostic is authorized before this round concludes.

The existing raw visibility-JOIN zero-score/wrong-winner failures and strict
schema-text failures in [the compatibility report](t0-turso-report.md) remain
unchanged. This follow-up does not choose the production relevance policy.

## Initial native evidence: `fa19309f`

Exact source head `fa19309ff6a0f8623f1670b0a3a34a686073ad85`,
[run 38333646435](https://github.com/TheLudlows/Origence/actions/runs/38333646435),
[job 115088768848](https://github.com/TheLudlows/Origence/actions/runs/38333646435/job/115088768848),
[artifact 11784505132](https://github.com/TheLudlows/Origence/actions/runs/38333646435/artifacts/11784505132).
The release build and warnings-denied Clippy passed with the committed lock,
Rust 1.98.0, and an empty Turso rustfmt patch. The artifact archive SHA-256 is
`6141fb6a5211990a55647f288014edc3efcf23d19d575a365e30c830e99cdf70`;
the full FTS report SHA-256 is
`b249c41b7efbc640053593e05cdc0a784490d4f9b64a4d69be29eb4fd9b11b7d`.

The [compact machine-readable snapshot](evidence/2026-10-13-t0-fts-cost-fa19309f.json)
retains all 66 configurations' raw paired wall/component durations, request
counts, outcomes, provenance, selected plans and raw-result digests. The linked
artifact preserves the full native score/authority/result rows and all plans;
the compact derivative is not a replacement input for the full report verifier.

Independent source/report audit found:

- **63 PASS, 3 RESOURCE_EXHAUSTED, 0 FAIL, 0 NOT_RUN** matrix configurations.
  The three rejected configurations retain ranking **NOT_RUN**, not recall PASS.
- **132 pairs / 264 paths**: 258 successfully ranked paths and six individual
  guard rejections. All 6,040 returned ranked hits matched the independently
  reconstructed native BM25 oracle, with **zero absolute score difference** and
  zero wrong-scope/stale/invalid rows. Full metadata and fresh verification passed.
- All 13 recorded source hashes match the reviewed sources. The actual native
  index/plans, post-pair audits and both measured orders are present.
- **2,226 measured bridge requests** (1,182 queries / 1,044 transaction executes),
  plus 114 post-pair audit requests, six explicit phase-connection setup requests
  and six implicit phase-connection PRAGMAs. Total driver wall time was
  **125.976650 seconds**.

### Material cost finding, without a new acceptance threshold

The raw two-round timings below are Python pipeline wall milliseconds, including
both authority checks and fresh visibility verification. They are descriptive
observations on the same synthetic 10k/common-term/K100 point at each source size;
no latency SLO was frozen or applied after seeing these results.

| Eligible rows | Current CTE, rounds 1 / 2 | Exact oracle, rounds 1 / 2 |
| ---: | ---: | ---: |
| 1 | 18.206 / 18.312 | 120.112 / 119.409 |
| 64 | 134.551 / 136.893 | 134.574 / 134.204 |
| 512 | 860.586 / 865.807 | 176.121 / 155.576 |
| 9,000 (dense) | **14,701.258 / 14,677.994** | **398.634 / 385.953** |

At 10k/all/dense/K100, the candidate CTE request itself took
14,672.965 / 14,649.413 ms wall, with bridge request spans of
14,672.223 / 14,648.636 ms. Standalone direct BM25 in the paired exact arm took
64.813 / 67.167 ms wall, with bridge request spans of 40.757 / 42.614 ms. The
candidate's long duration is therefore inside the native request span rather
than primarily Python/IPC overhead. That span includes SDK execution, row drain
and JSON Value construction; it is not a pure engine CPU measurement.

The actual plan first uses the FTS index, then indexed searches of `c`, `v`,
`e`, and `a`, followed by **`SCAN scored AS s`**, local-support checks and sorting.
The same 10k lexical-match input costs about 18 ms for source_1, 861–866 ms for
source_512, and 14.7 seconds for dense eligibility. At dense/K100, reducing lexical
matches to 1,008 reduces the candidate pipeline to 1,556–1,557 ms; with 108 matches
it is about 240 ms. These observations are **consistent with repeated scored-row
scanning in the current JOIN shape**. They do not prove a particular optimizer
root cause or establish an asymptotic bound. The fast direct-score request and
small-source case also prevent treating this as evidence that FTS is universally
slow.

Guarded 1k/all, 10k/medium and 10k/all correctly rejected at 513 returned rows.
Their two-round rejection-pipeline costs were 4.916/4.752, 5.533/5.399 and
5.700/5.481 ms respectively. These are observed guard costs, not hard CPU/memory
bounds or permission to silently return fewer answers.

**Bounded decision:** preserve the successful rank/visibility evidence, but do
not accept this current JOIN shape as a scalable production path. The frozen
matrix was not retuned and no latency FAIL threshold was invented. One separate
post-hoc comparison is authorized at **10k/all/dense/K100 only**: current
MATERIALIZED CTE versus a forced-scored-first CROSS JOIN shape, with the same
full-index/scoped-metadata oracle and two balanced orders. It has its own evidence
and does not replace this initial 14.7-second result or the 66-configuration
report. No other shape, scale or threshold tuning is authorized; stop after that
single comparison and final exact-head verification, regardless of improvement.

### Preparation history

Before this native run, the local authoring environment had no Rust/Cargo, and
native matrix evidence was NOT_RUN. The 25 Python/SQLite/mutation tests and
source review passed locally; those preparation checks were never substituted
for Turso results. The design below was frozen before initial native execution.

## Authorized one-point post-hoc diagnostic

Status: **NOT_RUN** until the final exact-head CI artifact is available. This is
separate from the initial matrix. `join_diagnostic.py` accepts no corpus, query,
K, shape-list or tuning options: it always uses a new synthetic 10k store and the
single all-match/dense/K100 point. Four measured paths run in two paired orders:
current MATERIALIZED CTE then forced-scored-first CROSS JOIN, followed by the
reverse order. Both preserve all-match materialization, bind-slot identity, the
actual visibility/local-support predicates, full scoped identity, deterministic
ranking and the same fresh authority/metadata verification. The diagnostic fixes
all four relational edges with CROSS JOIN, imposing `s → c → a → v → e`; an
improvement would establish a query-shape effect, not isolate moving `scored`
alone as the sole cause.

After those four paths, a standalone native score read plus independent actual
dense-eligibility read reconstructs the oracle. Physical-index metadata and
actual direct/current/forced plans are retained; the forced plan must really
visit scored input before `c`. No early inner limit or scoring fallback is added.
Raw hits, fresh metadata, each bridge request and both timing layers are kept.

Expected extra measured calls are **32** (16 query / 16 transaction execute),
plus six post-pair audit/plan requests and one phase connection-setup request.
Schema/index/fixture requests and both processes' implicit connection PRAGMAs
are separately counted. These costs are not included in, or substituted for,
the frozen original matrix's 2,226 requests. The diagnostic has an internal
180-second native-work deadline, with bounded cleanup and report verification
afterward; a completed report whose recorded driver wall exceeds 180 seconds is
rejected. This internal deadline is not a 180-second whole-process hard cap. CI
wraps both the original driver and this diagnostic in one 900-second external
process-group hard cap. Errors/timeout/missing paths remain
FAIL/NOT_RUN, and no alternative shape or larger run follows a failure.

The original measured helper gains an optional fixed-statement injection and
deadline error wording can reflect the diagnostic's 180-second bound; its default
candidate SQL, 900-second bound and 66-case matrix remain unchanged. This is a narrow
post-hoc query-shape diagnosis, not held-out validation or production acceptance.
Regardless of its result, this round stops after final evidence verification.

```sh
python3 tools/t0-turso-probe/join_diagnostic.py
python3 tools/t0-turso-probe/join_diagnostic.py \
  --binary tools/t0-turso-probe/target/release/origence-t0-turso-probe \
  --output "$RUNNER_TEMP/t0-turso/fts-join-diagnostic"
python3 tools/t0-turso-probe/join_diagnostic.py \
  --report "$RUNNER_TEMP/t0-turso/fts-join-diagnostic/report.json"
```

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
zero FAIL/NOT_RUN matrix cells. These were frozen expected counts; the initial
native run above observed exactly those counts without changing the policy.
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
