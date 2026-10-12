# Isolated LanceDB 0.23.1 T0 probe

Status: the committed locked run compiled and executed. The default **32/64 probe policy failed recall in 43/97 pairs**; an explicitly separate **64/64 full-partition control passed all 97 pairs and 33 batched groups**. All exact fallbacks matched and violation counters were zero in both runs. The control is exhaustive partition coverage, not a production default or speedup result. The bootstrap remains preserved with its original 49/97 failures. See [results and exact artifact provenance](../../docs/t0-ann-report.md). The local authoring executor still has no Rust toolchain; native evidence came from the authorized CI runner.

This crate is an independent workspace and has no dependency on Origence. It never opens application data, calls a model, or provisions paid resources. All database writes are under a new temporary directory inside `tools/t0-ann-probe/runs/`. No root Cargo files or production modules are changed. Output reports contain only synthetic data.

## Reproduce

Run from the repository root, on an authorized machine with the repository's declared Rust **1.98.0**, C/C++ build tools and Protobuf compiler/development headers. Do not silently choose a different toolchain. The exact direct LanceDB pin is `=0.23.1`, with default cloud/provider features disabled.

1. Use the isolated Cargo-generated lockfile. If starting from a preparation snapshot without it, generate it once with Cargo and retain/commit it before the final reproducible run. Never hand-write it or copy the application's lock. The bootstrap artifact lock has been imported.

   ```sh
   test -f tools/t0-ann-probe/Cargo.lock || cargo +1.98.0 generate-lockfile --manifest-path tools/t0-ann-probe/Cargo.toml
   cargo +1.98.0 fmt --manifest-path tools/t0-ann-probe/Cargo.toml
   cargo +1.98.0 fmt --manifest-path tools/t0-ann-probe/Cargo.toml -- --check
   cargo +1.98.0 clippy --locked --manifest-path tools/t0-ann-probe/Cargo.toml --all-targets -j 2 -- -D warnings
   cargo +1.98.0 test --locked --manifest-path tools/t0-ann-probe/Cargo.toml -j 2
   cargo +1.98.0 build --locked --manifest-path tools/t0-ann-probe/Cargo.toml --release -j 2
   python3 -m unittest discover -s tools/t0-ann-probe -p 'test_*.py' -v
   ```

2. Inspect the bounded plan. This command does not run Rust, install dependencies, create a database, or generate a lock:

   ```sh
   python3 tools/t0-ann-probe/probe.py
   ```

3. Run the first rung only. Keep Cargo build/test/Clippy logs together with the report to establish the executable's provenance:

   ```sh
   python3 tools/t0-ann-probe/probe.py --execute --rows 10000 --dimension 128 \
     --queries 3 --index ivf_flat --timeout-seconds 1800
   ```

   The driver requires the isolated lock and executable to exist, verifies `lancedb 0.23.1` in the lock, and verifies the inspected application-source hashes. It logs the binary, manifest and lock hashes. It does not prove that an arbitrary `--binary` was built from this source; retain the corresponding locked build log. On Unix, timeout kills the native process group. There is no hard RAM cap. A killed process may leave only its isolated fixture under `runs/`.

4. Optional, **manual** extensions after reviewing that rung:

   ```sh
   # Separate databases/runs; no automatic parameter tuning or index promotion.
   python3 tools/t0-ann-probe/probe.py --execute --index ivf_pq --refine-factor 10
   python3 tools/t0-ann-probe/probe.py --execute --index ivf_hnsw_pq --refine-factor 10
   # Explicit scale opt-in; these are NOT_RUN until deliberately invoked.
   python3 tools/t0-ann-probe/probe.py --execute --rows 100000 --allow-scale
   python3 tools/t0-ann-probe/probe.py --execute --rows 1000000 --allow-scale --timeout-seconds 7200
   # Separately test a real deployment dimension only with an explicit opt-in.
   python3 tools/t0-ann-probe/probe.py --execute --dimension 1024 --allow-scale
   ```

   Supported row totals are 10k, 100k and 1M, including 16 fresh rows. Dimensions are bounded to 128, 384, 768 and 1024. The driver prints raw-vector bytes; actual memory is substantially larger. One million 1024D vectors alone use 4,096,000,000 bytes, before metadata, Arrow copies, training, indexes and execution. No command automatically runs the ladder or provisions a larger machine.

