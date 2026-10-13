# T0 LanceDB 0.40.0: bounded chunk FTS and ANN probe

Status: source preparation; native evidence **NOT_RUN**. This report must not be
read as adoption or as a passing replacement of any prior probe.

## Scope frozen before native execution

- Isolated `tools/t0-ann-probe` now targets Rust `lancedb =0.40.0`, Lance 13.0.0,
  Arrow array/schema `=58.3.0`, and the repository's Rust 1.98.0. The production
  manifest, production lock and backend are unchanged. Cargo must generate the
  isolated resolved lock; a provisional runner lock is not final acceptance.
- Chunk FTS and vectors are the new derived-index candidate. Turso remains the
  agreed authority for business text, scope/permissions, versions, provenance
  and graph data. Original files remain in Blob storage. No real provider,
  private corpus, existing database migration, or destructive cleanup is used.
- Summary placement has not been decided in this slice. The existing same-chunk
  union-of-summaries AND contract is preserved, and its Lance implementation is
  **NOT_RUN**. Complete Context route/identity/Learning and production cross-store
  publication/recovery remain **NOT_RUN**.
- Turso FTS 0.8.2 failure at `0596202` is retained as historical evidence. It is
  not a candidate FTS serving path in this slice; unchanged Turso tests are
  skipped, never relabeled as passing.

## FTS gates and evidence

One synthetic raw-chunk corpus below 1,000 rows, with an explicit tokenizer and
no default English stemming/stop-word removal of pretokenized lexical terms.
The native driver has a 180-second process-group deadline; CI also has a
210-second outer hard cap. No scale-up or tuning follows the first result.

The probe must preserve raw native response order, capture native query plans,
index/coverage/version metadata and timings, and compare native LIMIT K output
with a complete **small-fixture diagnostic oracle**. Full enumeration is not an
approved serving strategy. A complete oracle is not, by itself, independent
proof of the BM25 formula.

Required observations: descending score order; tie-aware BM25 Top-K membership;
tenant/workspace prefiltering; exact multiword AND and Chinese pretokenized
terms; add/update/delete including unindexed fresh data; separate Index refresh
and Compact (never All/Prune); reopen; and batch final authority checks against
explicit synthetic fixture state. Fixture checks perform no real Turso calls
and do not prove the eventual production cross-store authority protocol.
A fixture-authority-prefilter arm may enumerate bounded authoritative identities
before native competition, then merge them with scope into one native filter.
That is distinct from FTS full enumeration. Scope-only retrieval followed by
authority filtering is retained as a negative control for candidate starvation;
an expected shortfall there is not automatically an engine correctness failure.

Two distinct tie results must be reported:

1. BM25-valid Top-K accepts any valid members of an equal-score cutoff tie and
   must not mislabel those as score-ranking failures.
2. Existing lexical implementation compatibility sorts all authorized matches
   by score descending, then chunk ID ascending, before its cap. See
   `src/storage/sqlite/app.rs` and `src/storage/sqlite/context.rs`. Strict global
   ID cutoff behavior and repeatability require separate evidence. A mismatch
   is an explicit contract decision/blocker, not a silently relaxed pass.

The `_score DESC, id ASC` builder shape is source-suspect because generated
`_score` is not a stored schema field. Its actual outcome must be recorded.
Neither sorting a truncated K-set nor fully enumerating and sorting all matches
may be substituted to make this gate green. Final authority filtering must not
be presented as complete filtered Top-K when stale top candidates can consume
its budget; the latter remains NOT_RUN unless explicitly proven.

## ANN gates

Re-run the existing exact/IVF_FLAT comparison and frozen filter-aware policy on
0.40.0. Keep the 10k/128D corpus, metric L2, public mapping `1/(1+distance)`,
filter eligibility, probe counts, two-build policy, recall >=0.95, zero safety
violations and original cost gates unchanged. Compare each selected query with
its same-snapshot exact baseline, not with the prior version's result.

