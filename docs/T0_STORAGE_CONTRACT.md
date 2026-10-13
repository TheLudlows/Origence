# Issue #52 T0: frozen candidate contracts and evidence gates

Baseline: `main@d0521c0935083e71890ab02c100920c2edaa7227`, 2026-10-12.
Scope: isolated compatibility/measurement probes only. The production SQLite,
LanceDB exact-query path, root Cargo manifest/lock, protocols and default backend
remain unchanged. Turso is the agreed long-term authority; this work evaluates
how to adopt it, not the vendor choice. T1–T5 are not authorized by a T0 result.

## 1. Version and execution contract

| Component | Candidate lock | Gate |
| --- | --- | --- |
| Rust | 1.98.0, repo toolchain | Actual rustc version, fmt and warnings-denied Clippy |
| Turso Rust SDK | `=0.8.2`, defaults off, `fts` on | Cargo-generated isolated lock, resolved turso_core/sdk-kit versions, feature graph; dev-only `io_memory_yield` for forced cancellation |
| FTS runtime | `experimental_index_method(true)` | Real physical index plus `QUERY INDEX METHOD fts` EXPLAIN evidence |
| LanceDB | `=0.23.1`, defaults off | Isolated Cargo lock; only APIs verified in this release |
| Data | Synthetic temporary new stores | No live app data, model/provider calls or existing-store rewrites |

Initial CI may bootstrap a missing nested Cargo lock. Such an execution is
provisional until that exact Cargo-generated lock is committed and the final
head passes `--locked`. Root production dependencies are not edited to run T0.
The compatible published SDK's FTS implementation must be tested directly;
stale web documentation is not evidence that it lacks transactions/RYW/merging.

## 2. Retrieval semantics frozen before comparison

1. Lexical input remains the current `parsing::lexical`/Jieba output. Unique,
   nonempty tokens are quoted/escaped and joined with explicit AND. Empty terms
   return no lexical candidates. Raw user input never becomes FTS query syntax.
2. A chunk matches the summary branch when all terms occur in the **union of
   all summaries for that same scoped chunk**, even across separate summaries.
   Requiring all terms in one summary is a regression. The probe intersects
   per-term indexed full chunk-key sets. Final summary score aggregation remains
   a T3 adapter choice and must be versioned/tested before default activation.
3. FTS scores are higher-is-better BM25. The experimental table-wide index uses
   global index statistics; an SQL scope filter does not make IDF scoped. This
   is recorded as the **T0 candidate**, not a chosen production relevance policy.
   Global versus scoped statistics require a fixed-query, cross-scope corpus
   perturbation measurement before the final ranking policy is selected.
4. Vector distance stays explicit **L2** and public score retains
   `1 / (1 + distance)`. No embedding normalization/model change is folded into
   ANN. PQ must refine against full vectors for exact-distance score semantics;
   refinement cannot recover candidates the approximate search never retrieved.
5. Authorization, exact identity, type/route and authoritative eligible
   source/version/generation bounds constrain candidate competition before
   branch top-k. A fresh authoritative read rechecks permission and visibility
   after native I/O. No fixed over-fetch multiplier proves arbitrary-filter
   recall. Zero unauthorized/stale evidence is mandatory, independent of 0.95
   recall tolerance.
6. Search's current branch cap is 100; requested final K is 1–100; RRF constant
   stays 60. Each branch deduplicates full scoped chunk identity before assigning
   ranks, including duplicates across embedding generations. Production Search
   lacks that generation dedup today; this is a later adapter regression target,
   not silently fixed or claimed completed here.
7. Context Memory/Knowledge/Learning have **independent candidate pools**, at
   most 100 per route, and use raw chunks. Identity narrows only Memory. No
   summary/graph substitution, shared-global-top-k-then-labeling, or Learning
   evidence detached from conditions/support is permitted. `score_kind` must
   distinguish BM25, exact L2-derived score and RRF rather than retain the old
   lexical overlap label when algorithms change.
8. Public graph traversal is directed outgoing reachability, maximum 3 hops,
   excluding the seed with deterministic dedup/order. Hybrid's graph expansion
   is a different, one-hop bidirectional behavior. Same-snapshot batched BFS is
   the fallback adaptation if the actual released engine cannot preserve the
   recursive query's behavior. Locked 0.8.2 diagnostics passed the actual
   directed/cyclic/scoped 0–3-hop query, so no unsupported-query claim or BFS
   rewrite is justified by this result. One SQL call per node is disallowed.
   Resource exhaustion must be explicit, never silent "complete" truncation.
9. Existing retrieval/context policy markers and historical evaluation records
   stay unchanged during T0. A new ranking/index strategy must receive a new
   policy marker when integrated; no old report is reinterpreted as BM25/ANN.