5. Re-validate a native report:

   ```sh
   python3 tools/t0-ann-probe/probe.py --report tools/t0-ann-probe/runs/RUN/native-report.json
   ```

Each run creates `config.json`, `environment.json`, `native.log`, `native-report.json`, `driver-result.json` and `summary.md`. The native report includes the complete explain plans and the exact, raw ANN, verified ANN and forced-fallback hit identities/distances/scores for every pair. The Python gate independently recomputes recall and exact/fallback identity-distance equality from these lists and checks the ANN operator in the actual plan text, plus index-list metadata. Raw pre-truncation batched results retain separate ANN/exact invariant counters and counts, while their merged candidate lists are retained for audit. `summary.md` shows per-bucket minimum recall, never just an aggregate mean. Defaults are 64 IVF partitions, 32 fixed probes, seed 52, candidate target 100 and three queries per bucket. This yields 30 indexed + 33 pre-refresh + 33 post-refresh query rows, plus one reopened pair (97 total); every pair also executes forced exact fallback. The 512-ID route runs in 33 bucket/phase groups. These are declared initial settings, **not validated tuning**. In particular, selective/small eligible sets can fail at fixed 32 probes. A raw ANN failure remains a failure even if forced exact fallback passes. Any later tuning must be a new labeled run with its own evidence. A run probing all partitions may prove mechanics but cannot establish useful approximation or speedup. The completed, explicitly scheduled 64/64 control remains separate from the failing 32/64 default: it rebuilt an independent index and shares the canonical fixture and executable hashes, not physical index identity or training randomness. Its full recall is evidence of bounded mechanics, not efficient ANN performance.

## What is measured

- 9,984 initial rows plus 16 post-index rows at the default 10k rung. Deterministic 16-cluster synthetic vectors with nonzero high-dimensional noise; no embedding provider. Canonical SHA-256 digests include all row metadata, visibility and complete float-vector bytes before and after appending fresh rows. The driver also records fixture-source hashes. The native index-training RNG is not explicitly seeded by this builder. Historic and current artifacts have distinct IDs, with explicit source versions and vector generations. Foreign scopes, historical versions and withdrawn rows are deliberately geometrically close to eligible rows.
- Initial append batches of 256, one explicit `OptimizeAction::Compact` using two compaction tasks and 4,096 target rows per fragment, then an explicit `Index::IvfFlat` build. Optional IVF_PQ/IVF_HNSW_PQ use their actual 0.23.1 builders. IVF_HNSW_SQ remains NOT_IMPLEMENTED/NOT_RUN.
- Exact and ANN query the **same table, same compacted layout, same explicitly checked-out snapshot, same L2 metric, same merged predicate, same query vector and same 100-candidate limit**. Phases alternate exact/ANN order across query iterations, retain first native executions and report explain time separately. Explain-plan creation precedes every pair and can warm metadata/index caches; these are prepared smoke timings, not cold samples. With three iterations the order is two exact-first and one ANN-first; the batched comparison is always ANN-first. No unbiased speedup claim is made. No uncontrolled mutations occur within a phase.
- Exactly one `only_if` call combines tenant, workspace, generation, source version, optional source and authorized IDs. Public `HasQuery::mut_query()` explicitly sets `prefilter=true` and `fast_search=false`. The exact route uses `bypass_vector_index()`. ANN uses a fixed `nprobes`, not a silently adaptive retry. Full authorized ID predicates are retained: this is not a coarser authorization redesign.
- Buckets: eligible count 0, 1, 7 (`<10`), 37 (`<100`), dense, stride-97 sparse, another tenant, another workspace, one source/version, historical generation, and fresh rows. Both Recall@10 and Recall@100 come from the same fixed candidate run. Recall denominator is `min(K, exact eligible count)`; empty recall is `null` and zero rows are required.
- Native ANN proof requires a matching `list_indices`, matching L2 `index_stats`, indexed row counts, and an actual recognized ANN operator in `explain_plan(true)` for each nonempty one-query measurement. The exact explain plan must have no ANN operator. Unrecognized plan text fails closed; the full text is retained for diagnosis. The index's existence or a faster timing is never sufficient proof.
- Every native hit, including every raw ANN/exact 512-ID result before merge/truncation, is checked against its own allowed-ID batch and the synthetic authority ledger for tenant/workspace, eligibility, source ID/version and generation; duplicates and nonfinite/distorted distances fail. Returned L2 distance is checked against squared Euclidean full-vector distance, and the application mapping remains `1/(1+returned_L2)`. PQ/HNSW always uses `refine_factor` before these scores are accepted; quantized distances cannot quietly change RRF inputs.
- Per-bucket native Recall@10/@100 must each be at least 0.95. Forced exact fallback must reproduce exact IDs/source/version/generation/distances/scores/order at 100%, on every bucket. Native and batched zero-violation checks are separate from recall. No overall average can rescue one failed bucket.
- The 512-ID shape is also executed once per bucket per phase with the same fixed per-call target, both native ANN and exact. Record eligibility enumeration, authority lookup-map construction, predicate construction, native-query count, native-query durations, merge cost and final verification separately. The one-query authorized-ID filter and the 512-ID route are reported separately, so reducing one native search cannot be mistaken for eliminating enumeration or batching. `selected_ann_component_sum_ms` sums only named components and excludes some setup (such as allowed-set construction and query-builder work); it is not full pipeline wall time.
- Synthetic final verification rejects forged same-ID source/version/generation/scope and a withdrawal between native search and final verification. This does not call or replace the real SQLite authority or prove its contract.
- After index creation, append 16 rows and require `unindexed_rows=16`; measure before refresh with fast search disabled. Use only `OptimizeAction::Index(OptimizeOptions::default())`, separately time it, require `unindexed_rows=0`, unchanged row/fragment counts, and remeasure. Reopen and recheck index metadata and fresh queries. **No `All`, no `Prune`, no old-file deletion**. The temporary fixture itself is removed after successful execution; retained reports survive.

