# Issue #52: isolated ANN T0 results

Updated: 2026-10-13 UTC. Application baseline: `d0521c0935083e71890ab02c100920c2edaa7227`.

This report preserves named-head evidence snapshots. Terminal CI for the latest source head is tracked in [draft PR #53](https://github.com/TheLudlows/Origence/pull/53); a report-only follow-up commit is not required after every final CI run.

## Filter-aware follow-up: initial bounded pass

**The frozen filter-aware candidate passed both fresh 10k/128D builds on the artificial test mix.** Each build passed all 576 pairs across 72 bucket/snapshot cases; every nonempty Recall@10/@100 was 1.0, with zero raw/final invariant violations. This is a bounded synthetic result, not a production ANN default or SLO.

Evidence: head `fa19309ff6a0f8623f1670b0a3a34a686073ad85`, [run 38333646435](https://github.com/TheLudlows/Origence/actions/runs/38333646435), [ANN artifact 11785013021](https://github.com/TheLudlows/Origence/actions/runs/38333646435/artifacts/11785013021), SHA-256 `50f432565dece226edfd82fd003aa9d05e6aa47d283e98a38ae5e863ac8b325e`. The [compact evidence JSON](../tools/t0-ann-probe/evidence/filter-policy-fa19309f.json) records provenance, exact gates, routing/call counts and separate costs; full hits and plans remain in the original artifact.

| Frozen benefit measure | Build 1 | Build 2 | Required |
| --- | ---: | ---: | ---: |
| Sum selected complete wall / sum all-exact wall | 0.844294 | 0.847120 | <=0.90 |
| ANN-selected median paired wall ratio | 0.517181 | 0.511865 | <=0.90 |
| Worst bucket/snapshot median paired ratio | 1.021915 | 1.020810 | <=1.10 |

Complete selected-path totals were 29.907 s and 29.759 s versus all-exact 35.423 s and 35.130 s, about 15.6% and 15.3% lower on this fixed mix. Each build routed **144 samples to genuine 32/64 ANN, 408 to exact and 24 empty**. All-exact matched the separate oracle in 576/576 samples; every selected-exact result also matched. All 16 fresh rows returned in all eight queries before refresh, after refresh and on reopen.

Each build executed 552 selected-path, 552 all-exact and 552 diagnostic-oracle vector queries, plus 1,104 per-vector explain requests; 64 unique plan texts were retained without dropping slot associations. Native processes took 178.55 s / 177.10 s and Python audits 6.18 s / 2.61 s. Independent revalidation reconstructed fixture/query hashes, authority and full-vector distances for all 435,360 retained hit entries across both reports, and recomputed both benefit decisions. The reusable report reader audits the native exact oracle rather than exhaustively selecting neighbors. A separate offline NumPy audit did exhaustively reconstruct top-10/top-100 sets for all 1,152 pairs (2,304 set checks): all matched, with all 435,360 hit entries passing authority/L2 checks and maximum absolute L2 difference 2.13e-5.


This isolated experiment is frozen in the [policy contract](../tools/t0-ann-probe/policy-contract.json); original smoke results and artifacts below remain unchanged. Use fixed 32/64 IVF_FLAT only for at least 4,096 eligible rows and fraction >= 0.40, otherwise exact bypass; empty scopes make zero vector calls in both arms. The fraction condition is redundant at 10k. Calibration uses the previous dense/history evidence; routing never reads query distances, observed recall, returned K or answers.

The opt-in launcher ran exactly two fresh 10k/128D rebuilds, each capped at 900 native-process seconds. Each has 72 bucket/snapshot cases × eight new near-row/inter-cluster queries = 576 pairs, including count boundaries, rotated/subsampled selections, all lifecycle phases and reopened fresh rows. Training randomness is uncontrolled; two rebuilds are not a statistical-independence claim. Every attempt is retained.

Selected-policy and all-exact arms independently measure the same complete synthetic candidate-path wall-clock boundary, including eligibility, authority maps/sets, routing/filter/query build, native execute/decode/sort, final verification and cleanup. Each case has balanced four AB/four BA orders; oracle/audits follow the pair and plans follow the bucket. Actual plans for all eight vectors in every case are collected after the bucket (1,104 explain calls/build), pooled without truncation, hit lists/query/fixture hashes retained, and actual SDK vector/diagnostic/setup call counts separated. This remains prepared in-memory cost, not SQLite/service or cold-storage performance.

Both builds must pass every >=0.95 Recall@10/@100 and zero-invariant gate; exact-selected results must equal the separate oracle. Exploratory benefit additionally requires workload sum-time ratio <=0.90, ANN-stratum median paired ratio <=0.90, and every bucket/snapshot median <=1.10, separately per rebuild. If quality or benefit fails, stop and recommend exact for this tested mix without changing thresholds, probes or test weights. No automatic larger rung or production activation follows. Commands, timing details and remaining limits are in the [README](../tools/t0-ann-probe/README.md).

The initial follow-up passed 69 Python tests, five Rust tests, warnings-denied Clippy, locked release build and both policy gates. Its committed-formatting gate failed as expected because CI produced the first formatter patch (28,014 bytes); those formatting-only changes have been imported. A final committed-format exact-head rerun is still required and tracked in PR #53. The local authoring executor still lacks Rust/Cargo; dependencies and Cargo lock are unchanged. The original smoke in the same artifact still fails at 32/64 (49/97 pairs and 17/33 batches); the 64/64 control passes 97/97 and 33/33. No historical failure was reclassified or threshold changed.

## Previous frozen decision

**The locked run still rejects the default fixed 32-of-64 IVF_FLAT policy. The independent 64-of-64 saturation control passes the bounded mechanics gates.** The default failed recall in 43/97 pairs; the full-partition control passed 97/97 pairs and 33/33 batched groups. Every nonempty control Recall@10/@100 was 1.0.

This supports fixed-probe undercoverage as the explanation for the tested selective-query misses. It does not establish useful approximation, a production ANN default or a speedup: the control searches every partition, and the two invocations independently trained different physical indexes. Each exact/ANN pair remains pinned to its own same snapshot, layout, L2 metric, predicate and candidate target 100.

Both runs matched exact fallback in all 97 pairs, proved native ANN on all 88 nonempty pairs, and recorded zero source, visibility, duplicate and distance/score violations, including raw 512-ID results before truncation. These are synthetic checks, not production authorization/source-preservation proof.

## Latest exact evidence

- Locked diagnostic head: `5593a18e7c1e0e4efeb4ca5328dd6b1340175d24`.
- [Workflow run 38261865400](https://github.com/TheLudlows/Origence/actions/runs/38261865400), [ANN job 114854128752](https://github.com/TheLudlows/Origence/actions/runs/38261865400/job/114854128752).
- [Original ANN artifact 11754305833](https://github.com/TheLudlows/Origence/actions/runs/38261865400/artifacts/11754305833), SHA-256 `19954885278f2247120e49675faf0f40bfab71605b2553e4e68036230faf726f`.
- [Concise locked diagnostic evidence JSON](../tools/t0-ann-probe/evidence/locked-5593a18e.json) records file hashes, build identity, fixture digests, bucket minima, index/layout evidence, separate cost components and independent validator results. Original artifacts are unchanged.

Warnings-denied Clippy, all 3 Rust fixture tests, all 22 Python tests, release build and committed formatting/lock checks passed with Rust 1.98.0 and LanceDB 0.23.1. The overall ANN job remains failed because the original 32/64 recall gate is preserved. The 64/64 control ran afterward under an independent workflow condition and passed; it does not erase the first result.

Both reports use the same executable, manifest, Cargo lock and Rust/Python source hashes. Their canonical indexed/final fixture hashes also match each other and the bootstrap. Native index-training randomness is not explicitly seeded, and physical index UUIDs/training results differ across invocations.

## Latest per-bucket recall

Minima span all measured phases and the reopened fresh check. Empty buckets return zero rows and have no recall percentage. All nonempty full-partition minima were 1.0.

| Bucket | Eligible rows | Default 32/64 Recall@10 | Default 32/64 Recall@100 | Control 64/64, both K |
| --- | ---: | ---: | ---: | ---: |
| Empty | 0 | n/a | n/a | n/a |
| One | 1 | 1.0000 | 1.0000 | 1.0000 |
| Fewer than 10 | 7 | 0.4286 | 0.4286 | 1.0000 |
| Fewer than 100 | 37 | 1.0000 | 0.4324 | 1.0000 |
| Dense current | 4,340–4,356 | 1.0000 | 1.0000 | 1.0000 |
| Sparse | 45 | 1.0000 | 0.4667 | 1.0000 |
| Other tenant | 218 | 1.0000 | 1.0000 | 1.0000 |
| Other workspace | 217 | 1.0000 | 0.9100 | 1.0000 |
| Source/version | 8 | 0.2500 | 0.2500 | 1.0000 |
| Historical generation | 4,340 | 1.0000 | 1.0000 | 1.0000 |
| Fresh rows, all phases | 16 | 0.8000 | 0.5000 | 1.0000 |

The default had 14 failing batched groups out of 33. Its batched-exact results matched single-query exact in all 33 groups. The control passed all 33 batched groups. The independent Python verifier reported only the expected default recall/status failures (140 errors including repeated gate summaries) and zero control errors; it recomputed recall and fallback equality from retained hit tuples. A separate NumPy reconstruction matched both fixture digests, exact top-10/top-100 sets in all 194 pairs (388 set checks), and authority/L2 values for all 44,666 retained pair and merged-batch hits.

## Fresh rows and maintenance

Both runs reported 9,984 indexed plus 16 unindexed rows after append. Both returned every fresh row before refresh. After explicit index-only refresh, both correctly reported 10,000 indexed and zero unindexed rows; exact and forced fallback still returned all 16.

The default ANN then returned 10, 10 and 8 fresh rows, with Recall@100=0.625/0.625/0.5. Reopen returned 10/16. The 64/64 control returned 16/16 in all queries before refresh, after refresh and on reopen, with Recall@10/@100=1.0. This is consistent with probe coverage, not observed row loss. Complete pre-refresh unindexed-tail results alone do not establish post-refresh ANN recall.

Physical compaction reduced 39 initial fragments to 3 before comparisons. Fresh append added one fragment. Index-only refresh retained 4 fragments and all 10,000 rows in both runs. Only explicit `Compact` and `Index` actions were used, with no `All` or `Prune`.

## Candidate-path cost remains

Dense buckets have 4,340–4,356 eligible rows and still issue **nine serial native calls per mode** in the 512-ID shape. The evidence JSON keeps eligibility enumeration, authority-map construction, predicate construction, native query, merge and final-verification components separate for each phase and configuration. A passing native index control does not remove enumeration or batching.

Those are prepared in-memory synthetic costs, not measured SQLite/service costs. `selected_ann_component_sum_ms` omits some setup and is not full pipeline wall time. Explain plans are prepared before timing, default query order is not balanced, and batched queries always run ANN first. Three samples per bucket cannot support production p95/P99 or an unbiased cross-run speedup claim.

## Bootstrap retained

The earlier bootstrap at head `8d0af2136e95331eebcfa0e1e7422992d41e8d3d`, [run 38257392122](https://github.com/TheLudlows/Origence/actions/runs/38257392122), completed all 97 pairs with 49 recall failures, 97 exact-fallback matches and zero violation counters. Fresh post-refresh ANN returned 8, 9 and 6 rows. Its source/lock gate failed as expected while generating the provisional lock and formatter patch.

The unchanged [bootstrap evidence JSON](../tools/t0-ann-probe/evidence/bootstrap-8d0af213.json) links [artifact 11752387239](https://github.com/TheLudlows/Origence/actions/runs/38257392122/artifacts/11752387239), SHA-256 `1250f53e432a3a584b73585e1494b0b8336a8bd6330189318cfdee2c3c8d4319`. The locked run's 43 failures versus bootstrap's 49 is compatible with unseeded index-training variation; it is not a changed threshold or demonstrated improvement. Both reject the same tested fixed-probe policy.

## Validation boundary and next decision

| Check | Current result |
| --- | --- |
| Official 0.23.1 API and independent source review | Complete; no remaining blocking findings |
| Python tests / Rust tests / warnings-denied Clippy / release build | PASS on locked head |
| Committed Cargo-generated lock and formatting | PASS on locked head |
| Real IVF_FLAT type/metric/coverage/native plan proof | PASS: 88 nonempty pairs in each configuration |
| Default fixed 32/64 recall | FAIL: 43/97 latest pairs; >=0.95 thresholds unchanged |
| Independent full-partition 64/64 control | PASS: 97/97 pairs, 33/33 batched groups; exhaustive partition coverage only |
| Exact fallback and batched-exact equivalence | PASS: 97/97 and 33/33 in both runs |
| Synthetic source/visibility/distance invariants | Zero violations in both runs; not production authorization proof |
| PQ/HNSW, 100k/1M, high dimensions, real corpus | NOT_RUN; HNSW_SQ not implemented here |
| Production public-library/HTTP path, source bytes, SLO | NOT_RUN |

No probe implementation defect was identified, and no semantic Rust/Python correction was made to obtain the control pass. No production module, root Cargo file, capability flag or score mapping changed. The control is explicitly configured and independently reported; it is not the default and does not automatically promote ANN.

Before an application ANN policy can be accepted, specify and separately validate sparse-scope exact fallback/adaptive probes and the actual candidate-filter strategy against representative embeddings, dimensions and scale. Full authorized ID enumeration is still retained here. Do not infer that saturating every IVF partition is a satisfactory production policy.

Stored historical/current artifact IDs are distinct. Forged stale metadata and withdrawals use a synthetic verifier, not real source-event/identity/context-route contracts. Source-byte preservation, deletion/rebuild interruption, crash recovery, concurrent writes and real-service transactions remain untested. The historic 563-row/1024D/49.44-second observation is not reproduced or explained by this fixture.

## Preparation review resolved

Two read-only reviews found no remaining blocking API/lifecycle or measurement-gate findings after corrections. Raw ANN and exact results are checked against each 512-ID batch before merging/truncation; fallback equality includes tenant/workspace. Required cardinalities and reopened fresh-query identity/snapshot are enforced. Retained hit lists, actual plan text and index metadata support independent checks.

All 22 Python tests passed, including mutations for all-empty fixtures with cleared hits, wrong reopen identity, contradictory plans, raw-batch violations and fallback workspace mismatch. See the [probe README](../tools/t0-ann-probe/README.md) for commands and API sources.
