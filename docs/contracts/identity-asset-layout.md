# Asset-level identity storage (#60, first batch)

Baseline: main `4c0862e80ccfb8b403268170c06d391239790d2b`.
This batch implements the identity merge only. The [complete baseline field
inventory](storage-field-audit-2026-10-15.md) records retention decisions;
[index/ledger analysis](index-ledger-audit-2026-10-15.md) explains why those two
relations remain separate. Unread ledger fields remain review candidates, not
approved deletions. No migration, old-schema fallback, or automatic repair is added.

## Persisted model

`oc_assets.identity_key` and `identity_json` are nullable independent columns.
Both must be NULL, or both non-NULL on a memory asset. The scoped unique
`oc_memory_identity_slot` index includes deleted rows. `oc_memory_fact_slot`
and structured assets' `fact_key` remain unchanged: the normal writer assigns
the same canonical key to both fields, preserving legacy endpoint collisions.

The old `oc_memory_identities` relation, its foreign-key association, duplicate
scope/asset identifiers and creation timestamp are removed. Creation is a single
asset INSERT. There is no independent identity update/delete API. Identity stays
with its asset through historical-version restoration; deletion leaves the key
occupied. Semantic restore restrictions, source retraction, permission checks,
publication, command idempotency and retry/CAS behavior are unchanged.

Authority storage has 23 physical tables including its format marker; graph
storage still has 3, for 26 physical tables total. The format marker remains in
each database. Full token-preserving DDL admission, not just the marker version,
rejects the previous layout read-only without modifying it. Same-format reopen
and whole-store backup/restore remain supported.

## Read and error contract

The user approved stronger rejection of inconsistent stored identities for this
batch. Selected reads validate the paired columns, memory kind, typed JSON,
business-field validity and the canonical scope-bound key/body agreement.
No JSON or key repair is attempted. Malformed identities return Unavailable;
native column type failures retain Backend. Missing identities remain absent.

- identity_slot selects by validated requested key and memory kind, decodes the
  existing body, and compares it to the request before checking deleted. A body
  mismatch remains Conflict, including on tombstones; a matching tombstone is
  the existing deleted-identity Conflict.
- Legacy slot remains a legacy-only endpoint. A typed structured body produces
  the existing endpoint Conflict before tombstone/hash checks; it never reuses,
  renders, or accepts a corrupted structured identity.
- identity_view checks the binding before asset visibility, preserving its
  Unavailable mismatch behavior. Candidate filtering excludes deleted/non-memory
  rows before decoding.
- Context identity batches return raw asset columns. Route membership precedes
  decoding, and unidentified rows still support suggested-identity evidence.
- Retrieval reuses already-batched asset rows, but decodes identity only after
  current/deleted and first semantic-status checks. Batches remain bounded by
  the existing read-model limit; no JOIN, correlated association, or N+1 is added.
- Property registration still checks identity predicates including tombstones;
  its existing json_extract error behavior and same-transaction serialization
  remain unchanged.

No public API/CLI signatures are removed. The intentional observable change is
stricter corrupt-row rejection and incompatible old-layout rejection, not a
change to normal identity, visibility or restore behavior.

## Review and validation

The design was independently reviewed before implementation. Regression covers
paired-null/memory constraints, tombstone uniqueness, cross-scope isolation,
legacy collisions, deferred corruption decoding, >128-key batches, missing
schema/index/column admission, old-layout read-only rejection and backup/history
preservation. Existing service/public-library/HTTP/semantic/recovery suites remain
required. Actual execution evidence is recorded in [VALIDATION](../VALIDATION.md).

The ignored `identity_layout_benchmark::identity_layout_before_after` test uses
isolated historical/merged native Turso fixtures, never a production old-schema
path. It measures 1k/10k identical inputs, 100 operations per transaction,
creation for all rows, reuse/binding-lookup over at most 1,000 evenly spaced keys,
explicit statement counts, timings and post-shutdown
DB/WAL/SHM bytes. Both variants run in the same release executable/configuration. Phase/batch progress
is printed, with logging overhead included in timing. The measurement has an
independent bounded CI job; it does not extend the fast-check path.

Expected statement counts per operation are create 2 reads + 2 writes → 2 reads
+ 1 write, reuse 3 reads → 1, and binding lookup 2 reads → 1. These are layout-path
counts, excluding BEGIN/COMMIT, setup and common asset_view/publication work.
Input hashing/new rendering hash checks are excluded, so timings are not a full
API performance claim. Fixed-order single samples carry cache/order noise; there
are no timing thresholds or production SLO claims. CI retains raw output as an
artifact for the exact tested revision.


The follow-up ignored `identity_layout_creation_diagnostics` experiment isolates
creation stages and the added paired-null/memory-only CHECK in test fixtures.
Its merged fixture preserves the production CHECK condition order; the fixture
without that added CHECK is diagnostic only, never a proposed production schema.
Repeated reordered runs reduce fixed-order bias without claiming statistical
confidence. Query-plan output is diagnostic and outside timed creation. The
existing historical before/after measurement is retained unchanged. CI explicitly
runs the creation diagnostic while investigating the measured regression; the
7-minute measurement limit remains unchanged. Production changes require evidence
and separate review; functional CI success does not accept the performance tradeoff.
