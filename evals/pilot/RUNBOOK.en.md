# Pilot Execution, Recovery, and Cleanup Manual

**English** · [简体中文](RUNBOOK.md)

## Pre-launch

Record integrated SHA, service and client binary hashes, OS, actual model revisions and settings, SQLite and AnySeek versions, Blob manifest, and time and cost budgets in the [freeze template](freeze.template.json). Initialize separate test resources and record logical-key aliases. The data owner signs off synthetic or redacted scope. The DBA and policy owner sign off both scenarios' rubrics, thresholds, and stopping conditions before execution. Missing required configuration or signature is BLOCKED.

## Question sets and scoring tables

Run at the root directory of the repository. All of the following seeds are public and used for demonstration purposes. The output directory must be a new path.

```sh
python evals/pilot/pilot.py generate --seed public-development-1 --output pilot-prepare-new
python evals/pilot/pilot.py generate --split held-out --seed public-example-only --exclude-tasks pilot-prepare-new/tasks.json --output pilot-example-new
python evals/pilot/pilot.py score --prepare pilot-prepare-new/observations.json
```

Each scenario has 8 questions, with two rows for each with and without Skill. Reimbursements cover normal, valid over-limit, duplicate, and missing vouchers, with two examples for each. The generator sorts out duplicates based on the actual input content and then renumbers the questions. Independent custodians should use `--exclude-tasks` for repeated questions in each batch, and the list will record the SHA-256 of these files. If there is insufficient available input, the generator will report an error. Formal unseen questions must be generated using independent confidential seeds after development and Skill freezing, with `tasks.json` delivered to the operator. Custodians seal the seeds, oracle, and digest; if the answer has been leaked, register the contamination and switch batches. Randomly arrange the two arms by question number, using clean sessions and the same client, model, and tool budgets, and record the order, timeout, and manual intervention.

Public development expense tasks from `--split prepare` retain self-contained rules and limits. Synthetic expense tasks from `--split held-out` omit rules, limits, target versions, and answers. They include only policy reference, claim date, facts, and evidence instructions. The generator keeps synthetic policy source and answers in custodian-side `oracle.json` without publishing material. Deliver only `tasks.json`, without output directory, seed, or oracle. If separately approved later, the custodian publishes only approved synthetic policy clauses and versions to the authorized test workspace. Baseline and reuse get identical access. Do not publish the oracle or expected answers. Without policy sources, retrieval cannot be tested. The externally supplied freeze digest reflects actual task-file bytes. Old synthetic expense holdouts with answers are historical records and cannot be scored as held-out with the new runner. Regenerate and freeze again without altering sealed files. Independently authored nonsynthetic cases still require freezing and review but need not use synthetic generator fields.

This is still a synthetic sample generation tool. Grouping and formatting do not prove independence, unavailability, or actual evidence collection; public seeds and samples must not enter formal pilot programs. The database sample structure does not change in this update and cannot replace DBA-approved obfuscation calibration. Fill in the `observations.json` in the same directory, retaining group summaries, table summaries, and row-by-row input summaries. Observation rows can be updated as verification progresses. The scorer checks the task files, lists, and observation tables against the original bytes. `--prepare` receives prepare and outputs `PREPARATION_ONLY`. Public handover drill records are kept separately. Unbound historical observation tables are for reference only.

Independent custodians should register the official reserved batch's original `tasks.json` SHA-256 in the trusted freeze record's `data.tasks_sha256`. Reserved scoring requires specifying this file independently, and when the verification is successful, it is marked as `FROZEN_BATCH`.

```sh
python evals/pilot/pilot.py score "$held_out_observations" --freeze "$trusted_freeze"
```

