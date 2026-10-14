# AML research execution archive

Issue #61 moves the finished AML research execution out of the active tool surface.
This is historical source preservation, not a new experiment, quality result,
official AML acceptance, or current performance gate. Frozen reports, corpora,
source references, and `evals/` bytes remain unchanged.

## Exact source group

[`manifest.json`](manifest.json) records the original paths, byte sizes, SHA-256
hashes, Git blob IDs, and local import dependencies of 18 Python files plus the
optional research dependency requirements. Every archived file is byte-identical
to its original at commit
[`44b74835928321482ca62fac0d16c716e0bb732b`](https://github.com/TheLudlows/Origence/tree/44b74835928321482ca62fac0d16c716e0bb732b).

The `tools/` directory is a complete local dependency group, including copies of
shared helpers and the drill client. It must not import active helper replacements
or be placed on the active tools' import path. Original module contents and their
repository-root assumptions are deliberately preserved. Do not invoke these files
in place and mistake an archive directory for a runnable current checkout.

The archived execution covers:

- Retrieval-quality and query-expansion experiments
- Answer, oracle, chat/BGE reranking, budget and temporal evaluation
- Search/vector/compaction profiling and maintenance-scale experiments

For historical source investigation, use a separate checkout at the recorded
commit. An intentional replay would also require its complete matching data,
binary, pinned model files, environment and original authorization. This archive
neither launches those processes nor authorizes spending model calls. Current
active tools never import this directory.

## Retained active tools

`tools/aml_drill.py`, its client validation, and the existing controlled host
recovery tests remain active and unchanged. Product API, protocol, identity and
source-provenance regressions remain in the normal test suite.

Offline diagnosis, budget summary, and budget/temporal/maintenance verification
remain active. Their helpers are now independent of the research runners:

- `aml_answer.py`: saved-answer validation and reference/citation metrics
- `aml_quality.py`: hashes, source validation and retrieval metrics
- `aml_experiment.py`: saved-response/input contracts and evidence selection;
  `RecordedChat.record` accepts a supplied response and performs no network IO
- `aml_cross_encoder.py`: saved-score, candidate and pinned-file validation;
  no model loader or inference implementation
- `aml_artifacts.py`: frozen data access, constants and strict source hashes
- `aml_report.py`: exclusive offline report creation, retaining partial evidence
  and finite error codes; also used by offline diagnosis
- `aml_budget.py`, `aml_evidence.py`, `aml_public.py`, `aml_temporal.py`: existing
  deterministic helpers retained

All pre-existing tools test cases remain. The former transport-fixture cases
exercise the extracted saved-response contract without any live transport. The
failed quality-run artifact case exercises the extracted exclusive report writer.
New regressions check archive hashes/import closure, absence of active research
entrypoints, and strict source-root behavior.

## Historical source verification

A frozen run can refer to older tool bytes than the source commit archived above.
In particular, the budget and temporal freezes predate later AML changes. Their
hashes are not rewritten to bless current helpers or these newer archived sources.

The verifiers accept `--source-root PATH`. For every frozen `tools/...` hash they
read exactly `PATH/tools/...`. Without that option, the source root is this archive.
Missing or mismatched bytes fail; there is no fallback to another checkout, the
active helper, or an older source. Frozen evidence and non-source inputs continue
to come from the current repository, even with a source override.

A complete source checkout at
[`2b7b6a67300d9e96eca8be0de282d308b8ded576`](https://github.com/TheLudlows/Origence/tree/2b7b6a67300d9e96eca8be0de282d308b8ded576)
contains the historical tool versions used by those freezes. Merely supplying a
commit label or a partial source tree is insufficient: every listed file hash is
still checked. The default archive is expected to fail those older source hashes.

Prepare the source-only historical worktree once (this does not execute models):

```sh
git worktree add --detach ../origence-aml-history 2b7b6a67300d9e96eca8be0de282d308b8ded576
```

Run from the current repository root, choosing output paths that do not exist:

```sh
python3 -B -m unittest discover -s tools -p 'test_*.py' -v
python3 -B tools/aml_diagnose.py --report /tmp/aml-diagnosis-new.json
python3 -B tools/aml_temporal_verify.py \
  --source-root ../origence-aml-history --output /tmp/aml-temporal-audit-new.json
python3 -B tools/aml_maintenance_verify.py \
  --directory evals/aml/maintenance-scale-v1 \
  --source-root ../origence-aml-history --output /tmp/aml-maintenance-audit-new.json
python3 -B tools/aml_budget_verify.py \
  --source-root ../origence-aml-history --output /tmp/aml-budget-audit-new.json
```

Full budget verification additionally requires `tokenizers==0.22.2` (the minimal
optional [`tools/requirements-aml-verify.txt`](../../tools/requirements-aml-verify.txt))
and the exact frozen `target/aml-rerank/model/tokenizer.json`, whose SHA-256 is
checked before use. It does not require PyTorch, Transformers, GPU inference or
chat credentials. Missing local tokenizer assets prevent that full audit; helper
unit tests are not a substitute. All three full audits also require the exact public corpus at
`target/aml-public/longmemeval_s_cleaned.json`, checked against
`evals/aml/public/preparation.json` (277,383,467 bytes, pinned source revision and
SHA-256 recorded there; see the [public-data record](../../evals/aml/public/README.md)).
Missing this corpus blocks full audits. Temporal and maintenance verification do not require the
tokenizer. The maintenance runner hash also predates the archived source snapshot;
use its matching historical source root rather than bypassing that check. Output creation stays exclusive, so previous evidence is
never overwritten.

The original larger `requirements-aml-rerank.txt` is preserved only in this
archive. No dependencies are installed, models fetched, or historical runs
restarted by archival or the normal test suite.
