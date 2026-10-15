# Frozen publication-cost diagnostic archive

Archived for Issue #61 from main
`75c238ffa830d50840d4b84c2dce6d7d69a4793c`. This is historical diagnostic
material, **not a current tool, product workspace member, or ongoing CI job**.
The 19 files originally under `tools/publication-cost-probe/` are preserved
byte-for-byte, including the README, manifest, Python preparation/lock verifier,
Rust source and tests. `SHA256SUMS` inventories all 19 original files (including
`manifest.json`); this note and the inventory are new archive metadata.

The original README's “current” and `NOT_RUN` statements refer to its preparation
point. They have deliberately not been rewritten into current validation claims.
The old manifest's source hashes and native status also remain historical; they
are not hashes or results for the Issue #61 cleanup. The old staged path in those
files is retained for provenance. Start here rather than treating those files as
current operational instructions.

## Immutable source and historical results

- [Complete original bundle at the archived main commit](https://github.com/TheLudlows/Origence/tree/75c238ffa830d50840d4b84c2dce6d7d69a4793c/tools/publication-cost-probe).
- [Original diagnostic workflow at that commit](https://github.com/TheLudlows/Origence/blob/75c238ffa830d50840d4b84c2dce6d7d69a4793c/.github/workflows/publication-cost-diagnostic.yml).
- [Historical validation record, including failed JSONB and successful receipt-summary runs](https://github.com/TheLudlows/Origence/blob/75c238ffa830d50840d4b84c2dce6d7d69a4793c/docs/VALIDATION.md#pr-59-后续审计与轻量回执摘要).
- [Initial native comparison run 38531694088](https://github.com/TheLudlows/Origence/actions/runs/38531694088)
  and [failed JSONB performance run 38534537552](https://github.com/TheLudlows/Origence/actions/runs/38534537552).
- Receipt-summary run for exact candidate
  `1e64ae50b8a3153a83368fac553a8bf5949a21e0`:
  [run 38541994079](https://github.com/TheLudlows/Origence/actions/runs/38541994079),
  [metadata artifact 11882214092](https://github.com/TheLudlows/Origence/actions/runs/38541994079/artifacts/11882214092),
  [complete artifact 11883362035](https://github.com/TheLudlows/Origence/actions/runs/38541994079/artifacts/11883362035),
  and [the exact run's workflow](https://github.com/TheLudlows/Origence/blob/1e64ae50b8a3153a83368fac553a8bf5949a21e0/.github/workflows/publication-cost-diagnostic.yml).
- Subsequent formatted-source revalidation:
  [run 38543901622](https://github.com/TheLudlows/Origence/actions/runs/38543901622),
  [metadata artifact 11883508259](https://github.com/TheLudlows/Origence/actions/runs/38543901622/artifacts/11883508259),
  [complete artifact 11884776018](https://github.com/TheLudlows/Origence/actions/runs/38543901622/artifacts/11884776018).

The repository bundle does **not** contain generated Cargo locks, extracted SQL,
raw measurements, or native binaries. The historical validation record locates
those in Actions artifacts, together with source hashes, features and format
patches. Their continued download availability has not been verified by this
archive operation. If expired/unavailable, report the missing evidence; do not
invent replacement locks or label new measurements as the old results. The
preserved `verify_lock.py` and its tests are lock-verification code, not a saved
resolved lock or evidence of a new native run.

## Historical reproduction only

Use an isolated temporary workspace outside every product checkout. For the
receipt-summary run above, obtain clean, read-only input checkouts at these exact
Git commits and verify each with `git rev-parse HEAD`:

- baseline: `1d5e0554b38e12df094a894f11a23fb26a117568`
- eight-slot comparison: `765d67190b02e01bcf78a756a5ae185b1027971c`
- receipt-summary candidate: `1e64ae50b8a3153a83368fac553a8bf5949a21e0`

For exact historical reproduction, copy the bundle from the **candidate commit's**
`tools/publication-cost-probe/`, and follow that commit's workflow linked above.
Do not silently substitute this archive snapshot or today's application source
for the run's original bundle/candidate. Keep the full candidate SHA and reviewed
source hash checks intact. Recover both real locks and run metadata from the
original artifacts when available; retain them and use `--locked`. If locks must
be regenerated with Cargo, verify all Turso components and features using the
preserved verifier and record the result as a **new dependency resolution/run**,
not exact reproduction of the saved run. Never hand-edit locks.

The workflow records the complete commands for the two SQL evidence modes,
production harness preparation, lock/feature verification, isolated formatting
and patches, strict Clippy, release builds, native tests, and separate metadata /
first-save / same-generation-retry measurements. Preserve Rust 1.98.0, native
Turso 0.8.2, the 50/100/200 × 4096 fixture, two warmups and five repetitions,
rotating comparison order and external process deadline. Missing historical
inputs or source-binding mismatch must fail closed. A replay is a newly dated
observation, never a retroactive update to the historical result.

Historical association SQL must only be generated and executed in that isolated
diagnostic workspace. Do not install this bundle back into active `tools/`, add
it to product build/CI, or introduce a SQL guard exception. `archives/` is outside
the guard's existing active Rust/Python scan paths; no guard source, tests, or
baseline were changed for this move. Archived SQL is not approved product SQL.

## Archive integrity and preparation-only check

From the repository root:

```sh
(cd archives/publication-cost-probe && sha256sum -c SHA256SUMS)
python3 -B -m unittest discover -s archives/publication-cost-probe -p 'test_*.py' -v
```

Local archive verification on 2026-10-14: all 19 original files matched the
source snapshot byte-for-byte, all 18 entries in the unchanged original manifest
matched, and all 24 Python preparation tests passed from the archived path.
Cargo/rustfmt were unavailable; no Rust compilation, Clippy, native correctness,
performance, or new lock verification run is claimed by this archive check.
