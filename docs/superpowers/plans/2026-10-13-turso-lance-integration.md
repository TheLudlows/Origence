# Turso authority and LanceDB search integration

Status: merged by PR #58 at main `1d5e0554b38e12df094a894f11a23fb26a117568`.
Implementation engineering CI passed at `b50e8748`; main CI is tracked separately.
This is not production performance or real-model acceptance. Base: main
`86fac48aeabdf349431801334485a2ac99c49448`, preserving restored history.

## Frozen scope

- Turso Database 0.8.2 Rust engine, not libSQL: authority for tenancy, permissions,
  jobs, idempotency, source/version metadata, text, graph and publication recovery.
- LanceDB 0.40.0: derived original-chunk inverted full-text and dense vector
  indexes, with scalar filtering. BlobStore retains originals.
- Fresh databases only; no old-schema upgrades or data migrations. Same-version
  reopen, rollback, crash recovery, idempotence and withdrawal remain required.
- Staged eligibility, native candidate retrieval and fresh authoritative checks;
  native index rows cannot grant authorization or source validity.
- Full-text writes use their own `full_text` ledger surface and exact
  scope/source/version/artifact/generation identity. A publication becomes ready
  only after all derived writes succeed. Replays use the persisted plan.
- Full-text query and indexed text share Jieba preprocessing and explicit
  whitespace tokens, no English stemming/stopwords/ascii folding.
- RRF receives scoped, deduplicated score/ID-sorted branch candidates. Any legal
  equal-score cutoff set is accepted; cross-rebuild membership can change.
- ANN uses the T0 frozen 64-partition IVF_FLAT/L2, 32 probes only for >=4096
  eligible rows and >=40% selectivity; smaller/selective batches use exact.
  This synthetic threshold is not a universal recall or latency guarantee.
- Summary retains authoritative text and existing token aggregation across
  rows of one chunk, preserving cross-row AND; no Turso FTS dependency.

## Delivery and acceptance

1. Adapt the Turso driver, schema admission, transaction/drop/error behavior.
2. Integrate native Lance FTS and filtered ANN through worker, search and Context.
3. Exercise failure between each durable stage, reopen/replay, withdrawal,
   stale native rows, complete scoped-key pre-RRF preparation and Summary.
4. Generate the dependency lock with Cargo, run exact-head formatting, warnings-
   denied Clippy, focused and full tests, HTTP/MCP and process recovery CI.
5. Report PASS/FAIL/NOT_RUN separately; integration was reviewed as a draft and merged only with explicit approval.

The cloud editor has no Cargo. Temporary preparation CI generated the real Cargo
lock and rustfmt artifacts, which were committed and validated with `--locked`.
The preparation workflow is removed after successful integration CI; normal CI
retains all applicable gates. Historical T0 probes/reports stay in PR #53 and are
not copied as a substitute for production integration tests.

Implementation CI: https://github.com/TheLudlows/Origence/actions/runs/38405514854.
All five delivery steps above are complete at the implementation head. Final
archive-only head checks remain visible on PR #58. The merged main run is
https://github.com/TheLudlows/Origence/actions/runs/38417503442; its status does not
follow automatically from the implementation run.
