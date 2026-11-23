# S1 real local-model retrieval and independent ablations (2026-10-10)

**English** · [简体中文](README.md)

The engineering retrieval experiment and evidence archive are complete. Independent human review of additional labels is pending. This result does not establish the full 300-case platform protocol, Agent success rate, or production quality.

Code `77d43a859d9c83c93d3a88d4fb0c7928a10d4f16`, tree `6420a9d8e001d469c890266092731d1531130e11`. Local Linux, Cargo Debug, actual HTTP host, and new temporary workspaces. Across all six configurations, 1,800 search and 1,728 resolve calls returned 0 request or resolve errors. Results belong to this code SHA. Later documentation and audit-archive commits did not rerun models.

36 synthetic documents and 300 questions (development 200 / held out 100, answerable 288 / no answer 12). Gold sources are disjoint across splits, but both search the same 36-document corpus. Original 100 questions retain human labels. The additional 200 were assistant-authored and checked against source text without independent human review. Data and settings were frozen before scoring. Held-out scores were not used to edit labels or tune thresholds.

BGE-small-zh-v1.5 (512 dimensions, CLS/L2, symmetrical without instructions) and Qwen2.5-0.5B-Instruct (float32 CPU, greedy). See [environment](environment.json), [models](models.json), and [frozen dataset](../../v2/README.md) for full revisions, file hashes, dependencies, binary hashes, hardware, and commands.

## Same-corpus comparison

Recall/MRR/nDCG/evidence and coverage below use only 288 answerable questions as denominator. False retrieval uses the 12 unanswerable questions. k=5 is deduplicated by source document. B=2000 UTF-8 bytes, independent of precise model-tokenizer counts. Evidence Recall verifies current source versions and exact quotations. Coverage counts only rendered quotations. Graph provenance is not original-text support.

| Configuration | Document/evidence Recall@5 | MRR@5 | nDCG@5 | Coverage@2000B | Unanswerable false retrieval |
| --- | ---: | ---: | ---: | ---: | ---: |
| keyword | 0.35% | 0.0035 | 0.0035 | 0.35% | 0/12 |
| vector | 97.57% | 0.9161 | 0.9314 | 98.61% | 12/12 |
| hybrid-original | 97.57% | 0.9161 | 0.9314 | 98.61% | 12/12 |
| hybrid-summary | 97.57% | 0.9161 | 0.9314 | 98.61% | 12/12 |
| hybrid-graph | 94.79% | 0.7793 | 0.8211 | 95.66% | 12/12 |
| hybrid-all | 94.79% | 0.7793 | 0.8211 | 95.66% | 12/12 |

Independent ablations show +0.00 percentage points in Recall from summaries and -2.78 percentage points from graph relative to raw text alone. Prefer raw text vector retrieval for this configuration and reassess graph expansion after improving extraction validity and candidate ranking. This decision applies to this extractor and dataset. API defaults remain compatible and independent switches let callers select validated settings.

## Development and held-out results

| Configuration | Development Recall / Coverage | Held-out Recall / Coverage | Development / held-out false retrieval on unanswerable questions |
| --- | ---: | ---: | ---: |
| keyword | 0.52% / 0.52% | 0.00% / 0.00% | 0/8 / 0/4 |
| vector | 96.35% / 97.92% | 100.00% / 100.00% | 8/8 / 4/4 |
| hybrid-original | 96.35% / 97.92% | 100.00% / 100.00% | 8/8 / 4/4 |
| hybrid-summary | 96.35% / 97.92% | 100.00% / 100.00% | 8/8 / 4/4 |
| hybrid-graph | 92.19% / 93.49% | 100.00% / 100.00% | 8/8 / 4/4 |
| hybrid-all | 92.19% / 93.49% | 100.00% / 100.00% | 8/8 / 4/4 |

## Single-run latency and cost

Values are in ms. p50/p95/p99 come from one sequential CPU run, including host HTTP and storage search and excluding offline projection generation. They do not represent concurrency or production SLOs. Six configurations ran in a fixed order, without repeated randomized runs to eliminate cache and runtime effects.

