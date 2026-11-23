# Evaluation entry point

**English** · [简体中文](README.md)

`cases.jsonl` defines a frozen sample format, not a completed competitive benchmark. Lifecycle and security gates are executable in `tests/local_app.rs` through `tests/local.rs`. Retrieval samples seed future real frozen datasets. Rankings with mocked embeddings do not count as real semantic quality.

As evaluation grows, report retrieval Recall@k and source accuracy, leakage of unpublished artifacts and cross-tenant/workspace leakage, version conflicts and non-resurrection after deletion, actual Agent citations and task completion, p50/p95/p99, and model costs separately. Average relevance cannot compensate for failing correctness gates.

Product comparisons with other memory and knowledge platforms are pending. Performance and comparative conclusions need hardware, dataset version, model settings, commands, and raw results.

The current host uses SQLite authority and AnySeek full-text, vector, and one-hop graph projections. See the [documentation index](../docs/README.en.md). Protocol peers and model stubs establish engineering behavior only and generate no retrieval quality score.

## HTTP retrieval evaluator

`run.py` uses the actual host HTTP API. It creates a temporary data directory and workspace, imports knowledge, waits for published, searches, and deduplicates by source document for scoring. By default it disables all model calls and reads no existing service or data directory.

```sh
python3 -m unittest discover -s evals -p 'test_*.py' -v
python3 evals/run.py --binary target/release/origence --mode keyword --k 5 --commit COMMIT --output target/evals/keyword-run
```

The first `corpus.jsonl` includes 12 synthetic documents and `retrieval.jsonl` has 24 examples covering exact terms, paraphrases, same-name scope distractors, multiple sources, negation, and unanswerable questions. These are runnable seeds. They are neither the full 300 cases nor completed quality results. The original `cases.jsonl` preserves lifecycle and security cases and cannot be fed directly to the retrieval runner.

Reports contain a manifest, imports, raw per-question responses, and a summary. They map source IDs to current asset IDs and score document-level Recall@k, MRR@k, and binary relevance nDCG@k. Search requests at most 100 chunks and takes the first k deduplicated documents, which is distinct from chunk recall or unbounded retrieval. All required sources enter the denominator. Unanswerable questions are excluded from Recall. A failed answerable request scores zero and a failed unanswerable request is recorded as an error, not a correct rejection. Successful request latency and all request errors are reported separately. Small-sample p99 does not establish production performance.

Vector/hybrid requires explicit `--allow-model-calls`, `OC_ENABLE_MODELS=true`, and complete model settings. Calls may incur charges. The old seed defaults to no model. S1 v2 used real local models, with a provider fee of 0, unpriced CPU, and separately measured usage. S1 v2 toggles summary and graph independently and runs a six-configuration matrix across the same published artifacts. [S1 v2](retrieval/v2/README.md)

The output path must not exist, preserving original results. Output can include authorized corpus text and omits API and model keys. CI uses public synthetic corpora for uploaded artifacts. Reports on real enterprise data must be stored according to their authorization scope and cannot use public CI artifacts. Fixtures verify adapters and metrics only, with separate acceptance for the Rust host or semantic outcomes.

## Historical S1 experiments

[S1 v2 results](retrieval/results/local-77d43a8-v2/README.en.md) include 300 questions across six configurations, an independent source holdout, real BGE/Qwen settings, archived raw responses, a failure list, and an independent audit. Results apply to the report's pinned historical commit and backend, and do not represent current AnySeek quality. The 200 additional questions have not had independent human review, and false retrieval on unanswerable questions remains known. Full-platform, Agent, and production acceptance are separate.
