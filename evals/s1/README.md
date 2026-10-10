# S1 retrieval quality baseline (dataset v1)

This is the first frozen, human-authored evaluation slice for S1. It uses the existing 12-document synthetic corpus and 100 manually written questions. It is a development baseline, not a representative production benchmark and not evidence of real-model quality.

## Annotation protocol

- Keep every document immutable within this dataset version. `source_id`, `source_version`, and `locator` form the stable evidence key; v1 `locator` is the whole synthetic source document.
- `relevant_source_ids` lists every source required to answer the question. A multi-source question is fully covered only when every listed source is retrieved.
- An empty `relevant_source_ids` means the corpus has no answer. Such cases are excluded from Recall/MRR/nDCG denominators and scored separately for false retrieval.
- `evidence_quote` is a verbatim excerpt from the cited source and gives reviewers a direct audit trail. It is not a generated answer or a model judgment.
- Queries are manually authored from the source text. Paraphrases are intentionally included, so this set can reveal lexical retrieval limits; they are not guaranteed to be answerable by every retrieval mode.
- Do not edit this dataset in place after a scored run. Create a new version, record the reason, and keep prior hashes and results.

## Frozen inputs

- `corpus.jsonl`: copied source documents, each version `1`.
- `cases.jsonl`: 100 questions with source-level evidence labels and verbatim evidence quotes.
- `manifest.json`: SHA-256 hashes and annotation scope.

The runner currently scores document-level evidence. It does not score exact claim support, answer generation, Agent task success, or Evidence Coverage under a shared token budget. The initial set has 12 source documents; the roadmap target remains 300 cases, with an independently held-out test split required before tuning or making quality claims.

## Reproduce

Build the release binary, then run the keyword baseline with the documented adapter. Vector/hybrid require an explicitly enabled embedding service and should record its model, version, dimension, and cost. Never treat a fixed-vector S0 regression model as a semantic-quality result.

```sh
python3 evals/run.py --binary target/release/origence --corpus evals/s1/corpus.jsonl --cases evals/s1/cases.jsonl --mode keyword --k 5 --commit 47d1375 --output target/evals/s1-v1-keyword
```

The output directory must not already exist. Retain the manifest, per-question results, imports, and summary together.

## First observation

On commit `47d1375`, the single local Windows keyword@5 run scored document Recall/MRR/nDCG `0.0104` (one of 96 answerable questions retrieved), with zero request errors and zero false positives on four no-answer questions. The old 24-question seed scored `0.6364` on the same host as a runner diagnostic; the case distributions differ, so those scores are not directly comparable. Full artifacts are under `results/keyword-47d1375/`. Treat the new result as a signal that keyword search struggles with these manually authored question forms, not as a production-quality estimate.
