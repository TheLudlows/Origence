# S4 bounded Agent comparison

**Live model/product comparison: NOT_RUN.** The generated local fixture run is a deterministic
harness check only. It does not establish an Origence retrieval result, model
quality, task-completion improvement, semantic entailment, or production readiness.
No paid API, remote model, model download, or existing user-data mutation was used.

## Frozen scope

`freeze.json` hashes `corpus.json`, `cases.json`, `labels.json`, and `config.json`
before any real inference. The ten new author-written synthetic tasks cover release
planning, cross-session handover, an updated decision, conditional learning (positive
and negative), unresolved conflict, revoked support, no answer, environment/entity
scope, and external guidance treated as data. They are not reused observed S1/AML
questions and have not received independent human review. A scripted fixture run is
not an unseen-quality evaluation. Changes after observing live results require a new
dataset/version; never rewrite an old result or its freeze.

The four arms share one Agent prompt, model/revision/generation policy, action schema,
call ceilings, output token cap, and context ceiling:

- `no_memory`: the retrieval tool returns no evidence.
- `simple_hybrid`: actual `/v1/search`, hybrid originals only (summary/graph off).
- `evidence_bundle`: actual `/v1/context`, memory/knowledge/learning/session routes.
- `gold_diagnostic`: oracle-selected frozen raw evidence; an upper-context diagnostic,
  not a deployable retriever. Answer aliases and rubrics never enter model prompts.

The model chooses each `retrieve(query)` or `finish(answer, citations)` action. It sees
the real tool observation before making the next choice, so this is a bounded Agent
loop, not a saved-answer proxy. Every attempt, including errors, consumes its budget.
A case can use at most four model calls, three tool calls, 512 output tokens per model
call, 24,000 serialized UTF-8 context bytes, and 7,000 evidence-observation bytes.
Evidence-bearing observation bytes include the model-facing wrapper. Empty tool
bookkeeping consumes the context budget but no evidence budget. UTF-8 bytes are not
exact model-token counts. Per-route candidate limits are not a global retrieval-work
budget; model calls and embedding costs must be reported separately.

All model inputs, outputs, tool raw responses, delivery/omission decisions, counters,
latencies, source bindings, and errors are retained. Model usage is `null` when not
reported completely; unpriced compute/total costs remain `null`. Local provider fees
may be zero only for a completed explicit local run. Each artifact records input,
configuration, code, prompt, schema, model snapshot, and binding hashes. Arm order
rotates across cases; this small sequential sample does not support p99/SLO claims.

## Run the harness without a model

From the repository root (Python standard library only):

```sh
python3 -m unittest discover -s evals -p 'test_*.py' -v
python3 tools/s4_agent.py --mode plan --output target/s4-plan
python3 tools/s4_agent.py --mode fixture --output target/s4-fixture
```

Output directories must not already exist. `plan` explicitly writes `NOT_RUN`.
`fixture` uses a label-blind scripted adapter and illustrative in-memory retrieval;
it validates plumbing, bounds, scoring and archival only. The fixture's numeric
metrics are intentionally not described as Agent or product quality.

The existing `evals/local_models.py` maps JSON-object chat to graph extraction. Do
not modify or use that chat route for S4. `tools/s4_local_chat.py` is a separate,
generic JSON-schema chat adapter with unchanged system/user messages, constrained
real generation, greedy CPU inference, no cache, no remote-code trust, and existing
local safetensor snapshots only. Optional runtime packages follow the repository's
pinned `evals/requirements-local.txt`; no package/model installation happens here.

## Live prerequisites (not executed)

A live run requires explicit authorization for local inference and all of:

1. An existing local model snapshot, model name, immutable revision, runtime, and
   model-file hashes. The adapter does not download anything.
2. A fresh isolated Origence database/workspace containing only this synthetic corpus.
   The runner is read-only and does not seed or mutate a service. Use the documented
   public `/v1/knowledge`, identified/semantic memory, SessionTurn, and Improve APIs in
   a separately authorized preparation step. Preserve original corpus text, versions,
   conditions, source lifecycle and source-to-product bindings. The session source
   includes exact `session_question`/`session_answer` seeding fields. Learning must be
   genuine condition-preserving Learning, not a knowledge document relabeled by this
   adapter; its source statement is the frozen content and its conditions are exactly
   `environment=staging` and `schema=7`. Save the actual Conditions/Statement/Why
   wrapper's content SHA after seeding. Revoke `revoked-code` before evaluating.
3. A writer-or-higher token scoped to that synthetic workspace, because the session
   route is explicitly requested. Keep the token only in `S4_ORIGENCE_TOKEN`; it is
   never written to reports. No archival or feedback writes happen during evaluation.
