# S4: opt-in EvidenceBundle and bounded Agent comparison

## Scope

This is an additive read-only context layer. `Service::evidence_bundle`, HTTP
`POST /v1/context`, and MCP `context_bundle` share one request and response.
Existing search/resolve, storage backends, migrations, publishers, workers and
AML experiments are unchanged. Guidance remains external evidence, not a new
lifecycle. Current-stage databases are fresh databases per AGENTS.md.

Engineering coverage and Agent effectiveness are separate gates. Fixtures prove
only harness behavior. A live-model run is **NOT_RUN** until a verified runtime,
source bindings and any necessary provider/data/spending approval exist.

## Recall and authority

Memory, ordinary knowledge and Learning use independent current-chunk candidate
pools. Type, scope, exact identity and complete source visibility are checked
before branch caps and native vector top-k. One query embedding is shared by
all enabled asset routes. Keyword uses the existing all-query-terms matching
rule; hybrid is deterministic RRF60. Each route has at most 100 candidates per
branch and an explicit returned candidate limit 1–100. This version deliberately
uses original chunks only, with no summary or graph branches. Search/resolve
remain available for their existing components.

Optional exact memory identity applies only to memory, including supported
explicit conflict counterparts. Historical asset versions never become current
retrieval candidates. Session references still preserve the exact historical
versions actually used when archived.

Session is off by default and requires live writer-or-higher permission before
any model IO. It is bounded recent relational lexical recall, not an embedding
index. At most 512 scoped active-source rows are inspected, ordered by creation
time descending and stable session/turn IDs. Full archived evidence/transitive
Learning visibility is checked before the retained turn limit. A lookahead row
reports `scan_exhausted`; older matching turns may exist. Hidden turns may use
the scan window, never the retained-result limit. This is disclosed, not claimed
as exhaustive history search.

All returned candidates are rebuilt in a fresh authorized relational snapshot
after embedding/native IO, then rendered while the snapshot remains open.
Commit rechecks the live role. Withdrawal before that snapshot hides evidence
before asynchronous cleanup. Snapshot visibility is neither factual truth nor a
claim of linearizability with a withdrawal committed after the read snapshot.

## Typed provenance and applicability

Asset citations include chunk/asset/version, original source and source version,
version-root source event, and immutable locator. These three version/source
concepts are deliberately distinct. Session provenance is a tagged real
(session_id, turn_id, source_event_id) tuple with archived references, never a
fictional chunk. Citation IDs include both session and turn IDs.

Learning is reader-visible only as its published statement/why, exact conditions
and sanitized support identifiers/ranges. Raw supporting Q&A/quotes remain
writer-only. Conditions appear in retained text, item metadata and citation
provenance. Caller context uses exact string membership for every condition;
`caller_context_exact_match` means only that the caller supplied those strings.
Otherwise applicability is explicitly `unverified`. No model interprets,
broadens or infers applicability.

The current archive API accepts only chunk/entity/relation `used_evidence`.
Asset bundle citations can be mapped to exact chunk references. Session-turn
citations cannot round-trip into that contract. The response, retained session
block and API documentation explicitly disclose this unsupported case; callers
must not invent chunk IDs or claim complete archive coverage.

## Conflict and budgeting contract

Only identical typed evidence locations coalesce, retaining keyword/vector
origins. Equal text at distinct locations/versions is never similarity-deduped.
Only exact explicit S2 identities with a registered single-valued property and
confirmed/conflict semantic values qualify for deterministic conflict grouping.
Multi-valued facts, different contexts, negation, proposals, ambiguity and
unstructured prose are not inferred to conflict. Assessment is bounded to
recalled candidates. General memory/document/Learning prose contradictions are
`not_assessed`, not certified absent.

Each conflict group is one indivisible render unit containing all recalled
alternatives and its warning. If it does not fit, the whole group is omitted
and reported; one side is never rendered alone after a detected conflict.

`budget` and per-route `max_bytes` are actual UTF-8 bytes, not tokens. Defaults
are 4000 total bytes, 20 candidates per enabled route and 4000 bytes per route.
These neutral caps are not learned quality weights or reserved percentages.
Routes are processed in stable memory/knowledge/Learning/session round-robin
order; oversized units are skipped, so a later smaller unit may fit. All titles,
conditions, provenance, citation markers, conflict warnings and separators count.
Whole blocks only: no Unicode cutting, orphan citations or dropped warnings.
Response JSON outside `rendered_context` is not charged to the context budget.

## Independent Agent evidence

The new `evals/s4` data and `tools/s4_*` harness are separate from AML and its
observed questions. Four arms share model, prompts, tool permissions, model-call
limits and context budget: no memory, simple hybrid (summary/graph disabled),
EvidenceBundle, and explicitly nondeployable gold diagnostic. A bounded
model/tool loop records raw actions, observations, final answer, citations,
errors, assigned-task denominators, token usage, latency and unknown costs as
unknown. Fixed synthetic labels are never supplied to non-gold prompts.

Dedicated generic local JSON chat handling avoids the old local model server's
graph-specific JSON dispatch. A deterministic fixture run is engineering evidence
only. A live answer-model pilot or heuristic substring proxy cannot establish
reproducible Agent benefit. Paired repeated outcomes, independently reviewed
citation support and explicit cost/latency evidence are required before that
claim. No paid provider, model download or private-data transmission is implied
by this implementation.
