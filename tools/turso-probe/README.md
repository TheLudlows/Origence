# Turso Database compatibility probe — Issue #52 / T0.1

Status: **implementation draft; runtime results are not yet established**.
This standalone Rust test package does not change Origence's default storage,
root Cargo dependencies, production schemas, APIs, or existing data.

## Pinned input

- Origence starting point: `d0521c0935083e71890ab02c100920c2edaa7227`.
- Turso Database v0.8.2: annotated tag resolved to commit
  `5c168be5ef6a32ac966daf5983502a4f694b37ad`.
- Rust: 1.98.0, matching the application's declared toolchain.
- The Rust SDK uses `default-features = false` and the `fts` build feature.
  FTS tests also explicitly call `experimental_index_method(true)`.
- LanceDB remains unchanged. This probe does not test ANN.

The package has its own workspace so investigating Turso does not add it to
Origence's dependency graph or change the root Cargo.lock. The first CI run
bootstraps a lock using Cargo and archives it. **That generated lock must be
reviewed and committed before this probe is accepted.** CI deliberately fails
its acceptance gate while the lock is untracked. Subsequent runs use `--locked`.
Never fabricate a lock or silently upgrade the pinned engine.

## First slice: ten executable probes

| Probe | What it checks |
| --- | --- |
| real_base_and_graph_ddl_commit_and_reopen | Directly includes the repository's base and graph DDL; creates, commits and reopens a fresh database |
| blob_json_and_epoch_round_trip | Binary UUID-shaped values without text coercion; UTF-8, JSON, boolean and millisecond timestamp round-trip |
| real_workspace_ownership_trigger_rejects_update | The actual immutable-owner trigger is enforced, not merely parsed |
| real_graph_foreign_key_cascade | Actual graph DDL and cascading edge deletion |
| relation_and_graph_writes_share_rollback | Relation and graph rows can participate in the same rollback |
| json_table_function_materialized_cte_and_returning | JSON iteration/projection and materialized CTE shape used by publication checks; RETURNING drained before reuse |
| dropped_returning_can_be_rolled_back_and_connection_reused | Explicit rollback after abandoning result iteration |
| fts_and_multilingual_commit_update_delete_reopen | Composite BLOB identifiers, whitespace FTS, AND, Chinese/Latin identifiers, committed DML, optimize, and clean reopen |
| fts_rollback_does_not_publish_or_remove_committed_text | Aborted insert/update does not change committed text or FTS results |
| fts_scope_filter_before_limit_and_score_observation | A simple SQL scope filter cannot lose its only match behind other scopes' top hits; logs synthetic BM25 values |

FTS score diagnostics are observations, not a frozen relevance policy. The
upstream documentation's score-direction prose and DESC examples conflict;
verify actual score ordering with this pinned engine before implementing T3.
The simple scope probe is **not** proof of Origence's full source/version,
identity, Learning, or post-filter refill semantics, nor a query-plan/performance
proof. The FTS fixture uses a composite-BLOB-key test table; tests on actual
`oc_chunks` and `oc_summaries` with complete production joins are still required.

## Run

```sh
cd tools/turso-probe
# Bootstrap once only if no lock is present, then review and commit it.
cargo +1.98.0 generate-lockfile
cargo +1.98.0 fmt --all -- --check
cargo +1.98.0 clippy --locked --all-targets -- -D warnings
cargo +1.98.0 test --locked --test contracts --no-fail-fast -- --nocapture --test-threads=1
```

All test data is synthetic and held in new `tempfile` directories. No application
CLI or existing data directory is opened. The direct SDK is used; SQL is not sent
through a hidden shell protocol. The CI workflow is Linux-only, read-only, has a
35-minute job ceiling and two Cargo jobs, and does not restore macOS or Docker CI.
CI records the exact checked-out revision, environment, Cargo lock, dependency
metadata, lint/test logs and any rustfmt patch as an artifact. It cannot write or
merge repository changes. A timeout is a failed job, not a successful probe.

## Remaining T0 gates — do not mark T0 complete

- Review the generated lock and formatting changes, then rerun exact-head CI.
- Probe identity, semantic, session and Learning DDL and complete domain SQL,
  including owned-workspace root/capability constraints and partial indexes.
- Verify actual chunks/summaries FTS joins, execution plans, scoring direction,
  query escaping, snapshot visibility and complete candidate filtering.
- Verify SDK transaction-drop/cancellation, reader snapshots, writer conflicts,
  and data-format recognition/incompatible-directory rejection.
- Add process-kill/restart and same-version stopped-host backup/restore evidence;
  a clean close/reopen here does not establish crash or power-loss durability.
- Freeze ANN metric/normalization, exact baseline, per-filter recall and latency
  budgets, index maintenance and new-data visibility. No ANN claim is made here.
- Record exact run/commit evidence in docs/VALIDATION.md and synchronize STATUS
  and the implementation plan only after the result is known. Preserve historical
  evidence; do not mark unexecuted or blocked checks as passing.

Only after T0's durability, authorization and FTS recovery gates pass may T1
switch the relational adapter. No data migration, old-schema upgrade, MVCC rollout
or second Worker is part of this slice. Existing data must never be overwritten
or silently converted. A supported new Turso database must still reopen normally.

## Primary references

- Plan: https://github.com/TheLudlows/Origence/issues/52
- Repository constraints: ../../AGENTS.md
- SDK: https://github.com/tursodatabase/turso/blob/5c168be5ef6a32ac966daf5983502a4f694b37ad/bindings/rust/src/lib.rs
- SDK query and batch semantics: https://github.com/tursodatabase/turso/blob/5c168be5ef6a32ac966daf5983502a4f694b37ad/bindings/rust/src/connection.rs
- FTS syntax/known integration limits: https://docs.turso.tech/sql-reference/functions/fts