Empty template hashes and self-consistent unrelated batches are rejected. `FROZEN_BATCH` only establishes task byte agreement with the supplied freeze digest. It does not prove the freeze was approved. The custodian verifies freeze provenance and sign-off. Observation rows with verified checks or reported success require a nonblank reviewer and a nonblank reference per check. The scorer does not open these references or verify identity, content, or sign-off. Group metrics are labeled `OBSERVATION_SHEET_ONLY`. Even a perfect sheet is not a real pilot outcome. All preregistered cases stay in offline denominators. Independently confirm official business success and coverage and report them separately under the [PRD metric rules](PRD-MAPPING.en.md#disclose-metric-differences).

### The topic description and scoring calibration before official freezing

The following are all public synthesis counterexamples (`CALIBRATION_ONLY`), which do not enter the formal dataset, observation tables, or denominator. Public generative engines directly provide v1/v2 limits and rules in the reimbursement statement. Correct answers do not prove the retrieval or actual use of v2.

**Custodians and system owners** should separate the delivered statements from the verification materials. The statements only contain declared facts, system ID, applicable date, and evidence to be submitted, without system provisions, maximum amounts, or answers. The verification materials retain the restricted original policy versions, publication and withdrawal records, and expected results. Both arms use the same statement bytes, client, permissions, and budget, with only the decision to use the approved Skill being changed. If the operator or Skill knows the answer to the question, registration of contamination occurs, and the question is replaced.

Calibrate with the following examples: declare 120 units, complete vouchers without duplicates; old limit 120, new limit 80, applicable date after the new version takes effect. Only provide review opinions, without executing payments.

| Synthesis Observation | Amount Determination | Precision Version Use Determination |
| --- | --- | --- |
| Recommended 80, actual evidence indicates that the Run can verify v2 provisions | Correct | Independently verify provisions, applicable date, and corresponding operations, and record v2 |
| Recommended 80, but only service preparation v2 or the client claims to use v2 | Correct | Actual use UNKNOWN, not recorded as passed |
| Recommended 80, but actual evidence indicates v1 | Correct | Version mismatch, cannot offset with correct amounts |
| Recommended 120, unreadable after the source is withdrawn | If the independently retained authoritative basis has confirmed the application of v2, then incorrect; otherwise, it cannot be independently determined | UNKNOWN due to lack of actual use evidence; registration refusal, source not reopened |

**DBA** compares three types of examples that also manifest application timeouts: connection pool queuing without corresponding blocking on the database side; database lock chains causing connection saturation; lack of database or pool-side observations within the same time window. Compare queuing/occupation, waiting, or blocking chains with read-only troubleshooting recommendations. Pool fullness does not equal the root cause; insufficient evidence does not rule out database locks, nor does it justify passing based on accidental guessing.

An independent person freezes official tasks, verification material, and scoring rules separately. Record business judgment, version used, and evidence coverage separately under the [PRD metric rules](PRD-MAPPING.en.md#disclose-metric-differences). This section does not change the scorer or count as real-client verification or independent sign-off.

## Execution Matrix

| Inspection | Recording and Judgment |
| --- | --- |
| P1 Access | Verify dual-end MCP search/get and archiving round-trip on the same integration submission and new database, with independent testers checking the freeze table. |
| P2 Database Troubleshooting | No issues found in Claude Code first-time use and Codex re-use. DBAs verify blocking chains, connection pools, timeout classifications, and read-only recommendations based on snapshots. |
| P3 Reimbursement Review | No issues found in Codex first-time use and Claude Code re-use. System owners verify amounts, duplicate invoices, missing materials, and applicable system versions. Only review opinions are provided. |
| P4 Correction and Withdrawal | Correct to v2 and then refer to v1 with precise citations; re-review after candidate editing. After withdrawal, check get/search/context/package, associated runs, and prepare for replay. Compare new sessions, cache, and managed online reloads separately. |
| P5 Isolation | Archive across ends with the same key; deny reading private sessions, candidates, and runs for different keys (including administrators). Verify isolation across tenants/workspaces with the same name, credential rotation, and read and replay after revocation. |
| P6 Traceability | Associate input with each issue, prepare bundles precisely, deliver actual outputs, load natively, tools, outputs, and make independent judgments. Group by scenario, client, and baseline/multiplication. Mark UNKNOWN for executed but missing observation records, and NOT_RUN for unexecuted records. [Independent verification of origin and Run validation is linked](P6-EVIDENCE.en.md). |
| P7 Recovery | Stop writing according to the next section's experiment, and restore and clean up to the same version. DBAs, system owners, and independent testers sign off separately. |

Zero violations are required for isolation, withdrawal, and version checks. Register Windows/Linux separately; report counts and descriptive differences for small samples. Verify business results, evidence coverage, and Skill loading separately.

## Real closed-loop and precise loading evidence

Retain the precise asset version of the first creation or the immutable Run prefix, candidate revision, masking basis, reviewers, and approved packet digest. Attempts to publish after editing with old approvals should be rejected. Review and approve the publication according to the new revision. The re-creation endpoint exports approved packages in new projects and sessions. Register tenant, workspace, Skill UUID, version, package summary, file hashes, native session, Run, and time. Record server authorization acquisition, client native loading events, observable model inputs, and actual operations. After revocation, record new downloads, local bytes, session context, and managed online reloads; for non-observable cases, record UNKNOWN, and for non-executed cases, record NOT_RUN. Evidence is stored in a restricted directory, with indexes including relative paths, SHA-256, byte count, UTC time, collector, evidence type, PRD terms, execution items, and case number. Retain the original source and location (including PDF page numbers and UTF-8 range within the page), tool request receipts, output artifacts, and independent judgments. Mask public attachments, register collection omissions, record claims and citations, rework, time consumption, and costs.

## Same version of SQLite / AnySeek / Blob recovery experiments

The integrated mainline passed supplemental engineering recovery checks in a Linux synthetic environment. See [VALIDATION](VALIDATION.en.md#main-and-independent-test-trees-have-been-integrated-2026-11-18) for scope and gaps. Formal pilot.P7 still requires the full procedure and independent sign-off.

1. Only when the sample import is completed in a dedicated new database, precise v1→v2 corrections are performed, source revocation is carried out, private Runs are executed, Skill releases are made, and pending/failed jobs are handled; save expected get/search/context/package results, Run prefix, and permission denial results.
2. Record the same integrated SHA, binary hashes, schema, analysis, embedding settings, and AnySeek version. Stop clients, writers, host, and worker together and confirm they exited. Wait for unfinished requests or record their idempotency identities. No online consistent cross-store snapshot exists. If writes cannot stop, mark BLOCKED.
3. Save the entire **OC_DATA_DIR** (context.db, existing WAL/SHM, binding configuration, blobs, and associated files) in the same stop-writing window. Hash each file individually. Save the text, summary, full/text/vector/graph index, and configuration of all matching tenant/workspace collections according to the official backup process for the frozen AnySeek version. The DBA shall confirm the specific commands and snapshot IDs. This manual does not invent AnySeek backup interfaces.
4. Bind the backups on both sides to the same checkpoint ID and completion time. Backups with missing collections/blobs or different timestamps are considered invalid. The original site remains retained, AnySeek is not cleared, and historical evidence is not reconstructed from the source.
5. Restore to a new isolated directory and AnySeek instance/namespace, retaining the original scope/collection correspondence and binding. It is prohibited to return to the original instance. Perform the offline checks supported by this version with exactly the same binary/configuration, then start the ready check. No migration is required, and the new version is not needed for opening.
6. Compare the precise v1/v2 text, original source bytes, locator/hash, Skill digest, Run prefix, current authorization, and revocation failures before and after restoration. Verify each one individually for get/search/context/ preparation for replay/associated Run. Retry the original idempotent requests without duplicate publication. Restore the legal pending/failed states according to the original plan. Record failures and time taken, and do not substitute successful startup with failure.
7. Set up negative examples for restoring copies: missing blobs, conflicting AnySeek text, mismatched configuration, incompatible structures should be clearly failed and cannot be silently filled/transformed. Create independent copies from the complete backup each time. It is prohibited to destroy the original backup. Permissions and revocations must still be bypassed.
8. The DBA shall verify the consistency, integrity, and recovery results of the checkpoint. The system owner shall verify the business version conclusions after restoration. Independent personnel shall sign off on pilot.P7 (additional restoration threshold, not prd-v0.3.A7). If not actually executed, it is NOT_RUN. Results for Linux/Windows are recorded separately. This Python approach does not constitute a restoration experiment.

## Clean up and retain

Before the experiment, register all temporary directories, workspaces, collections, credential aliases, client projects/caches, model sessions, backups, evidence retention periods, and responsible persons. First, seal the necessary sensitive evidence and obtain approval, then revoke the test keys and shut down services/clients. The responsible person shall verify the precise test objectives based on the resource list before deleting temporary copies and isolating collections. Do not perform broad rm/glob operations; do not delete shared directories/historical libraries/unregistered collections. Backups of independent records shall be retained and destroyed upon expiration. Clear managed Skills and explicitly export copies, and close old context sessions. Uncontrollable client caches/vendors shall remain in the remaining state, without claiming physical erasure. Verify the cleaned/retained/blocked status of the resource list with the reviewer; retain data for restricted permission management and do not upload to public repositories.
