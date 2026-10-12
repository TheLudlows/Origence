# T0 Turso probe preparation report

Date: 2026-10-12. Baseline: `d0521c0935083e71890ab02c100920c2edaa7227`.

## Result

**T0 is not complete. Turso compilation/runtime/FTS/recovery are NOT_RUN.**
The local preparation environment has Python but no `rustc` or `cargo`. A
separate draft-PR Linux run is required to generate the nested dependency lock,
compile the exact published candidate and produce executable engine evidence.
No production backend, dependency, database, default or protocol was changed.

| Local check | Result | Meaning |
| --- | --- | --- |
| Python probe unit tests | PASS, 5 tests | DDL splitting, fixture expectations, literal query compilation, constraint false-pass rejection and missing-trigger mutation |
| SQLite fixture execution | PASS, 17 cases | Includes open/close and deferred Learning FK; 47 original DDL statements and 188 SQL preparations |
| Native-only planned cases in SQLite run | NOT_RUN, 25 cases | Driver/FTS/reopen/9 process-kill+restore cases are not simulated |
| Failed-start reporting with `/bin/false` | PASS as harness negative test | Native report correctly has 1 FAIL and 41 NOT_RUN, exit 1 and bounded stderr artifact |
| Turso 0.8.2 build/fmt/Clippy | NOT_RUN | No local Rust toolchain |
| Turso native SQL/FTS/recovery | NOT_RUN | No compiled bridge |
| Production format-discriminator/public-service integration | NOT_RUN | Outside isolated probe implementation; explicit future gate |

Commands executed from the baseline checkout:

```sh
python3 -m unittest discover -s tools/t0-turso-probe -p 'test_*.py' -v
python3 tools/t0-turso-probe/verify.py --sqlite-baseline --output <new-fixture-output>
python3 tools/t0-turso-probe/verify.py --binary /bin/false --output <new-failure-output>
python3 -m py_compile tools/t0-turso-probe/verify.py
```

The failed-start case is a test of truthful reporting, not an actual Turso
failure. All 42 planned cases appear exactly once in either report. A future
case may explicitly report NOT_RUN when a write completes before the requested
cancellation point; successful rollback alone does not prove cancellation.

## Independent review

A separate read-only review identified and prompted fixes for: missing own-write
assertions; a one-row RETURNING fixture that did not leave unread rows;
pipe-write/lifetime deadlines; insufficient out-of-scope top-k competitors;
incomplete backup assertions; unrelated errors passing constraint checks;
startup/finalization/report loss; discarded stderr; unverified checkpoint
completion; and query escaping not tested against native FTS.

The follow-up review found **no remaining source-review blocker** in the prepared
Turso probe and independently reran the final checks above: five unit tests,
17 SQLite PASS/25 NOT_RUN, and the negative startup case's 1 FAIL/41 NOT_RUN.
The review included the deferred Learning FK, raw summary/graph recovery checks
and missing-immutable-trigger mutation. This clearance
does not assert that Rust compiles or that native engine behavior passes.

## Required next evidence

1. Authorized exact-head Linux CI, with Cargo-generated lock and resolved
   Turso core/sdk-kit versions. Import formatting/lock artifacts, then rerun
   the committed final head with `--locked`.
2. Inspect all actual failures; the existing recursive graph CTE is a known
   source-level adaptation gap, but has not yet been executed in this harness.
   The probe must retain that failure rather than silently skip the statement.
3. Evaluate FTS/main/job/ledger outcomes at all nine deterministic crash barriers
   and same-version stopped-directory restore. These are not arbitrary mid-I/O,
   mid-COMMIT/checkpoint/OPTIMIZE or power-loss evidence.
4. Resolve the remaining contract gates in
   [T0_STORAGE_CONTRACT.md](T0_STORAGE_CONTRACT.md), including global/scoped BM25,
   summary scoring, production format recognition and representative scale.

Any unresolved persistence, authorization or FTS recovery failure blocks default
switching. No draft PR or source-review result authorizes a merge or T1–T5.
