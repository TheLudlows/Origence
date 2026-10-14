# Native Turso publication cost probe (temporary diagnostic bundle)

This separately reviewed temporary diagnostic bundle is staged under
`tools/publication-cost-probe`, then copied to `$RUNNER_TEMP` for its isolated
Cargo workspace/build/run. It is not a product workspace member, application
dependency, or no-JOIN lint exception. The historical `sql-evidence.json` is
generated only in the temporary directory from fixed read-only checkouts and
**must not be committed**. Current status: Python preparation checks passed;
**Rust build, formatting, Clippy, and all native measurements NOT_RUN**.

## Frozen comparison

- Baseline: main `1d5e0554b38e12df094a894f11a23fb26a117568`.
- Current: `765d67190b02e01bcf78a756a5ae185b1027971c`.
- Turso Rust SDK and every resolved Turso component: **0.8.2**.
- SDK default features disabled; no FTS, sync/remote runtime, Lance or model IO.
- Rust 1.98.0; release build; one current-thread Tokio runtime; one local file DB.
- Counts: **50 / 100 / 200**, each with **4096** f32 embedding components.
- Two warmups, then five measured repetitions. Rotate variant order each round.
- Each variant uses its own read transaction over the identical unchanged row.
  All its SQL is in that one snapshot; every result is drained before reuse.
- Capture raw samples, median, phase time, payload bytes, executed SQL count,
  returned text bytes, source/SQL/binary hashes, lock/features and runner details.
  The count increments at every timed query, including compact native DISTINCT.
- No source text, saved chunks object or embedding array is returned by the
  measured SQL. Returned text byte counts do not bound engine-internal memory.
- This is a normal-shape synthetic fixture, not a Worker recovery/quality test.
  Payload generation goes through f32 -> serde_json::Value -> String as production
  does; byte size therefore need not equal the older SQLite proxy's f64 fixture.

## Two separate scopes

### A. Metadata cost (small Turso-only crate)

`extract_sql.py` reads fixed-hash `publication.rs` from the two read-only source
checkouts. It does not modify either checkout. The generated `sql-evidence.json`
contains the historical SQL outside the product source/guard. Both the full
source hash and each selected SQL hash are recorded.

Baseline runs the **original whole first header SELECT** and the **exact original
expected materialized CTE**, changing only that CTE's terminal consumer to
`SELECT id,dimension FROM expected`. Current runs the actual header SELECT,
all eight-slot ID projections, compact native DISTINCT, and all eight-slot
ID/dimension projections. Native values/results are decoded and combined in
Rust. The returned header, raw distinct count, and normalized ID/dimension sets
must agree on every iteration, including warmups.

The observed timed query counts must be baseline 2 and current 16 / 28 / 52 for
50 / 100 / 200. These are asserted against the runtime counter; they are not
reported solely from a formula. BEGIN/COMMIT and fixture setup are excluded from
both the timer and that counter. The ID phase includes the initial header;
embeddings are evaluated for dimensions only after the ID phase.

This measures native SDK metadata SQL and its Rust result handling. It **does
not** measure the application's adapter wrapping, live authorization,
source/asset checks, index/ledger associations, full receipt latency, Service's
post-transaction native availability gate, network, models or polling loops.
Do not compare its median directly to a full API latency SLO.

A constants-only candidate may be added with `--candidate-root`,
`--candidate-commit`, `--candidate-sha256`. Its two Rust metadata-loop bodies must
be byte-identical to frozen 765d and its parameter contract unchanged; otherwise
extraction fails. A candidate that changes the algorithm requires a separately
reviewed runner before measurement. Never just relabel the old implementation
as a new candidate. At most one candidate is compared with the same baseline
and current variants, in the same process/environment with rotated order.

### B. Real production authority receipt call

`production/prepare.py` creates a new external Cargo workspace whose path
dependency is the selected Origence checkout with `default-features = false`.
It copies only the harness source, never modifies Origence, and records hashes
of all selected checkout Rust/SQL source plus Cargo files. On CI also verify the
actual checkout commit with `git rev-parse HEAD`; local reconstructed snapshots'
git HEAD is not a provenance substitute.

The production harness opens the real TursoStore/schema, provisions a workspace
and authenticated admin, seeds a completed publication, and calls actual
`TursoTx::message_batch_status` via public APIs. It measures begin + real method
+ commit, including live authorization and all authority tables. It asserts
Retrievable, then Invisible when one FTS ledger witness is absent, Retrievable
after restoration, and Invisible after root-source withdrawal. This is a real
production-method regression, not a rewritten status function. SQL counts for
this path are explicitly **not instrumented**, because the immutable production
adapter is not patched. Setup is outside timings; large JSON construction is on
a blocking thread outside transactions.

The initial production regression builds **current 765d only**; historical and
current metadata are both compared by the small SDK-only executable. A later
whole-authority comparison would need separate fixed-source binaries with
matching toolchain, release flags, counts, warmups and repetitions, and alternate
execution order. It is outside the initial benchmark batch. The no-default-feature dependency still compiles
Origence's non-vector library graph. It avoids Lance, rather than claiming to be
as small as the metadata-only executable. It never runs the Worker or any model.

