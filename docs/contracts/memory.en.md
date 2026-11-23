# Memory identity and semantics

**English** · [简体中文](memory.md)

This document defines explicit memory domain rules. See [controlled extraction](semantic-capture.en.md) for bounded model extraction, [API](../API.en.md) for interfaces, and [storage invariants](storage.en.md) for physical identity reservations. A valid source, valid syntax, and matching version do not establish truth.

## Identity and update rights

| Field | Rule |
| --- | --- |
| scope | tenant/workspace are authentication-derived. The authorized principal differs from the described subject. |
| subject | kind + stable_id. Names, aliases, and similarity are not unique identities. |
| predicate | A controlled property managed by the workspace. Valid syntax does not indicate registration. |
| context | Explicit applicability conditions, canonically encoded and exactly matched. Different conditions such as production and test stay separate. |
| asset_id | Stable asset ID. Identity binding is immutable and reserved after deletion. |
| source/evidence | Original event, exact quote/locator, parser, and source version |
| expected_version | Concurrent write condition. It does not establish truth, validity period, or authority. |

[MemoryIdentity](../../src/memory_identity.rs) encodes version, scope, subject, predicate, and sorted context in a fixed JSON tuple, then hashes with SHA-256 and prefix `memory-identity:v1:`. It does not fold case or Unicode or include body, principal, or version preconditions.

Semantic similarity grants no right to overwrite, bind aliases, or merge subjects. `fact_key` has a separate entry point and cannot silently change a bound structured asset.

## Property registry

Administrators register predicate, single/multi, and required_context in a workspace. Definitions are immutable. Equivalent definitions can replay, but different definitions are rejected. A new registry entry cannot take over a predicate used by any identity, including unpublished or deleted ones. Registered properties use the semantic endpoint and cannot be changed through old identified/capture paths. New databases include the complete registry schema. Missing tables or incompatible databases are rejected and upgrades are unavailable.

## Explicit writes

Input includes original content, optional identity, operation, value, and a verbatim quote and half-open UTF-8 byte range. Exact equality does not fold synonyms or Unicode. Source checks establish provenance, without proving classification.

- `assert` creates a fact if no current value exists. An identical value is idempotent. A different value for a single-valued fact retains conflict evidence.
- `update` is single-value only, takes the current expected_version, and appends a version.
- `supplement` is multi-value only and computes a union. Duplicates add no version. Each member retains its original source and span.
- `negate/propose/ambiguous` or absent identity produces independent unnormalized evidence without changing the current fact.
- Multiple distinct values in single-value input retain conflict. Unknown properties and missing required conditions are rejected.

Writes require expected_version, with 0 for no publication. Determinate facts recheck at acceptance and Worker publication. Version races become superseded. Unnormalized evidence never updates a suggested identity and its input version is not an update precondition for that identity. Persist source, idempotent response, asset, and job in one transaction. Retry reuses the durable plan.

## Publication, retrieval, and withdrawal

Determinate facts reuse the identified asset. Uncertain evidence uses an independent memory asset with no fact_key or identity. A chunk retains status and original member sources and byte ranges. search/resolve return normalization_status. Exact identity filtering limits current facts before top-k and rechecks sources, tombstone, version, and permissions before return.

Multi-value facts publish one complete version. Withdrawing the current operation source or any earlier member source hides the full snapshot, without automatic historical fallback. Retained members cannot be reattributed to a new source. If the current publication source is still valid, explicit supplement can rebuild the still-supported set. If that publication source is invalid, resubmit facts and do not backfill from history. Generic restore cannot bypass semantic writes. Historical text and locators are unchanged.

See the [Learning contract](learning.md) for conditions and support chains and [EvidenceBundle](evidence-bundle.md) for retrieval budgets, deterministic conflicts, and session-reference limits. Engineering results are in [VALIDATION](../VALIDATION.en.md). This contract does not track milestones or historical counts.
