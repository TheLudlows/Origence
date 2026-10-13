# Issue #52: staged Turso FTS T0

At source preparation: **native validation NOT_RUN**. Exact-head results and
artifact hashes will be recorded in [PR #53](https://github.com/TheLudlows/Origence/pull/53)
and [issue #52](https://github.com/TheLudlows/Origence/issues/52), without a report-only rerun.

The user selected single-table Turso FTS candidates followed by batched
application authority checks and bounded refill. This report concerns that new
raw-chunk probe, not production integration. The frozen contract is
[section 8](T0_STORAGE_CONTRACT.md#8-staged-single-table-fts-follow-up-authorized-2026-10-13).

Historical exact head `380fedeffad2425c96715994dfdecd4c27e54f6c` remains intact:
[CI 38340711565](https://github.com/TheLudlows/Origence/actions/runs/38340711565).
Its correct dense CTE cost around 20 seconds; forcing scored-first order made the
single authorized point about 22.24% slower. Neither result is a reason to
continue JOIN tuning. Original schema/raw-JOIN failures and all original probes
remain available in [the FTS cost report](t0-fts-cost-report.md) and artifacts.

## New validation scope

- 60 configurations over 1k/10k synthetic raw-chunk corpora, two paired orders
- Single-table scoped FTS, native BM25, deterministic paging and exact filtered top-K
- Snapshot-local batched authority, whole-version support checks, fresh final validation
- Explicit candidate/row/call exhaustion and hard whole-process deadline
- Separate full-scan comparator and fixture-truth ranking oracle
- Focused ties, deep-filter, budget, snapshot and revocation cases

No new native result exists at this source snapshot. Unit/source review and later native stages
will be distinguished as PASS / FAIL / NOT_RUN / RESOURCE_EXHAUSTED. Summary AND
aggregation and full Context route/identity/generation/ledger/Learning semantics
remain NOT_RUN in this slice. No production default, migration or merge is made.

ANN source and its fixed contract are unchanged. A new FTS-only head may skip ANN
when conservative changed-input selection proves its execution inputs unchanged;
that means ANN was not rerun at that new head, not a transferred CI pass. The
complete ANN evidence remains tied to `380fede` and its own artifacts.

## Evidence boundaries

The native focused set is candidate/authority-row/native-call budgets,
insert/update/delete snapshot observations, and fresh key/root revocation.
Explicit adversarial equal-score ties, exact 512/513 candidate boundaries,
malformed/duplicate JSON, nonmatching sibling support, and empty/short results
are initially SQLite harness tests using fabricated FTS values. Natural ties
and filtering in the native 60-case matrix are recorded separately; harness
passes must never be relabeled as native adversarial passes.

Source preparation checks passed: 52 aggregate Turso Python tests (18 staged,
including complete 60-case fabricated-FTS driver acceptance and failed-evidence
mutation tests), and 25 conservative CI-selector tests. Independent source and
workflow reviews found no remaining blocker. Native execution is still pending
for the new exact head; these counts cannot establish Turso index/transaction
behavior. No isolated or root Cargo dependency changed.
