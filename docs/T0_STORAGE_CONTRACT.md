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
| Turso Rust SDK | `=0.8.2`, defaults off, `fts` on | Cargo-generated isolated lock, resolved turso_core/sdk-kit versions, feature graph |
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
   recursive query's behavior. The bootstrap prepared that query successfully;
   directed/cyclic/scoped 0–3-hop execution is measured separately rather than
   declaring it unsupported from older documentation. One SQL call per node is disallowed.
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
