# Issue #52: isolated ANN T0 preparation

Date: 2026-10-12 UTC. Application baseline: `d0521c0935083e71890ab02c100920c2edaa7227`.

## Decision today

**T0 harness prepared; native result NOT_RUN. No ANN production decision is supported yet. The initial 10k/128D/three-query run is a smoke test, not performance acceptance.**

The isolated crate pins LanceDB `=0.23.1`, disables its default cloud/provider features and does not import Origence. Only `tools/t0-ann-probe/` and this report were added by the ANN preparation task. Root Cargo files, production storage/retrieval/maintenance code and existing application data were untouched. The default plan is 10,000 total synthetic rows, 128 dimensions, real IVF_FLAT, 64 partitions, 32 probes and three queries per bucket. The 100k/1M ladder, higher dimensions and optional IVF_PQ/IVF_HNSW_PQ are explicit manual opt-ins. IVF_HNSW_SQ is NOT_IMPLEMENTED/NOT_RUN in this scaffold.

## Actual validation

| Check | Result | Evidence / limit |
| --- | --- | --- |
| Read repository constraints and relevant source | DONE | AGENTS, STATUS/design, storage/lancedb, retrieval, context_retrieval, storage/maintenance; hashes in isolated source-baseline.json |
| Exact LanceDB 0.23.1 API source inspection | DONE | Official tag resolves to `e8feaede1ee8dc7063c6f1e32b9936dacef915f9`; immutable links in probe README |
| Independent read-only API/lifecycle and measurement review | DONE | No remaining blocking findings after raw-batch, cardinality, retained-hit and plan-proof corrections; not compilation evidence |
| Python driver/report gates | PASS | 22 unit tests; Python compilation also passed |
| Default no-execute plan | PASS | 10k/128D, 5,120,000 raw vector bytes, baseline hashes match; no native execution or DB creation |
| Cargo-generated isolated lockfile | NOT_RUN | No Cargo installed; no lockfile was fabricated or copied |
| Rust formatting / Clippy / compilation / unit tests | NOT_RUN | No Rust/Cargo toolchain in this executor |
| Real IVF_FLAT build, metadata, explain/native plan | NOT_RUN | Code is prepared, not runtime evidence |
| Per-bucket Recall@10 / Recall@100 and exact fallback | NOT_RUN | Requires native execution; configured gates are each >=0.95 and exact fallback 100% |
| Source/visibility/score zero-violation gates | NOT_RUN | Synthetic implementation prepared; production integrations remain separate |
| Fresh rows before/after Index refresh and reopen | NOT_RUN | Requires native execution |
| 512-ID eligibility/native/final verification cost | NOT_RUN | Synthetic route prepared; real SQLite/service cost remains NOT_RUN |
| PQ/HNSW, 100k/1M, high dimensions / real corpus | NOT_RUN | Not automatically run or inferred |
| Production public-library/HTTP path, source bytes, SLO | NOT_RUN | Explicitly outside this isolated mechanics probe |

## Ready for the first authorized execution

See [probe README](../tools/t0-ann-probe/README.md) for exact commands. Generate the **isolated** `Cargo.lock` with Cargo, retain its artifact, import formatting changes, then rerun formatting/Clippy/tests/build with the lock frozen. Build with Rust 1.98.0 and retain logs proving the executable's source. A first CI bootstrap that generates the lock is not the final reproducible locked run.

First native command:

```sh
python3 tools/t0-ann-probe/probe.py --execute --rows 10000 --dimension 128 \
  --queries 3 --index ivf_flat --timeout-seconds 1800
```

The driver requires both lock and executable, verifies the LanceDB version and baseline hashes, creates a unique report directory, and applies a native-process deadline. It never installs software or provisions compute. Record any compilation/API correction as a correction, then rerun the final source; do not label static source review as a native pass.

The default three-query configuration has 30 indexed + 33 pre-refresh + 33 post-refresh paired measurements (96 query rows), plus one reopened fresh pair (97 total). Every pair also runs a forced exact fallback. The 512-ID comparison runs for query zero of each bucket/phase and the reopened fresh pair (33 groups).

## Acceptance evidence the executable will retain

1. Same snapshot/layout/query/L2/predicate/100-candidate exact-vs-ANN pairs; explicit prefilter, one merged AND filter, no fast search.
2. Index type/column/metric from list/statistics plus full native explain text, independently parsed by the Python gate. A nonempty query without recognized native ANN proof fails rather than claiming an indexed speedup.
3. Every query's retained exact/ANN/verified/fallback hit tuples and distances, with Python-recomputed recall/equality, plus canonical fixture SHA-256 digests. Recall at 10 and 100, including empty/one/<K/dense/sparse/tenant/workspace/source-version/historical/fresh buckets. Empty recall is null, not an invented 100%.
4. Zero source/visibility/duplicate/distance-score violations and forced exact fallback equality. Exact fallback success never hides a failed raw ANN bucket.
5. Separate enumeration, authority-map, predicate, 512-ID native query count/time, merge and final-verification timings. These are explicitly in-memory authority simulations, not measured SQLite performance. Selected component sums omit some setup and are not full pipeline wall times.
6. Distinct Compact, index build and Index refresh timings; new rows demonstrably unindexed and returned before refresh, indexed afterward; unchanged row/fragment counts during index-only refresh; reopen evidence.

## Unresolved risks / honest boundaries

- Static API alignment does not establish compilation. Native builder/planner/index-stat behavior must be observed on the pinned crate.
- Fixed 32-of-64 probes can underfill selective or small eligible sets; this is intentionally surfaced as a failed bucket. A future adaptive or small-scope exact policy needs separate evidence. Nothing automatically tunes parameters until a pass appears.
- Easy clustered synthetic geometry can pass while real embeddings fail. Three samples per bucket cannot justify a production p95 or general recall claim. Explain plans are prepared before each pair, so samples are not cold; odd query counts are not order-balanced and the batched route always runs ANN first. Timings are descriptive only.
- Historical/current artifacts use distinct stored IDs here. Forged stale same-ID metadata is rejected by a synthetic verifier, but the real application's reused-ID/generation/source-event/visibility contracts are not exercised.
- Full eligible ID filters are still enumerated and can be large. This does not validate a coarser-predicate redesign or eliminate the existing 512-ID path's overhead.
- No source-byte preservation, crash/rebuild interruption, deletion, concurrent-write or real-service transaction proof is included. No production behavior, capability flag or score mapping has changed.
- The historical 563-row/1024D/49.44-second result is not reproduced and must not be explained or compared using this differently sized/dimensional toy fixture.