Old 0.23.1 reports and failures remain historical; no pass carries forward.
Removed training-loss API data is recorded as unavailable, not invented. Native
plan gates and actual fresh/unindexed-row coverage must be re-established.
No 100k/1M, formal production SLO or real-model relevance claim is authorized.

## CI and stopping boundaries

Only the independent probe workflow changes. Release Clippy/test/build share
release dependency artifacts, and its cache is saved after Cargo resolves the
new lock, including failed diagnostic/provisional-lock runs. Each compile phase
emits Cargo timing artifacts; final-build timing alone is not cold build cost.
Cache contents are never acceptance evidence. Each final command uses
`--locked`; committed formatting and lock identity are checked on the exact
head. A provisional lock or formatter patch is imported in the minimum follow-up
commit before final-head validation. Compilation steps are bounded at 45 minutes;
measurement limits are not extended to compensate for correctness failures.
The job has an overall 180-minute cost cap, not the sum of step allowances.
Failure artifacts/cache are best-effort and cannot be guaranteed after abrupt
runner termination or the overall job timeout.

Failures are retained with raw outputs. Probe API/build defects may be repaired
within this bounded task; genuine engine/contract failures are reported without
altering thresholds or inventing a passing serving strategy. PR #53 stays draft.
No merge, default switch, production dependency change or T1 replacement occurs.

Native results and exact-head artifact links will be reported on
[PR #53](https://github.com/TheLudlows/Origence/pull/53) and
[issue #52](https://github.com/TheLudlows/Origence/issues/52) after execution.


## Initial native bootstrap attempt: dependency resolution failed

Head `9e65e706def1f67e08d67ba2f62531603754486b`, [run 38360488838](https://github.com/TheLudlows/Origence/actions/runs/38360488838),
failed before compilation: published DataFusion 54.0.0 requires Arrow schema
`^58.3.0`, while the probe initially pinned `=58.0.0` from the top-level
LanceDB manifest. Direct Arrow pins and lock guards are corrected to `=58.3.0`.
No native FTS/ANN case ran in that attempt and no new lock was produced.
The failed attempt remains evidence; it is not an engine-ranking result.


## Second bootstrap attempt: released feature combination does not compile

Head `dd605620ed81c55d900cca7df6c75e6a44e9d220`, [run 38361069756](https://github.com/TheLudlows/Origence/actions/runs/38361069756),
resolved the Arrow correction and passed 100 Python tests, but Clippy failed
inside published LanceDB0.40.0 before the probe compiled. `job.rs:102/112` uses
`Error::Http` unconditionally while `error.rs:113` exposes it only with the
`remote` feature. The release has an empty default feature set. This failure is
retained; it is not a failure of a native FTS query that never ran.

The next isolated configuration retains `default-features=false` and enables
only `remote` as the smallest existing build workaround. This adds HTTP/TLS,
OAuth, Arrow Flight/gRPC and namespace REST compilation dependencies. It does
not enable provider features, configure endpoints/credentials, or call remote
services. Both probes construct temporary filesystem paths; remote dispatch
requires a `db` URI. Actual local native behavior still requires the next run.
The Cargo-generated source-format patch from the failed run is imported; its
pre-feature lock is retained as evidence, not substituted for the new graph.

[Published feature declarations](https://github.com/lancedb/lancedb/blob/b1b080b97c3edd2fd9f2aae8a3b71ff19487b1f8/rust/lancedb/Cargo.toml#L164-L177),
[unconditional references](https://github.com/lancedb/lancedb/blob/b1b080b97c3edd2fd9f2aae8a3b71ff19487b1f8/rust/lancedb/src/job.rs#L100-L120),
[conditional variant](https://github.com/lancedb/lancedb/blob/b1b080b97c3edd2fd9f2aae8a3b71ff19487b1f8/rust/lancedb/src/error.rs#L113-L124),
[URI dispatch](https://github.com/lancedb/lancedb/blob/b1b080b97c3edd2fd9f2aae8a3b71ff19487b1f8/rust/lancedb/src/connection.rs#L1477-L1498).