## Local preparation (already permitted)

```sh
python3 -B -m unittest discover -s "$BENCH_DIR" -p 'test_*.py'
python3 "$BENCH_DIR/extract_sql.py" \
  --baseline-root "$BASELINE_CHECKOUT" \
  --current-root "$CURRENT_765D_CHECKOUT" \
  --output "$NEW_EVIDENCE_JSON"
python3 "$BENCH_DIR/production/prepare.py" \
  --checkout "$CURRENT_765D_CHECKOUT" --label 765d6719 \
  --output "$NEW_PRODUCTION_WORKSPACE"
```

The 13 preparation tests (5 extraction and 8 lock/feature gates) are Python-only. It does not execute SQLite and is not
Turso performance evidence. All output paths must be new. The source checkouts
are read-only inputs; generated Cargo locks belong to the external workspaces.

## Minimum Linux CI batch

CI copies the reviewed `tools/publication-cost-probe` bundle to
`$RUNNER_TEMP/bench`, outside product checkouts; record its exact source hash.
Do not place the historical SQL JSON in product source, add a lint exception,
or disable the product guard. The parent sequences this diagnostic batch after
the first audit CI batch. These steps describe the authorized diagnostic run;
this README is not a record that the workflow or native measurements ran:

1. Checkout exact main1d5e055 and 765d into two distinct read-only input paths.
   Verify each `git rev-parse HEAD`. Install existing Rust 1.98.0 toolchain with
   rustfmt and Clippy. Use the existing authorized Rust cache approach, keyed
   to the isolated manifest/lock/features; no protobuf/Lance setup is needed.
2. Run Python extraction tests and the frozen source extractor. Save the evidence
   JSON and source commit IDs outside the checkouts.
3. Have Cargo generate the isolated lock. If caret transitive resolution moves
   a Turso package, use Cargo's `update -p NAME --precise 0.8.2` for the affected
   Turso packages, highest-level SDK/sync SDK first, then their engine/parser/
   macro dependencies. **Do not handwrite/edit a lock.** Native measurements are
   blocked unless `verify_lock.py` proves all resolved Turso components are 0.8.2.
   Generate this lock once, preserve it, and use `--locked` for all later commands.
4. Record `cargo metadata --locked --format-version 1` and `cargo tree --locked
   -e features`, then run the lock/feature verifier. Verify no FTS/sync/default
   SDK feature and no Lance/Arrow graph.
5. Run Cargo fmt on the temporary copy only (preserve the actual patch), then
   Clippy `--all-targets -- -D warnings`,
   and a release build. Record source hashes after formatting, toolchain, OS,
   CPU, available memory and release environment variables. Do not compare
   debug and release or different engine-feature binaries.
6. Run `run_probe.py` for the metadata binary. It applies a 1200-second external
   process deadline and preserves stdout/stderr even on failure. This is process
   isolation for this experiment, not a claim about application future cancellation.
7. Generate/build the production workspace against **current 765d only**, with a
   Cargo-generated verified lock, and run its actual-method regression. This can
   follow the first Linux metadata result; Windows production regression follows
   in the later batch. Do not compile the full historical app in this first run.
8. Always upload evidence: bundle/source hashes, extracted SQL JSON, both real
   Cargo locks, Cargo metadata/features, rustfmt patches, toolchain/runner details,
   executable hashes, all raw JSONL samples and result.json (including failures).
   A timeout preserves completed samples and marks remaining work NOT_RUN.

Representative build/run commands after those inputs are provisioned:

```sh
cargo generate-lockfile --manifest-path "$BENCH_DIR/Cargo.toml"
# Use Cargo --precise updates only if necessary, then verify the actual result.
cargo metadata --locked --manifest-path "$BENCH_DIR/Cargo.toml" --format-version 1 > "$EVIDENCE/metadata.json"
python3 "$BENCH_DIR/verify_lock.py" "$BENCH_DIR/Cargo.lock" --metadata "$EVIDENCE/metadata.json"
cargo tree --locked --manifest-path "$BENCH_DIR/Cargo.toml" -e features > "$EVIDENCE/features.txt"
# BENCH_DIR is the temporary copy. Preserve diff against the staged source.
cargo fmt --manifest-path "$BENCH_DIR/Cargo.toml"
cargo clippy --locked --manifest-path "$BENCH_DIR/Cargo.toml" --all-targets -j 2 -- -D warnings
cargo build --release --locked --manifest-path "$BENCH_DIR/Cargo.toml" -j 2
python3 "$BENCH_DIR/run_probe.py" \
  --binary "$BENCH_DIR/target/release/origence-publication-native-bench" \
  --input "$NEW_EVIDENCE_JSON" --output "$EVIDENCE/metadata-run"
# Repeat lock generation/verification/build for the generated production manifest.
python3 "$BENCH_DIR/run_probe.py" --production \
  --binary "$NEW_PRODUCTION_WORKSPACE/target/release/origence-publication-production-probe" \
  --input 765d6719 --output "$EVIDENCE/production-current"
```

No native result exists at this preparation stage. The CPython SQLite proxy
only identified a possible regression and is not an acceptance result.
