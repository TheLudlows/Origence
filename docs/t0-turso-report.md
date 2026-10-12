# T0 Turso probe preparation report

Date: 2026-10-12. Baseline: `d0521c0935083e71890ab02c100920c2edaa7227`.

## Result

**T0 is not complete. Initial native evidence exists and further diagnostics
are pending.** [Draft PR #53](https://github.com/TheLudlows/Origence/pull/53)
bootstrap head `8d0af2136e95331eebcfa0e1e7422992d41e8d3d` compiled and passed
warnings-denied Clippy on Rust 1.98.0. Its
[native run](https://github.com/TheLudlows/Origence/actions/runs/38257392122)
reported **37 PASS, 4 FAIL, 1 NOT_RUN**. The Cargo-generated lock resolves Turso,
core and SDK-kit components to **0.8.2**. See the preserved
[bootstrap evidence](evidence/2026-10-12-t0-turso-bootstrap.json).

- All 47 original DDL statements created successfully; 188 real SQL strings
  prepared. Deferred FK, JSON, composite constraints, graph cascade, explicit
  rollback, RETURNING cleanup, SDK drop/reuse and snapshots passed.
- FTS physical-index/AND/literal escaping, summary term union, own/cross-connection
  visibility, rollback, update/delete/optimize and standalone positive BM25 passed.
- All nine deterministic 999/1000/1001-row process-kill and stopped-directory
  restore cases passed, covering chunks/summaries, both FTS indexes, graph,
  jobs and ledger. This does not prove arbitrary power-loss safety.
- Two strict schema-text checks failed because Turso canonicalizes the `key`
  identifier as `"key"`. Old string equality would reject its own new database;
  no schema corruption is established. The strict failures remain in the matrix;
  a separate narrowly scoped equivalence/structural reopen candidate is added.
- Two trigger-category checks failed despite the expected writes being rejected
  with the exact intended messages: the SDK classifies RAISE(ABORT) as a generic
  error. Follow-up behavior assertions accept only exact fixture-owned trigger
  messages and still require unchanged business rows and mutation-test rejection.
- The original scoped-FTS test returned the correct eligible ID with **score 0**.
  Its old candidate-only assertion was insufficient. Stronger follow-up tests
  require the correct winner among two eligible rows and the same positive score
  as direct BM25; they isolate parameter identity and raw/plain/materialized-CTE
  joins. [Upstream #7532](https://github.com/tursodatabase/turso/issues/7532)
  documents this filtering-versus-scoring distinction. It is unresolved here.
- The once-polled disk write completed before yielding, so actual pending-future
  cancellation remained NOT_RUN. A separate SDK memory-yield VFS unit test is
  added to force that observation without substituting for disk recovery.

The local preparation environment still has no `rustc` or `cargo`; execution
uses the authorized isolated Linux workflow. Final exact-head, committed-lock
validation is pending, and original engine failures are not erased.
No production backend, dependency, database, default or protocol was changed.

## Local preparation evidence (before native bootstrap)

| Local check | Result | Meaning |
| --- | --- | --- |
| Python probe unit tests | PASS, 5 tests | DDL splitting, fixture expectations, literal query compilation, constraint false-pass rejection and missing-trigger mutation |
| SQLite fixture execution | PASS, 17 cases | Includes open/close and deferred Learning FK; 47 original DDL statements and 188 SQL preparations |
| Native-only planned cases in SQLite run | NOT_RUN, 25 cases | Driver/FTS/reopen/9 process-kill+restore cases are not simulated |
| Failed-start reporting with `/bin/false` | PASS as harness negative test | Native report correctly has 1 FAIL and 41 NOT_RUN, exit 1 and bounded stderr artifact |
| Turso 0.8.2 build/fmt/Clippy | Initially NOT_RUN | Superseded by the remote bootstrap evidence above; final head pending |
| Turso native SQL/FTS/recovery | Initially NOT_RUN | Superseded by the remote bootstrap results above |
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

The follow-up diagnostic review additionally found that whitespace/case folding
inside quoted SQL literals could accept a changed default. The isolated
comparator now preserves quoted tokens and rejects `pending` → `PENDING` and
quoted-space mutations. Seven unit tests pass; the expanded SQLite fixture
reports 20 PASS / 28 native-only NOT_RUN across 48 planned cases. Production
normalizers are not modified as part of this probe.

## Required next evidence

1. Authorized exact-head Linux CI, with Cargo-generated lock and resolved
   Turso core/sdk-kit versions. Import formatting/lock artifacts, then rerun
   the committed final head with `--locked`.
2. Inspect all actual failures; the existing recursive graph CTE prepared
   successfully in the bootstrap, so older unsupported-feature assumptions
   must not substitute for executing its actual directed/cyclic/scoped behavior.
3. Evaluate FTS/main/job/ledger outcomes at all nine deterministic crash barriers
   and same-version stopped-directory restore. These are not arbitrary mid-I/O,
   mid-COMMIT/checkpoint/OPTIMIZE or power-loss evidence.
4. Resolve the remaining contract gates in
   [T0_STORAGE_CONTRACT.md](T0_STORAGE_CONTRACT.md), including global/scoped BM25,
   summary scoring, production format recognition and representative scale.

Any unresolved persistence, authorization or FTS recovery failure blocks default
switching. No draft PR or source-review result authorizes a merge or T1–T5.
