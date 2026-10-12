# Issue #52: isolated ANN T0 results

Updated: 2026-10-12 UTC. Application baseline: `d0521c0935083e71890ab02c100920c2edaa7227`.

## Decision today

**The native probe executed and rejected the tested fixed 32-of-64 IVF_FLAT policy.** All 97 paired measurements completed; 49 failed the unchanged per-query Recall@10/@100 >=0.95 gate. This is a genuine recall result, not a compilation failure, timeout or identified probe defect. It is a 10k/128D synthetic smoke test, not production performance acceptance or a verdict against LanceDB ANN generally.

Exact fallback matched in all 97 pairs. Every recorded source, visibility, duplicate and distance/score violation counter was zero, including raw ANN/exact 512-ID results before truncation. All 88 nonempty pairs had actual native ANN plan evidence. The 33 batched groups reproduced the single-query exact result; 20 batched groups failed ANN recall.

## Exact evidence

- Draft PR head: `8d0af2136e95331eebcfa0e1e7422992d41e8d3d`.
- [Workflow run 38257392122](https://github.com/TheLudlows/Origence/actions/runs/38257392122), [ANN job 114838864911](https://github.com/TheLudlows/Origence/actions/runs/38257392122/job/114838864911).
- [Original ANN artifact 11752387239](https://github.com/TheLudlows/Origence/actions/runs/38257392122/artifacts/11752387239), SHA-256 `1250f53e432a3a584b73585e1494b0b8336a8bd6330189318cfdee2c3c8d4319`.
- [Concise derived evidence JSON](../tools/t0-ann-probe/evidence/bootstrap-8d0af213.json) records individual file hashes, build/source/lock identity, fixture digests, bucket minima, index/layout evidence and validator error categories. Original artifact data was not modified.

The bootstrap generated the isolated Cargo lock and applied rustfmt before build. Warnings-denied Clippy, all 3 Rust fixture tests, all 22 Python tests and the release build passed with Rust 1.98.0 and LanceDB 0.23.1. Its committed-format/lock gate correctly failed because these generated changes were not in that head. The Cargo-generated lock and exact formatter patch have now been imported locally; a committed, locked exact-head rerun remains pending. Build source hashes in the artifact identify the formatted source actually executed.

## Per-bucket recall

Minima below span the three measured phases and, for fresh rows, the reopened check. Empty buckets must return zero rows and have no recall percentage.

| Bucket | Eligible rows | Minimum Recall@10 | Minimum Recall@100 |
| --- | ---: | ---: | ---: |
| Empty | 0 | n/a | n/a |
| One | 1 | 1.0000 | 1.0000 |
| Fewer than 10 | 7 | 0.7143 | 0.7143 |
| Fewer than 100 | 37 | 1.0000 | 0.5135 |
| Dense current | 4,340–4,356 | 1.0000 | 1.0000 |
| Sparse | 45 | 1.0000 | 0.4444 |
| Other tenant | 218 | 1.0000 | 0.9000 |
| Other workspace | 217 | 1.0000 | 0.8500 |
| Source/version | 8 | 0.3750 | 0.3750 |
| Historical generation | 4,340 | 1.0000 | 1.0000 |
| Fresh rows, all phases | 16 | 0.6000 | 0.3750 |

Returning the requested number is not the recall gate: one workspace query returned 92 candidates but recalled only 85 of the exact top 100. Full retained hit lists independently reproduce the reported recall, rather than deriving it only from returned count.

## Fresh-row and maintenance finding

Before refresh, statistics reported 9,984 indexed and 16 unindexed rows. All three fresh queries returned all 16 rows, with Recall@10/@100=1. After the explicit index-only refresh, statistics correctly reported 10,000 indexed and zero unindexed rows, while the fresh ANN queries returned only 8, 9 and 6 rows. Their Recall@10 was 0.8/0.9/0.6 and Recall@100 was 0.5/0.5625/0.375. Reopen reproduced the first result (8/16).

Exact search and forced fallback still returned every fresh row after refresh. The result is consistent with fixed-probe partition undercoverage, not observed row loss. It illustrates why unindexed-tail correctness alone cannot establish post-refresh filtered ANN recall. The independent saturation diagnostic below will test that interpretation without changing the default gate.

Physical compaction reduced the initial 39 fragments to 3 before either exact/ANN query ran. Appending fresh rows added one fragment. Index-only refresh retained 4 fragments and all 10,000 rows. Only explicit `Compact` and `Index` actions were used, with no `All` or `Prune`.

## Next bounded diagnostic

Retain the default 32/64 run and its failing result. Separately run an explicit **64/64 saturation diagnostic** at the same 10k rows, 128D, candidate target 100 and seed 52:

```sh
python3 tools/t0-ann-probe/probe.py --execute --rows 10000 --dimension 128 \
  --queries 3 --index ivf_flat --partitions 64 --nprobes 64 --timeout-seconds 1800
```

Both runs retain the >=0.95 recall and zero-violation gates. This second invocation creates another database and independently trained index; compare canonical fixture digests, not physical index identity. Training randomness is not explicitly seeded. Each individual exact/ANN pair still uses its own same frozen snapshot, layout, L2 metric, merged predicate and candidate budget. Probing every IVF partition is a mechanics/coverage control, not evidence of useful approximation, ANN speedup or a suitable production default. There is no automatic tuning or promotion.

No probe code correction is currently warranted by the native result. A future sparse-scope exact policy, adaptive probing or revised candidate strategy needs a separately specified and tested policy; forced exact success does not convert this raw ANN failure into a pass.

## Validation boundaries

| Check | Current result |
| --- | --- |
| Official 0.23.1 API and independent source review | Complete; no remaining blocking source-review findings |
| Python tests / Rust fixture tests / warnings-denied Clippy / release build | PASS on bootstrap artifact source |
| Cargo-generated lock and rustfmt | Generated and imported; committed locked rerun pending |
| Real IVF_FLAT type/metric/coverage/native plan proof | PASS for all 88 nonempty paired measurements |
| Fixed 32/64 per-bucket ANN recall | FAIL: 49/97 pairs; thresholds unchanged |
| Exact fallback and batched-exact equivalence | PASS: 97/97 and 33/33 respectively |
| Synthetic source/visibility/distance invariants | Zero violations recorded; not production authorization proof |
| Independent 64/64 saturation diagnostic | NOT_RUN |
| PQ/HNSW, 100k/1M, high dimensions, real corpus | NOT_RUN; HNSW_SQ not implemented in this scaffold |
| Production public-library/HTTP path, source bytes, SLO | NOT_RUN |

The isolated crate does not import Origence or change production modules, root Cargo files or application data. It retains full eligibility enumeration and measures the 512-ID shape separately. These are in-memory authority simulations, not measured SQLite/service costs. `selected_ann_component_sum_ms` omits some setup and is not full pipeline wall time.

All explain plans are prepared before timing; default query order is not balanced, and the batched comparison always runs ANN first. Three samples per bucket cannot support a production p95/P99 or unbiased speedup claim. The historic 563-row/1024D/49.44-second observation is not reproduced or explained by this fixture.

Stored historical/current artifact IDs are distinct. Forged stale metadata and withdrawals are tested through a synthetic verifier, not the real source-event/identity/context-route contracts. Source-byte preservation, deletion/rebuild interruption, crash recovery, concurrent writes, real embeddings and production scale remain untested.

## Preparation review resolved

Two read-only reviews found no remaining blocking API/lifecycle or measurement-gate findings after corrections. Raw ANN and exact results are checked against each 512-ID batch before merging or truncation; fallback equality includes tenant/workspace. Required cardinalities and the reopened fresh-query identity/snapshot are enforced. Retained hit lists support independent recall/fallback checks, and actual plan text and index metadata support native ANN proof.

All 22 Python tests passed, including independent mutations for all-empty fixtures with cleared hit lists, wrong reopen identity, contradictory ANN plans, raw-batch violations and fallback workspace mismatches. See the [probe README](../tools/t0-ann-probe/README.md) for reproduction commands and API sources.