| Configuration | search p50 / p95 / p99 | resolve p50 / p95 / p99 | Query embedding requests |
| --- | ---: | ---: | ---: |
| keyword | 9.33 / 23.12 / 43.70 | 9.63 / 23.53 / 59.22 | 0 |
| vector | 332.82 / 569.19 / 750.35 | 334.74 / 539.49 / 806.49 | 588 |
| hybrid-original | 315.35 / 531.00 / 903.56 | 315.13 / 536.73 / 1163.71 | 588 |
| hybrid-summary | 327.64 / 597.61 / 739.76 | 326.04 / 634.46 / 1034.36 | 588 |
| hybrid-graph | 331.18 / 423.99 / 529.73 | 329.70 / 404.78 / 633.76 | 588 |
| hybrid-all | 329.95 / 541.38 / 776.40 | 329.82 / 516.67 / 777.25 | 588 |

Offline preparation generated 72 real projections (36 summaries and 36 graphs) in 1998.83 s of cumulative service time with 10981 / 8723 model input/output tokens. Importing 36 documents later took 11.97 s, hit 72 projection cache entries, and generated 36 text embeddings. Import time excludes offline generation and does not represent full ingestion cost. See comparison for embedding time and tokens per configuration. Queries do not generate summaries or graphs.

Local provider fee = 0 USD. CPU cost is unpriced and total cost is not asserted to be zero. The models are reproducible small local configurations. JSON constraints and matching endpoints do not prove graph facts. Small-model projections and short synthetic texts limit generalization.

## Failures and follow-up decisions

- The original 24-question/12-document keyword seed reran at 63.64% on the same code, with 0 errors and 0 unanswerable false retrievals. The same eight misses are retrieval-15 through retrieval-22. [Failed questions and frozen hashes](legacy-seed-failures.json) This seed differs from the 36-document/300-question distribution, so scores do not establish a regression.
- Keyword hits exact-number questions only. `keyword_hits` requires query tokens to be a subset of the document token set. Interrogative terms and paraphrases can prevent recall. Test Chinese query parsing and FTS/BM25 on development, and keep the holdout for acceptance.
- Vector/hybrid returns candidates for all unanswerable questions. Candidates do not establish an answer. Calibrate abstention and evidence thresholds in a separate validation. Do not add thresholds after seeing the holdout to raise scores.
- Summary and graph gains are isolated in [paired question deltas](paired-deltas.json). Enable them based on this extractor and frozen set. A low gain in one arm does not invalidate graphs for all tasks.
- With 36 documents, this run provides no need for ANN, so ANN is deferred. Experiment with reranking separately after increasing hard candidates and budget, without preemptively adding it.
- See the [failure list](failure-analysis.json) for misses, budget omissions, and unanswerable false retrieval. Real graph and summary outputs are preserved without score-driven edits.

## Verification and raw evidence

The [independent audit](audit.json) recomputed document and exact-text metrics per question. It checked rendered citations, UTF-8 byte caps, source versions, and no sideband from disabled components, then compared overall and split scores. All six matched. Default lib29/local89, light lib28/local72, Clippy, fmt, HTTP smoke, and Python9 passed. Both actual jobs from the source [PR CI run 38034197676](https://github.com/TheLudlows/Origence/actions/runs/38034197676) passed. Other main-only jobs were skipped, so this run does not claim new S1 cross-platform results.

The [raw result archive](raw-results.tar.gz) contains all question-level search/resolve, six manifests and summaries, imports, offline projection prompts and raw outputs from both stages, usage, model/runtime metadata, and the 24-question rerun. [SHA-256 manifest](sha256.json) verifies every entry and the archive. No API key or enterprise data is present. After extracting, run the following.

```sh
tar -xzf evals/retrieval/results/local-77d43a8-v2/raw-results.tar.gz -C /tmp
python3 evals/audit_s1.py --run /tmp/run --dataset evals/retrieval/v2
```

See the frozen dataset README for full model commands and installation, and environment for original commands and dependency lock. Repeated generation can differ with CPU or dependency floating-point behavior. No cross-hardware bit equality is claimed.

Implementation reference for later FTS/BM25 experiments. [SQLite FTS5 documentation](https://www.sqlite.org/fts5.html) Chinese parsing and tokenization need project-specific validation. Full-text indexing does not automatically handle paraphrases or evidence abstention.
