# Run verification records (P0, #89)

**English** · [简体中文](run-verification.zh-CN.md)

Baseline: `293e2bf6af3b180d43c8050731047b552ac07519`.

The first release records **owner-transcribed external assessments** only. It does
not implement a platform verifier or authenticate an external verifier. The server
records the authenticated credential principal and logical owner, receipt time,
and an exact immutable Run prefix (sequence and SHA256). `verifier_label` and
`assessed_at_ms` are owner claims; client `reporter=user/test` is not an authenticated
role. A verifier label grants no Run, session, source or workspace access.

`POST /v1/runs/{run}/verifications` accepts a strict independent DTO:
`verification_id`, `through_sequence`, `prefix_digest`, `verifier_label`,
`assessed_at_ms`, `conclusion` (`passed|failed|inconclusive`), `basis_summary`,
`basis_sequences` (unique positive event sequences within that prefix), and
`reported_skill_version` (null = unknown, or exact `skill_id`, positive `version`,
canonical `digest`). The version is a claim, never inferred from preparation,
latest publication or a success report. Basis sequences address immutable events;
summary is bounded sanitized text, not a URL to be fetched or trusted HTML.
At least one basis sequence and a nonempty summary are required.

Only the Run owner with read/write permission can create or withdraw a record.
Reads require owner/read. Prefix visibility and all original sources are checked
inside the transaction before create, replay, read or withdrawal. A changed digest
conflicts; inaccessible sources fail closed. No endpoint accepts owner/principal,
platform status, authenticated verifier role or receipt timestamp from the caller.
The server returns `evidence_origin:owner_transcribed_external`,
`verifier_authentication:unverified_claim`, `platform_verification:not_performed`.
A failed external assessment can coexist with a client success report; neither
changes the other or implies model use. A prepared v2 bundle only proves preparation.

The UUID is a retry identity scoped to the Run. Identical requests return the same
stored record and receipt time, including any later withdrawal; changed payloads
conflict. Records are immutable except for a one-way withdrawal tombstone.
`POST /v1/runs/{run}/verifications/{id}/withdraw` takes `{reason}`; identical retries
retain the first tombstone, differing reasons conflict, and withdrawal never
reactivates evidence. Corrections use a new UUID. Records are capped at 1,000/Run.
`GET /v1/runs/{run}/verifications?after=UUID&limit=25` is UUID-ordered, bounded 1–100,
returns `items,next_after` including withdrawn records, and rechecks each prefix.
No aggregate pass badge, background execution, external fetch or new identity role.

Fresh databases only: `oc_run_verifications` is admitted with the exact schema;
existing incompatible databases are rejected without modification. JOIN-free
queries and in-transaction Rust validation preserve the private source contract.
The existing MCP/client ingestion interfaces are unchanged. Integration and pilot
owners should use this HTTP/service DTO, display attribution and withdrawal, and
keep self-report, preparation and assessment separate. No production task/model
acceptance is claimed.

## PRD and ingestion boundary

This slice maps only to “the Run owner records external sign-off evidence”: the owner transcribes external assessment claims and references. It does not implement independent sign-off, allow a user to certify their own business outcome, or calculate a verified business pass rate. Even `conclusion:passed` stays an unverified external claim with `platform_verification:not_performed`; #89 remains open.

#87 handles explicit client collection and preparation using the existing Run
interfaces, and explicitly does not change verification semantics. No existing
Run event/reporter/prepare schema, MCP ingestion interface, permission role or
shared DTO changes in this slice. Integration must preserve the separate columns
for preparation, client report, external assessment and platform verification;
no source may promote a `user/test` report to authenticated independent sign-off.
