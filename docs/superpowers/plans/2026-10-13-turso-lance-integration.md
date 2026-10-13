# Turso authority and LanceDB search integration

Status: implementation in progress, not production acceptance. Base: main
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
5. Keep the PR a draft, report PASS/FAIL/NOT_RUN separately, never auto-merge.

The cloud editor has no Cargo. Temporary preparation CI produces a real Cargo
lock and rustfmt diff as artifacts; these must be committed and revalidated with
`--locked` before completion. Historical T0 probes/reports stay in PR #53 and are
not copied as a substitute for production integration tests.
