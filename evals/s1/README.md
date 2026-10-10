# S1 retrieval quality baseline (dataset v1)

This page documents the S1 v1 dataset inputs and annotation protocol. Current measured results are in the [S1 v2 report](results/local-77d43a8-v2/README.md). These synthetic datasets are development material, not representative production benchmarks.

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

The runner currently scores document-level evidence. It does not score exact claim support, answer generation, Agent task success, or Evidence Coverage under a shared token budget. The v2 study uses 300 cases with an independently held-out split.

## Reproduce

Build the release binary, then run the keyword baseline with the documented adapter. Vector/hybrid require an explicitly enabled embedding service and should record its model, version, dimension, and cost. Never treat a fixed-vector S0 regression model as a semantic-quality result.

```sh
python3 evals/run.py --binary target/release/origence --corpus evals/s1/corpus.jsonl --cases evals/s1/cases.jsonl --mode keyword --k 5 --commit 47d1375 --output target/evals/s1-v1-keyword
```

The output directory must not already exist. Retain the manifest, per-question results, imports, and summary together.