4. A prepared binding file, with the exact frozen-data hash, product commit/binary hash,
   and local embedding model/revision/file hashes. Every current source must be bound.
   Every product chunk/turn requires its exact current content SHA, source event and
   immutable coordinates; unbound or revoked evidence fails the case rather than
   silently entering the prompt. Learning/session wrappers are verified and retained.
5. Origence configured to use that same local embedding endpoint, without remote
   fallback. The operator must verify this configuration; the API does not expose the
   host's secret configuration. The runner checks the declared literal-loopback
   endpoint's actual `/metadata` against the binding before retrieval and archives
   before/after usage. This metadata check is not proof of the host's private config.

Minimal binding shape (use real verified coordinates/hashes, never these placeholders):

```json
{
  "synthetic_only": true,
  "fresh_database": true,
  "freeze_sha256": "SHA256_OF_FREEZE_JSON",
  "workspace_id": "WORKSPACE_UUID",
  "product_provenance": {
    "commit": "40_HEX_COMMIT",
    "binary_sha256": "64_HEX_BINARY_HASH",
    "embedding_base_url": "http://127.0.0.1:8098",
    "embedding_model": "PINNED_LOCAL_MODEL",
    "embedding_revision": "IMMUTABLE_REVISION",
    "embedding_files_sha256": {"config.json": "64_HEX_FILE_HASH"}
  },
  "sources": [
    {"type":"asset_chunk","source_id":"release-approval","asset_id":"ASSET_UUID","asset_version":1,"chunk_id":"CHUNK_UUID","source_event_id":"EVENT_UUID","content_sha256":"64_HEX_CONTENT_HASH"},
    {"type":"session_turn","source_id":"handover-primary","session_id":"SESSION_UUID","turn_id":"TURN_UUID","source_event_id":"EVENT_UUID","content_sha256":"64_HEX_CONTENT_HASH"}
  ]
}
```

After those prerequisites and approval, the explicit local-only commands are:

```sh
S4_ALLOW_LOCAL_MODEL_CALLS=true python3 tools/s4_local_chat.py \
  --allow-local-inference --model-path /approved/existing/snapshot \
  --model MODEL --revision REVISION --port 8099

S4_ALLOW_LOCAL_MODEL_CALLS=true python3 tools/s4_agent.py --mode live \
  --allow-local-model-calls --chat-url http://127.0.0.1:8099 \
  --model MODEL --model-revision REVISION --origence-url http://127.0.0.1:8080 \
  --bindings /approved/synthetic-bindings.json --output target/s4-live-new-run
```

Literal loopback HTTP only; proxies, redirects, URL credentials/query secrets, and
remote endpoint URLs are refused. HTTP operations have a timeout and bounded reads;
this is not a hard model-process cancellation guarantee. Failed readiness/approval
leaves `NOT_RUN`. Interrupted execution after attempts is not a completed comparison.

The simple baseline receives only actual returned product fields, not frozen oracle
conditions or locators. EvidenceBundle's own `rendered_context` is passed unchanged as
one untrusted observation, preserving conflict groups and source references. It is
kept whole or omitted whole if the remaining evidence/context budget cannot hold it.
Later retrievals request a smaller bundle using the remaining evidence budget. The
renderer allowance reserves 512 bytes for wrapper/warnings; the actual observation
is measured before delivery. Any remaining excess is omitted, never truncated.

## Score interpretation and denominators

Every frozen case appears in every arm, including failed requests. Missing/duplicate
rows are rejected. Answerable errors count as zero success and zero required-source
coverage. No-answer errors are errors, never successful abstention. No-answer cases
are excluded only from required-source-coverage denominators (they require zero
sources), not task-success denominators.

- `task_success_heuristic`: expected answer aliases, all required cited source IDs,
  valid delivered citations, and no forbidden answer string; or valid abstention for
  a no-answer task. This may accept incorrect extra claims and miss valid paraphrases.
- `citation_source_support_heuristic`: cited IDs mapping to required source IDs divided
  by all cited IDs, including invented IDs. It is source relevance, **not entailment**.
- `required_source_coverage`: required sources cited through actually delivered IDs
  divided by all required sources. Retrieval/delivery is tracked separately from use.
- Missing citations have a separate count. Citation precision is `null` when no
  citations exist, and a factual uncited answer cannot pass task success.
- Independent human review / semantic entailment: `NOT_EVALUATED`.

Read `traces.jsonl.gz` with `gzip.open(path, 'rt', encoding='utf-8')` to inspect every
raw turn. Reports contain only the synthetic corpus; do not reuse their public
artifact policy for private data. The plan and fixture are engineering evidence only. Raw run artifacts are retained locally; publication is pending explicit approval and they are not included in this PR. Reproduce them with the commands above into a new output directory.
