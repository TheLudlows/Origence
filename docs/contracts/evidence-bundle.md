# EvidenceBundle

**English** · [简体中文](evidence-bundle.zh-CN.md)

## Scope

This contract defines read-only context assembly over memory, knowledge, Learning,
and opt-in private session turns. Skill packages and Run traces have their own
access contracts. `Service::evidence_bundle`, HTTP
`POST /v1/context`, and MCP `context_bundle` share one request and response.
Search/resolve retain their separate contracts. Guidance remains external
evidence without an independent publication lifecycle. Current-stage databases are fresh databases per AGENTS.md.

Engineering coverage and Agent effectiveness are separate gates. Fixtures prove
only harness behavior. A live-model run is **NOT_RUN** until a verified runtime,
source bindings and any necessary provider/data/spending approval exist.

## Recall and authority

Memory, ordinary knowledge and Learning use independent current-chunk candidate
pools. Type, scope, exact identity and complete source visibility are checked
before branch caps and native vector top-k. One query embedding is shared by
all enabled content retrieval routes. Keyword uses the existing all-query-terms matching
rule; hybrid is deterministic RRF60. Each route has at most 100 candidates per
branch and an explicit returned candidate limit 1–100. Native content retrieval
uses original chunks. Search/resolve remain available for their existing components.

Optional exact memory identity applies only to memory, including supported
explicit conflict counterparts. Historical content item versions never become current
retrieval candidates. Session references still preserve the exact historical
versions actually used when archived.

In regular context reads, the memory route includes consolidated fact topics.
A normal question uses the [long-term lexical scan](memory-consolidation.md#统一召回)
with at most 32 query terms and 512 scoped fact rows. The route reports `scanned`
and `scan_exhausted`. `memory_identity` bypasses the scan and selects an exact
topic. Consolidated topics use the memory candidate limit before the remaining
slots go to memory chunks. A recalled consolidated topic takes precedence over
chunks with the same complete identity in that request. This exclusion holds
when the date's conclusion is conflicting or unknown, and when the topic is
omitted for budget. Chunks with other identities remain eligible. This lookup works independently of the selected
native keyword/vector mode. The final snapshot reopens each topic's latest
revision, checks every source and requires each ranking source to still belong
to that revision.

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

Content item citations include chunk, content item, and version, original source and source version,
version-root source event, and immutable locator. These three version/source
concepts are deliberately distinct. Session provenance is a tagged real
(session_id, turn_id, source_event_id) tuple with archived references, never a
fictional chunk. Citation IDs include both session and turn IDs.

`as_of_ms` selects business time in Unix milliseconds (default now).
`long_term_memory` provenance includes `memory_id`, `version`, `as_of_ms`,
`effective_from_ms`, the `state` at selection time, the `timeline` of conclusion
changes, and `citations`. Each citation includes `source_event_id`, the fact's
`effective_at_ms`, and a verbatim `statement` with half-open UTF-8 byte offsets.
`effective_from_ms` is `null` for an undated baseline or an `unknown` result.
`current` includes one value. `conflicting` and `unknown` carry no selected
value. A question with no visible matched topic yields no consolidated item.
Keyword matches have `unverified` applicability. An exact caller identity has
`caller_identity_exact_match`.

The console reopens the source event using writer-or-higher permission and
checks the byte range against the fixed quote before highlighting. A reader
can recheck the published excerpt by opening its pinned memory version. Each
open enforces current source visibility.

Learning is reader-visible only as its published statement/why, exact conditions
and sanitized support identifiers/ranges. Raw supporting Q&A and quotes require
writer authority and ownership of the private session's logical key. Conditions appear in retained text, item metadata and citation
provenance. Caller context uses exact string membership for every condition;
`caller_context_exact_match` means only that the caller supplied those strings.
Otherwise applicability is explicitly `unverified`. No model interprets,
broadens or infers applicability.

The current archive API accepts only chunk/entity/relation `used_evidence`.
Content citations in a bundle can be mapped to exact chunk references. Session-turn
citations cannot round-trip into that contract. The response, retained session
block and API documentation explicitly disclose this unsupported case; callers
must not invent chunk IDs or claim complete archive coverage.
Consolidated fact citations are marked `unsupported_long_term_memory_reference`
for that same archive contract. Run preparation rejects `as_of_ms` before
recording a snapshot or failure event. Use standalone context for dated memory.

## Conflict and budgeting contract

Only identical typed evidence locations coalesce, retaining keyword/vector
origins. Equal text at distinct locations/versions is never similarity-deduped.
Only exact explicit memory identities with a registered single-valued property and
confirmed/conflict semantic values qualify for deterministic conflict grouping.
Multi-valued facts, different contexts, negation, proposals, ambiguity and
unstructured prose are not inferred to conflict. Assessment is bounded to
recalled candidates. General memory/document/Learning prose contradictions are
`not_assessed`, not certified absent.

Each conflict group is one indivisible render unit containing all recalled
alternatives and its warning. If it does not fit, the whole group is omitted
and reported; one side is never rendered alone after a detected conflict.
Each consolidated topic is likewise one whole block covering its selection,
timeline and all citations. When the selected business time is conflicting, the
topic appears in `conflicts` if retained and counts toward
`omitted_conflict_groups` if skipped for byte budget. Conflicts for other times
remain visible through the topic's timeline when the topic fits.

`budget` and per-route `max_bytes` are actual UTF-8 bytes, not tokens. Defaults
are 4000 total bytes, 20 candidates per enabled route and 4000 bytes per route.
These neutral caps are not learned quality weights or reserved percentages.
Routes are processed in stable memory/knowledge/Learning/session round-robin
order; oversized units are skipped, so a later smaller unit may fit. All titles,
conditions, provenance, citation markers, conflict warnings and separators count.
Whole blocks only: no Unicode cutting, orphan citations or dropped warnings.
Response JSON outside `rendered_context` is not charged to the context budget.

## Independent Agent evidence

The `evals/agent` data, `evals/agent/runner.py`, and
`evals/agent/local_chat.py` harness use separately authored synthetic cases.
Four arms share model, prompts, tool permissions, model-call limits and context budget: no memory, simple hybrid (summary/graph disabled),
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
