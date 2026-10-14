# Temporary native Turso receipt-metadata diagnostic

This isolated probe is staged as `tools/publication-cost-probe` and copied to a
new temporary workspace by CI. It does not change product dependencies, product
workspace members, source checkouts, schema, or the no-JOIN guard. Historical
association SQL is extracted only into temporary evidence from immutable
read-only checkouts. Do not commit generated SQL evidence into the product.

## Current source-bound mode

- Historical main: `1d5e0554b38e12df094a894f11a23fb26a117568`.
- Historical eight-slot implementation: `765d67190b02e01bcf78a756a5ae185b1027971c`.
- Candidate: actual checked-out commit, supplied as its exact 40-character SHA.
- Reviewed candidate `src/storage/sqlite/publication.rs` SHA256:
  `1d56bbfcbc28793a0f603b9ce881e792aae6687786fb15e2619b3638c6fc2053`.
- Explicit mode: `reviewed_receipt_metadata_v1`; the old constants-only mode
  still rejects changed Rust flow. It is not repurposed to accept this algorithm.
- The extractor verifies the candidate's actual Git HEAD and committed source
  bytes, its whole-source hash, every extracted SQL literal, and hashes of the
  six relevant Rust sections. Its typed metadata section must match
  `src/receipt_types.rs` byte-for-byte. Source changes require another review.
- The standalone SDK probe explicitly uses serde/derive 1.0.229, already present
  in the product lock, to use that exact typed decoder. No alternate JSON parser
  or interpretation of native ID tokens is introduced.

## Same-process comparison and separate save costs

One small SDK process compares main / eight-slot / receipt summary over the
same persisted row, including the saved summary. Use native Turso 0.8.2, no SDK
default features, Rust 1.98.0 release, 50/100/200 chunks × 4096 f32 components,
2 warmups and 5 repeats. Rotate metadata variant order each round. Compare the
entire projected header, native DISTINCT count and normalized ID/dimension sets
on every round, including warmups.

The metadata timer includes native queries, typed receipt decoding, native token
validation, native DISTINCT and native ID normalization, plus Rust handling. It
excludes transaction begin/commit, all permission/source/version/index/ledger
checks and Service/Lance. It is a normal-shape metadata fixture, not a copied
implementation of every authority/status error branch. Every result is drained.
Returned text is O(total ID bytes + N); vectors are never returned by status SQL.
Actual query counters must be main 2, eight-slot 16/28/52, and summary 4.

Before that read comparison, first save and same-generation retry are each
measured separately for 2 warmups + 5 repeats at all three sizes. Both time the
complete adapted save helper: Value serialization, lightweight old metadata
lookup, native kind/count and single-relation json_each extraction, typed
comparison and atomic plan+metadata update. There are 4 executed statements per
nonempty save. Derivation has a separate included sub-timer. Bound text bytes,
returned text bytes and serialized plan bytes are reported. BEGIN/COMMIT and
fixture resets are outside these SDK save timers.

The retry reconstructs the actual saved plan with supplied receipt_metadata
between timers, reading back only the small field. Thus the retry timer includes
serialization of that field and both retained/supplied equality checks, as in
the Worker's saved-plan path. Derivation is never hidden in fixture setup.

All three metadata variants run this same new runner and same payload. Do not
calculate speed ratios against previous diagnostic runs or full API timings.

## Actual candidate API regression

`production/prepare.py` creates a separate temporary path-dependency workspace
pointing only at the exact candidate checkout. `--label` must be its full commit;
HEAD and the reviewed committed publication source are verified. The probe uses
real TursoStore, real save_publication and real message_batch_status. It times
first save, saved-plan replay, and receipt read separately, including begin/commit.
It checks persisted summary existence, byte-equal replay, rejection of changed
same-generation membership, Retrievable status, missing full-text witness,
witness restoration, and source withdrawal. This is actual authority API
coverage, not Service/native availability gate, Worker, model or Lance coverage.
Its SQL counts are uninstrumented and reported as null. It does not compare the
historical full API or claim an end-to-end speed ratio.

## CI integration

1. Keep exact historical read-only checkouts for main and 765d. Copy this probe
   outside all source checkouts. Record actual source Git commits and hashes.
2. Generate summary evidence with extract_sql.py's existing baseline/current
   arguments, plus --candidate-root ACTUAL_CHECKOUT, --candidate-commit EXACT_SHA,
   --candidate-sha256 the reviewed hash above, and
   --candidate-mode reviewed_receipt_metadata_v1.
3. Run all Python preparation tests. Generate Cargo locks with Cargo, pin every
   Turso component to 0.8.2 using Cargo as needed, then retain/use --locked.
   Verify lock, cargo metadata/features, matching serde 1.0.229, no Lance/Arrow,
   and Turso default/FTS/sync/mimalloc features disabled. Preserve real locks.
4. Preserve original source hashes; run rustfmt in the temporary workspace and
   archive its patch, then Clippy with warnings denied and release builds.
   The extracted typed source is reviewed before formatting; formatting is not
   permission to change its decoder or the source-binding checks.
5. Preserve the 4 historical JSONB correctness tests. Generate their old frozen
   input with prepare_candidate.py and constants-only extract_sql.py, but do
   not run old JSONB performance again. Set PUBLICATION_PROBE_JSONB_SQL to that
   old evidence and PUBLICATION_PROBE_SQL to summary evidence, then run all
   native tests. Alternatively run each named test group separately with its
   matching evidence, recording both outcomes. Missing evidence must fail.
6. Run run_probe.py against summary evidence (2 warmups / 5 repeats, external
   process deadline). It requires all metadata and both save matrices to finish.
7. Prepare/build/run the production harness against the exact candidate SHA,
   also with generated/verified lock. No crate renaming or dual-version app
   process is used. Complete read/save/replay samples are required at all sizes.
8. Always retain complete JSONL/stdout/stderr, failure/timeout status, hashes,
   source evidence, locks/features, toolchain/OS/runner, and format patches.

## Current validation and historical evidence

Local Python preparation: 24 tests pass; all Python modules parse. This local
environment has no Cargo/rustfmt, so native compilation, Clippy, native tests and
performance are NOT_RUN here and must be established by the exact CI commit.
No local performance conclusion is asserted.

The previous JSONB candidate's correctness passed but performance failed. At
200 chunks its same-run metadata medians were main 223.205 ms, eight-slot
4868.480 ms, JSONB 7374.514 ms. The separate actual-765d authority API median
was 5822.794 ms, a different phase and scope. These numbers are historical only.
The previous preparation text is preserved in README-jsonb-history.md; it is
not the current execution recipe or a claim that the old candidate is untested.
