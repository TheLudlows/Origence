# S3 explicit Improve → Learning

## Scope and authority

This slice completes the explicit session feedback loop, not roadmap S4. It targets
fresh databases and uses the existing configured extraction model, typed durable
queue, serialized Worker, and knowledge publication path. It introduces no automatic
trigger, ranking change, memory overwrite, Guidance lifecycle, provider, migration,
or background orchestrator. Deterministic fixtures establish engineering behavior;
they do not establish factual truth, real-model quality, or Agent benefit.

The caller explicitly supplies a stable improve_id and nonempty applicability
conditions. Scope comes from authentication and the session route. The model cannot
broaden conditions. Positive feedback is evidence of usefulness, never proof of
truth. Untrusted turn text, feedback and model responses cannot grant authority.

## Durable flow

1. In one command transaction, validate caller authority and immutable domain ID,
   freeze a bounded batch, reserve the session's single unfinished run, and enqueue
   a typed Improve job. A monotonic, scope-qualified feedback sequence is used;
   wall-clock timestamps and feedback UUIDs without their principal are not cursors.
2. The Worker checks archived turns and all evidence dependencies, obtains at most
   eight proposals and mechanically validates exact support IDs and UTF-8 quotes.
   Persist this plan before verification calls.
3. Run a separate support/durability verification pass with the same configured
   extraction infrastructure. Persist every acceptance/rejection before any
   downstream asset creation, model indexing or native write.
4. Atomically create accepted Learning records, support references, generated
   source events, knowledge assets and ordinary Ingest jobs. No nested public
   Service command is allowed. Recovery reuses the original IDs and plans.
   Exact statement/conditions dedup is rechecked against valid accepted/published
   registrations in this same transaction. A conflicting reservation rolls staging
   back and leaves the cursor unchanged; cancel that unfinished run and submit a
   new improve_id after the competing outcome. It is not semantic deduplication.
5. Each child follows existing publication and artifact ownership rules. Publication
   is atomic per Learning, not across the whole run: the graph publication structure
   assumes one knowledge asset. A failed child leaves the run unfinished. Explicit
   retry uses that child's existing job and persisted plans.
6. Advance the session cursor only after every accepted child has published and all
   other frozen outcomes are durably accounted. Empty/rejected-only plans do not
   create fake assets. Cancelled/revoked/stale workers cannot finalize publication.

Input is bounded to 32 feedback records and 128 KiB of frozen model input.
Each fully serialized proposal/verification request, including conditions and the
saved proposal where applicable, is also capped at 128 KiB before network IO. Oversized
initial input fails without advancing the cursor. Conditions, statement, explanation,
quotes and output count also have validation limits. Hidden turn inputs may be
recorded as skipped by ID/sequence without exposing their content.

## Withdrawal and visibility

Learning support means the entire archived turn remains valid, including every
historical reference, version root, original semantic member source and complete
semantic snapshot. A generated source event being active alone is insufficient.

The same dependency check runs on proposal/plan replay, before indexing, at final
publication, before candidate limits and on final search/get/resolve/graph/source
reads. Cleanup detaches invalid owners but is not the visibility boundary.
Learning-backed turn evidence creates transitive dependencies: traverse them with a
fixed bound and reject cycles or over-limit closure; never truncate and accept.
Separate original member source versions from asset versions. Ordinary knowledge
update and restore cannot be used to rewrite a Learning's support contract.

## Acceptance

Cover public API permissions and strict fields, turn → feedback → Improve → indexed
Learning → another session's retrieval, command/domain replay and concurrent runs,
principal-qualified feedback IDs, equal timestamps, late feedback, durable stage
reopen, child failure/retry, revoked/cancelled jobs, malformed JSON and false quotes,
condition broadening, unsupported helpful answers, and source/member/asset withdrawal
including nested Learning dependencies. Exact execution evidence belongs in
`docs/VALIDATION.md`; pending or static inspection is never a passed Rust check.