## 3. Candidate filtering and ANN measurement design

The safe correctness baseline retains full authoritative eligibility plus
512-ID batches, one **merged AND** Lance predicate. In 0.23.1 repeated
`only_if()` replaces the previous filter. The measured candidate strategy pushes
immutable tenant/workspace/source/version/generation and explicit synthetic
eligibility into prefiltering with one native query per bucket. It does not
claim production identity/type/Learning rules are already in the Lance schema.

T0 records native-query counts, enumerated IDs/bytes/time, query execution,
returned candidate count and final verification separately. The 512-ID path is
measured as a correctness/cost baseline; adding ANN inside it is not proof that
the end-to-end path scales. Context's route enumeration/intersection and
`src/context_retrieval.rs` are explicitly part of the later production design.

Native ANN requires index metadata, coverage/stats and an EXPLAIN plan. Ordinary
ANN must include fresh unindexed rows; `fast_search` is forbidden. Index refresh
uses **OptimizeAction::Index**, separately from data-file **Compact**. Never use
**All** (which also prunes old versions) or **Prune** in these probes.

Turso FTS also requires score provenance, not only an index-name plan marker.
The tested raw visibility JOIN uses the FTS method for filtering but projects
scalar zero scores. Same-snapshot scored CTE candidates passed correct positive
scores and visibility-before-top-k; use the same bind slot for score/match, no
early inner LIMIT, and retain the failed raw JOIN as regression evidence. The
CTE's all-match materialization cost and production ranking remain unaccepted.

Paired exact and ANN runs use the same compacted layout, frozen table snapshot,
vectors, L2 metric, eligibility filters, tie rule and K. The exploratory first
run is 10k synthetic vectors at 128 dimensions, with production dimensions and
100k/1M scale explicitly later work, not launched automatically. IVF_FLAT is the
initial correctness/index-path candidate; PQ/HNSW comparison remains bounded by
documented training/runtime constraints and cannot be labeled tested if omitted.

The first native 32-of-64 IVF_FLAT run failed selective-bucket recall. That result
is retained. A separate explicit 64-of-64 run diagnoses exhaustive partition
coverage on the same canonical fixture; it rebuilds its own index and is not a
same-physical-index parameter comparison or an ANN speedup claim. Neither the
default candidate nor the frozen recall threshold is silently changed.

Frozen acceptance target: per declared bucket Recall@10 and Recall@100 >= 0.95,
denominator `min(K, eligible unique chunks)`, exact fallback = 1.0; empty buckets
are reported separately. Cross-scope, withdrawn, unpublished, invalid Learning
and revoked-key evidence returned = **0**. The synthetic index harness covers
only the eligibility states it explicitly models; real service authorization,
Learning closure and concurrent revocation are NOT_RUN until integrated tests.
Small exact-fallback buckets (0/1/<K), tenant skew, selectivity, history, fresh
writes and route pools are reported separately. Record per-query and poor-tail
recall, not only a pooled mean. No hardware-specific P95/P99 SLO or production
speedup is accepted until representative hardware/workload is fixed; a small
timing sample is not a P99 claim.

## 4. Database creation/open contract

Future adapter creation is permitted only in a new empty storage destination.
An existing SQLite/unknown/legacy directory must be explicitly rejected without
repair, deletion or conversion. A SQLite-compatible file header alone does not
identify a supported Turso store.

Candidate discrimination: explicit Origence storage-format/engine/schema
manifest plus exact schema invariants, supported pinned engine/format version,
and an initialization-complete marker coordinated with successful creation.
Supported stores reopen normally; markers must never be guessed or added to an
unrecognized nonempty directory. Interrupted initialization requires a defined
fail-closed recovery state. T0 freezes this rule; the production implementation
and its interruption tests are NOT_RUN, not smuggled into the generic probe.

The executable verifies current actual DDL roundtrips and same-version process
reopen. Backup copies the whole stopped directory and records its file set;
logs/FTS/sidecars are not assumed unnecessary. No migrations, upgrades or
cross-engine restore support are part of current acceptance.

## 5. Executable matrix and honest boundaries

