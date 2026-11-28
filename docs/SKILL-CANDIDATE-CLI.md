# Selected-experience Skill CLI (#88)

**English** · [简体中文](SKILL-CANDIDATE-CLI.zh-CN.md)

`origence-skill` is an independent entry point over the existing authenticated HTTP
API. It does not open a database, invoke a model, mine Runs, submit a review, or
publish as a side effect of drafting. The host must already be running. Use the
owner's `OC_API_KEY` and `OC_SERVER_URL`; switch credentials explicitly for the
appointed reviewer. Keep selection files and owner-only candidate output private.
The normal `origence` CLI remains the default binary.

## Draft from a Run in the console

Sign in with the Run owner's workspace account and open the Run in the trace view.
Select the endpoint event and choose **选取依据并起草** (Select evidence and draft).
The editor stays bound to that exact prefix while you inspect other timeline events.

Enter the method and the redacted evidence you want to share. Confirm disclosure
of both before saving. Keep the candidate ID to reopen it from the **Skill 候选**
section of the same Run. Saving changes creates a revision and invalidates older
approvals. Enter the designated reviewer's writer or admin credential ID, Skill ID, and version,
then submit the saved revision. The reviewer can get their credential `id` from
`GET /v1/whoami` and open the packet with `origence-skill review` below. Use the
commands below for approval, explicit publication, and online download.

## Select and edit with the CLI

Build with `cargo build --bin origence-skill`; use `target/debug/origence-skill`
(or add `target/debug` to PATH). The examples below use placeholders,
not real task data or credentials. Choose exactly one fixed source:

```sh
origence-skill run-source RUN_UUID 3
# Or an already published Improve → Learning result at a specific version:
origence-skill asset-source LEARNING_ASSET_UUID 1
```

Copy that JSON object into `source` in a private `selection.json`. Run sequences
are immutable prefix endpoints, not “latest” or an implicit entire Run. The
`asset-source` command also accepts authorized exact knowledge or memory versions.
It returns the address and hash. Source prose and private Learning support stay
outside the candidate. Existing source visibility and Learning-support checks apply.
This CLI does not run Improve or extract a private turn; select an existing Run
prefix or published Learning instead.

```json
{
  "source": {
    "kind": "run",
    "run_id": "11111111-1111-4111-8111-111111111111",
    "through_sequence": 3,
    "digest": "REPLACE_WITH_EXACT_PREFIX_DIGEST"
  },
  "name": "verify-result",
  "description": "Independently verify a result before reuse",
  "steps": "1. Record inputs and assumptions.\n2. Check the output independently.\n3. Record limitations and unresolved failures.",
  "selected_basis": "A redacted observation the owner explicitly permits this reviewer to see; not proof of general reliability.",
  "disclosure_confirmed": true,
  "private_notes": "Private supporting notes, excluded from review and package."
}
```

The owner must confirm disclosure of **both method and selected basis**. The flag
is an explicit user assertion, not a secret detector or independent verification.
A false/missing flag fails before saving. Fields are strict and bounded; files are
read with a 1 MiB actual-read limit. Names and steps use existing text-only package
validation. No executable metadata, dynamic shell expansion, or model is added.

```sh
origence-skill save CANDIDATE_UUID --expected-revision 0 --input selection.json
origence-skill candidate CANDIDATE_UUID
# Edit selection.json manually, then append an exact revision:
origence-skill save CANDIDATE_UUID --expected-revision 1 --input selection.json
```

`SelectedExperience::candidate()` is a public, host-independent deterministic
prefill function. It preserves selected steps as the editable Skill body and binds
one exact source. The existing `save_skill_candidate` service/transaction checks
live permission, logical ownership and source authority before persisting anything.
Calling the prefill function alone grants no access and creates no server object.
Same ID + same expected revision + identical content retries return the same
revision. Changed payloads conflict. A stale editor cannot overwrite newer work.

Selected basis is stored in an owner-only versioned notes envelope. Every step,
basis, source or notes edit appends a revision and invalidates older approvals.
For this envelope, the server rejects a review whose basis differs from the saved
selection; changing basis requires a candidate edit even through a direct HTTP
caller. Existing hand-authored candidates without this envelope retain the existing
explicit review-input contract. Original notes, raw source IDs and private supports
never enter the selected packet or distributed package automatically.

## Review, approve and explicitly publish

Submit the exact saved revision and target to a named live reviewer credential:

```sh
origence-skill submit CANDIDATE_UUID REVIEW_UUID --revision 2 \
  --reviewer-id REVIEWER_CREDENTIAL_UUID --tenant-id TENANT_UUID \
  --workspace-id WORKSPACE_UUID --skill-id SKILL_UUID --version 1
# Appointed reviewer, using their own credential:
origence-skill review REVIEW_UUID
origence-skill decide REVIEW_UUID EXACT_PACKET_DIGEST --approve
# Or --reject; decisions are immutable. A revised decision needs a new review ID.
origence-skill publish REVIEW_UUID EXACT_PACKET_DIGEST
```

Submit rechecks the current revision at the server after reading it; a concurrent
edit fails closed. The packet contains only the generated package, selected basis
and review/target identity. Review does not grant raw archive access. Publish is
separate and rechecks current revision, credential, source and digest authority.
A reviewed method remains a proposal until explicit publication. Its instructions
do not grant tool execution rights or establish that reported outcomes are true.

```sh
origence-skill package SKILL_UUID 1
origence-skill withdraw SKILL_UUID 1
# Subsequent online package fetch now fails.
```

Source withdrawal also denies online fetches. Callers manage copies they previously downloaded. Offline copies are not
remotely cleaned up or instantly recalled. No native client loading, automatic
publication, shared navigation, schema migration, or cross-Run mining is added.

See the [archived Issue 88 validation](https://github.com/TheLudlows/Origence/blob/a56246406a0688f646a45a85cc2954fb14a8d28a/docs/validation/issue-88-skill.md) for test results and acceptance scope.