## Deliberate limits and decision boundary

The default 10k/128D, three-query run is a smoke test, not performance acceptance. Passing this fixture is only a T0 mechanics result. Production authorization/source-byte preservation, application public-library and HTTP paths, memory identity/context routes, SQLite eligibility costs, reused artifact IDs across stored versions, deletion/rebuild interruption, restart crash recovery, concurrent reads/writes, real-corpus embeddings, real dimensions, multi-seed distributions, storage costs and production p95/recall remain separate work. The forged-hit checks are a synthetic surrogate, not production integration coverage.

Do not compare these numbers with the historical 563-row/1024D/49.44-second observation: that result is neither reproduced nor explained by this 10k/128D harness. Do not replace `filtered_ann: false`, remove application final verification, change score semantics, or enable ANN from this experiment. Large eligibility enumeration and many 512-ID native calls remain visible costs even when a native ANN call is faster.

## Verified API sources

Sources were inspected at LanceDB tag `v0.23.1`, resolved to immutable commit `e8feaede1ee8dc7063c6f1e32b9936dacef915f9`:

- [Real IVF_FLAT / PQ / HNSW builders](https://github.com/lancedb/lancedb/blob/e8feaede1ee8dc7063c6f1e32b9936dacef915f9/rust/lancedb/src/index/vector.rs)
- [Index variants, index metadata/statistics](https://github.com/lancedb/lancedb/blob/e8feaede1ee8dc7063c6f1e32b9936dacef915f9/rust/lancedb/src/index.rs)
- [only_if replacement semantics, public prefilter/fast_search request, nprobes, exact bypass and refine](https://github.com/lancedb/lancedb/blob/e8feaede1ee8dc7063c6f1e32b9936dacef915f9/rust/lancedb/src/query.rs)
- [Separate Compact and Index actions, version/checkout, list_indices and index_stats](https://github.com/lancedb/lancedb/blob/e8feaede1ee8dc7063c6f1e32b9936dacef915f9/rust/lancedb/src/table.rs)
- [0.23.1 dependency versions: Lance 1.0.1 and Arrow 56.2](https://github.com/lancedb/lancedb/blob/e8feaede1ee8dc7063c6f1e32b9936dacef915f9/Cargo.toml)

Reading these sources is API design evidence; successful compilation and actual native plans remain runtime evidence that must be collected.