| Area | T0 probe | Follow-on or currently NOT_RUN |
| --- | --- | --- |
| Real DDL | All 6 source schemas, 47 statements, no rewrite | Production Turso initialization adapter |
| Real SQL | 188 directly executable literals/assembled projections prepared; job enqueue/RETURNING, publication JSON, visibility queries executed | Every dynamically formatted SQL branch and full public-service regressions |
| Constraints | UUID BLOB/time/JSON, composite PK/FK, deferred Learning FK late-parent/missing-parent COMMIT, partial unique, triggers, UPSERT, graph cascade | Exhaustive Learning support/permission failure matrix |
| Transactions | BEGIN IMMEDIATE, commit/rollback, SDK drop/reuse, RETURNING drop, once-polled cancellation, read snapshot | Every async yield point, multi-process/MVCC concurrency |
| FTS | Real chunk/summary index, EXPLAIN, AND, summary union, Unicode/update/delete/optimize/rollback/scoped join | Representative corpus relevance, global-vs-scoped default, rebuild interruption |
| Crash/restore | 999/1000/1001 rows at pre-commit/post-commit/post-checkpoint process-kill barriers; main+FTS+job+ledger; stopped whole-directory restore | Arbitrary mid-I/O power loss, disk-full, mid-OPTIMIZE/checkpoint/commit |
| Graph | Current DDL/cascade/atomic rollback and actual recursive CTE direction/hops/scope/cycle behavior | Batched BFS only if needed; full public/hybrid graph-service integration |
| ANN | Separate physical IVF index and paired filtered exact baseline | Production integration and final scale/hardware SLO |

PASS means the named executable assertion ran successfully. FAIL means it ran
and did not meet the frozen expectation. NOT_RUN means no such evidence exists.
Source review and successful SQLite fixture tests do not promote Turso/ANN to
PASS. Expected direct SQL incompatibilities remain failures in compatibility
evidence, with an explicit bounded adaptation decision; they are not hidden.

## 6. T0 exit and next authority

Before T1: commit Cargo-generated locks; repeat the final exact-head lint/tests;
review all failures and missing durability/authorization/FTS evidence; select or
explicitly retain unresolved candidate-ranking choices; independently review
probe assumptions and the measured filtering strategy. An unresolved safety or
persistence blocker stops default switching. Large scale and real-model work
require their own bounded plan. Passing this probe is not T1 implementation,
production acceptance, merge authority or permission to enable a default.

## 7. Bounded follow-up frozen before native execution (2026-10-13)

This follow-up is still T0. Historical raw-schema, raw-JOIN-score and fixed-probe
recall failures remain in the reports and executable compatibility gates. It
cannot authorize a production adapter, default switch, merge, or new vendor
selection. The local editing environment has no Rust/Cargo; native results must
come from the committed exact-head, locked CI run. All data remains synthetic.

### 7.1 One filter-aware ANN policy, two new index builds

The only candidate routing rule is metadata-based: use IVF_FLAT with fixed
32-of-64 probes when the authorized eligible count is at least 4096 **and** at
least 40% of snapshot rows; otherwise use exact bypass. Empty eligibility makes
zero retrieval-native calls. Never choose the path from returned K, vectors,
measured recall, or the exact oracle. The 40% clause is redundant at this 10k
scale; the rule is calibrated from earlier evidence, not a cross-scale policy.

Each of two fresh 10k/128D index builds uses the canonical fixture and records
training/index metadata. The released builder exposes no fixed training seed;
these are repeated builds, not claimed statistically independent trials. Add
metadata-only rotated/subsampled sets around 100/101/512/513/2048/4095/4096/4097
eligible IDs alongside dense/history/empty and existing safety buckets. Use eight
held-out queries per bucket (four new near-row offsets and four off-row queries),
indexed/fresh-before-refresh/fresh-after-refresh/reopen snapshots, and balanced
four AB/four BA timing order. Each build has 72 bucket/snapshot cases and 576
paired queries. No 100k or 1M run is scheduled in this pass. The isolated release-build
steps have a separate 45-minute CI limit; this does not alter the main
application workflow or native measurement deadlines.

Each arm independently measures the complete eligibility-enumeration,
authority-map/set, route/filter/query construction, native execution/decode/sort,
and fresh-verification wall time. The exact comparator has the same empty fast
path. Retrieval uses one merged authorized-ID filter and one native query per
nonempty arm; enumeration is retained, including the 512-ID boundary, rather than
silently optimized away. Report actual retrieval, setup, plan, and audit calls
separately. The independent exact oracle/audit runs after measured pairs; query
plans for all eight query vectors run after their bucket (1,104 recorded explain
requests per build, with deduplicated plan text and per-slot hash references).
This is prepared/warm synthetic work, not cold I/O.

Quality is a gate on **every** nonempty pair: Recall@10 and Recall@100 >= 0.95,
zero source/visibility/duplicate/distance violations; exact-selected paths must
match exact completely. Selected ANN must prove an actual fixed-32-probe plan.
An exploratory benefit decision separately requires, in **each** fresh build:
aggregate selected/exact full-wall ratio <= 0.90, ANN-stratum median paired ratio
<= 0.90, and each bucket median ratio <= 1.10. These are declared heuristics for
this artificial mix, not a production latency SLO. Either quality or benefit
failure ends tuning in this round: recommend exact for the tested mix and report
why. Full-partition recall is not an ANN speedup. Each native build is limited to
900 seconds; an incomplete run is not a pass.

