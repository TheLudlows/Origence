# Skill review, publication and distribution

**English** · [简体中文](skills.zh-CN.md)

This contract defines the public-library/SQLite workflow. A reported successful
outcome does not independently establish that a method works. A client supplies a
draft derived from authorized, exact content item versions or immutable private Run
prefixes. No operation auto-publishes.

## Contract

- `save_skill_candidate(id, expected_revision, input)` creates or appends immutable
  revisions under the caller's stable logical key. Private notes and source
  references remain owner-only, even from workspace administrators.
- The owner submits an explicit review ID, exact revision, named reviewer credential,
  workspace/Skill/version target, and selected redacted basis. Only that reviewer
  and the owner can read this packet. A reviewer gets no implicit raw archive access.
- `decide_skill_review(review, packet_digest, approve)` requires the appointed
  credential to have live writer or admin access. A decision is immutable. A new review ID is needed to change
  the decision. Credential rotation requires a new reviewer appointment; candidate
  ownership survives an authorized logical-key rotation.
- `publish_skill(review, packet_digest)` is a distinct explicit operation. It
  rechecks current write access, the approving credential, current candidate
  revision, supporting source authority, and exact generated package. Any candidate
  edit invalidates prior approval, including a source or private-note edit.
- Publication stores provenance, the approved review reference, and the package
  digest in a SQLite transaction. Package bytes live in the immutable review record.
  Distribution verifies the manifest, digest, and sources. Versions increase monotonically.
  The publication transaction needs no external I/O.
- `skill_package(skill, version)` returns only approved manifest and files, with
  fresh source and withdrawal checks. Review basis, raw source IDs, private notes,
  and approval records are not in the distributed package. Exact historical source
  versions remain usable while their authority/support is valid; later appends to a
  Run do not alter an approved immutable prefix. Retraction/withdrawal fails closed.

`SkillPackage` contains `manifest`, `files` and `digest`. The format-1 manifest has
only tenant/workspace, Skill ID/version, name, description, and artifact hashes.
Files are server-generated `SKILL.md` and at most sixteen bounded
`references/<lowercase-slug>.md|.txt` files. The digest is SHA-256 of the canonical
serde JSON tuple `(manifest, files)`; maps are ordered. `SkillPackage::validate()`
checks the exact generated frontmatter, allowlisted files, and all hashes. It does
not substitute for a live authorized distribution call.

No arbitrary frontmatter, executable/script file, hook, dynamic command,
tool-preauthorization or credential metadata fields exist in this format; unknown
fields and unsupported paths are rejected, including Windows device names.
The validator rejects Claude shell-preprocessing markers in descriptions, body, and
references (inline and fenced forms), per the [client contract](https://code.claude.com/docs/en/skills#inject-dynamic-context).
Natural-language instructions can still suggest actions; they are not execution authorization. This is not a general secret detector: reviewers remain responsible
for ensuring the selected prose/reference contents are authorized and redacted.
Previously downloaded copies cannot be instantly recalled, and approval does not grant
business tool permissions.

## Client boundary

The authorized package endpoint rechecks scope, version, and approved source on each
download. Callers manage any local files created from the response. See
[CLIENTS](../CLIENTS.en.md) for supported client entry points. Current results belong in [VALIDATION](../VALIDATION.en.md).
Private Run references retain the restrictions in the [Run contract](runs.md).

## Selected-experience drafting entry point

The independent [Skill CLI](../SKILL-CANDIDATE-CLI.md) deterministically prefills a
private editable candidate from one explicitly selected fixed Run prefix or exact
memory, knowledge, or Learning version plus owner-confirmed disclosed prose. It reuses existing
candidate/review/publication endpoints. Selected basis is revision-bound in a
private notes envelope; submitting a different basis for that envelope conflicts
until the owner appends a candidate revision. No private supports are auto-copied,
no model is required, and saving never submits, approves or publishes.
