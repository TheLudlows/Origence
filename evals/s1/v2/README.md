# S1 retrieval experiment v2

Frozen before retrieval inference: 300 questions (200 development, 100 held out),
36 synthetic source documents, 288 answerable and 12 no-answer questions.
Every answerable question has source/version/whole-document locator and verbatim
supporting quotation(s). Gold sources for the held-out split are disjoint from
those for development, while both splits search the same 36-document corpus.
`manifest.json` fixes hashes, model revisions, k=5, UTF-8 byte budget=2000 and the
six configurations before scoring. Do not change queries or gold labels after a
run; revise the dataset version if a label correction is necessary.

The original 100 manually labelled v1 questions are preserved. The additional
200 questions and 24 synthetic sources are assistant-authored and checked
against their source text; independent human review has not been performed.
This is a synthetic engineering benchmark, not representative production data
or a certified independently human-annotated benchmark. The held-out split is
independent of development sources and retrieval tuning, not of its author.

## Configurations and metrics

One fresh Origence publication is shared by keyword, vector, hybrid original
chunks only, hybrid + summary, hybrid + graph, and hybrid + summary + graph.
All configurations use the same corpus, embedding, candidate cap, k and context
budget. Import enables real embedding and extraction once; component switches
only affect query branches. The graph-only configuration isolates graph benefit
from summary benefit. Keyword/vector ignore the hybrid-only components.

The reports retain raw search and resolve responses, imports, dataset hash,
model file hashes, CPU runtime versions, generated projection responses and
per-question metrics. `summary.json` reports both splits separately as well as
categories, document Recall/MRR/nDCG@5, exact quotation evidence Recall@5,
Coverage@2000 bytes, fully covered question fraction, no-answer false retrieval,
search/resolve p50/p95/p99 and model usage. Graph provenance alone does not count
as a verbatim original quotation. These metrics do not certify semantic
entailment, citation correctness of generated answers, or Agent task success.

Local provider fee is zero; CPU cost is unpriced and not asserted to be zero.
Import usage is separate and query usage is recorded per configuration. No
fixed-vector regression model is used for semantic scoring. No embedding-score
abstention threshold is tuned on held-out questions; vector retrieval can thus
return irrelevant documents for questions without an answer. Report this failure
rather than interpreting zero request errors as successful abstention.

## Reproduce

Optional Python model dependencies do not change application Cargo dependencies:

```sh
python3 -m venv /tmp/origence-model-env
/tmp/origence-model-env/bin/pip install torch==2.14.1+cpu --index-url https://download.pytorch.org/whl/cpu
/tmp/origence-model-env/bin/pip install -r evals/requirements-local.txt
cargo build --locked -j 2
HF_HUB_DISABLE_XET=1 /tmp/origence-model-env/bin/python evals/local_experiment.py \
  --binary target/debug/origence --model-dir /tmp/origence-models \
  --download-models --dataset evals/s1/v2 --commit "$(git rev-parse HEAD)" \
  --output /tmp/origence-s1-v2-run
```

The loopback server and evaluation host run in one process tree. Runtime model
loading uses local files only. The optional download step fetches the exact
snapshots named by the dataset manifest. Output must be a new directory.
Real extraction is prepared offline before application import. The local provider
uses greedy float32 CPU generation, a two-pass JSON schema (entities, then
relations whose endpoints are constrained to generated names), and at most eight
items per array. Both raw stages and their prompts are retained; these constraints
do not establish semantic correctness. Results are cached by full prompt, model
revision and inference policy, so no ablation gets a different summary or graph.
Preparation time/usage is retained separately; import timing excludes generation. Embeddings are not
cached. Timing is a single sequential CPU run, not a production latency SLO.

The legacy 24-query/12-document seed is still frozen in `evals/corpus.jsonl` and
`evals/retrieval.jsonl`; its eight keyword misses are recorded separately with
the seed result. Its score cannot be compared directly to this different corpus
and question distribution.

## Model implementation references

- [BGE model card](https://huggingface.co/BAAI/bge-small-zh-v1.5): CLS pooling, normalization and symmetric no-instruction policy.
- [Qwen model card](https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct): local Transformers inference.
- [LM Format Enforcer](https://github.com/noamgat/lm-format-enforcer): JSON-schema token constraints. Constrained shape and endpoint consistency do not certify factual support.

This 300-question retrieval slice does not replace the full platform protocol's
memory lifecycle, Agent tasks, governance/fault cases, repeat-run uncertainty or
production workload evaluation. Those remain separate S2–S4/operations work.

## Executed results

[Local CPU experiment at 77d43a8](../results/local-77d43a8-v2/README.md) retains the six-configuration results, development/held-out breakdown, paired ablation deltas, all failures, independent quotation/budget audit and compressed raw responses with SHA-256. Additional labels still need independent human review.