### 7.2 Correct scored-CTE FTS cost and explicit budget rejection

Use the proven MATERIALIZED scored CTE with identical query parameters for
`fts_score` and `fts_match`, no early scored-row LIMIT, current Origence DDL and
visibility/authority SQL. Preserve full scoped keys and asset/version/source
metadata. The independent ranking oracle enumerates full-index native BM25 and
then applies authoritative eligibility and deterministic tie ordering outside
the candidate CTE. Statistics are global to the physical FTS index; this does
not select the future scoped/global product default.

Freeze 60 unbudgeted configurations: 1k/10k corpora x three approximate lexical
match densities (1%, 10%, all) x five eligible/source buckets (1, 8, 64, 512,
dense) x K=10/100, with two AB/BA pairs each. Add six budget configurations, one
per corpus/query at dense K=100, with two rounds each: 66 configurations total.
Ninety percent of corpus rows are target-scope eligible; adversarial competitors
include foreign tenant/workspace, withdrawn/deleted/stale/unpublished state and
same-ID wrong-scope collisions. Only modeled states count as tested coverage.

Each reopened phase first records a setup-only SELECT 1 and the implicit
foreign-key PRAGMA; it does not prewarm FTS. Connection setup is outside both
measured prepared-query arms. Native-call counts distinguish bridge requests
from implicit setup SQL statements. Candidate end-to-end wall time includes
bridge IPC, BEGIN, current API-key
authority, optional guard, scored CTE plus visibility, COMMIT, fresh transaction
reauthorization/visibility verification, and decoding. Exact uses its own
eligible-ID query plus full-index direct BM25 and Python filter/sort, with the
same authority and fresh-check stages. Log actual query counts. A separate
optional `engine_elapsed_ms` span covers bridge parameter conversion, SDK call,
row drain, and JSON-value construction; it excludes connection setup, request
parse, serialization and IPC, and is not pure engine CPU time. CI uses a release
bridge for these costs; no production Rust end-to-end speedup is inferred.

A logical 512-row preflight budget reads at most 513 matches. More than 512 yields
explicit `RESOURCE_EXHAUSTED`, not empty success or an accepted low-recall answer;
ranking/recall are NOT_RUN for that rejected request and coverage/rejections are
reported. LIMIT bounds materialized/output rows, not hidden engine CPU or memory.
Unbudgeted all-match materialization remains measured separately. Oracle/plan
work occurs outside measured spans, after measured pairs. Per-request and
per-process deadlines remain bounded. CI places the whole Python driver and
its child process group under an external 900-second SIGKILL cap, covering
startup, setup and report validation as well as measured SQL. The driver also
checks its deadline and saves incremental evidence; a hard stop retains the last
complete report, with missing work NOT_RUN. Expensive failing work is not
retried unboundedly. Quality, source safety, ranking and explicit-budget behavior
are gates. Latencies are exploratory because representative hardware and SLOs
are not yet fixed.

### 7.3 Single post-hoc JOIN-order diagnostic, then stop

The initial native follow-up at `fa19309ff6a0f8623f1670b0a3a34a686073ad85`
completed all 66 FTS configurations with correct ranking/budget behavior, but
10k/all-match/dense/K100 took 14.678–14.701 seconds in the candidate pipeline,
versus 0.386–0.399 seconds in its independent full-index BM25/eligibility
comparator. `SCAN scored AS s` appears after visibility joins. This supports a
JOIN-shape cost hypothesis, not a proven optimizer root cause or a universal
FTS slowdown; small-source cases behave differently.

After observing that result, authorize **one explicitly post-hoc point only**:
repeat 10k/all/dense/K100 using the original CTE and a scored-first CROSS JOIN
candidate in two paired orders (current/forced, then forced/current). Retain
actual plans and original full scoped BM25/metadata ranking, authority and fresh
verification. Collect oracle/plan work after measured paths. The new report is
separate; the original 66 configurations, 2,226 measured bridge requests, initial
cost evidence and quality gates are unchanged. Syntax alone cannot establish
the physical order; report the observed plans.

Use a fresh synthetic store, a 180-second diagnostic native-work SIGALRM
deadline (bounded cleanup/verification may follow), and reject a completed
report whose measured driver duration exceeds 180 seconds. One combined
900-second CI SIGKILL process-group cap around the original cost driver plus
this diagnostic is the hard whole-process guarantee. Report extra calls separately. No other shapes, thresholds,
scales or optimizer tuning may be tried in this round. Whether this candidate
improves or fails, conclude the bounded round and state the engineering decision.
A favorable single point is not cross-bucket acceptance or production SQL.
