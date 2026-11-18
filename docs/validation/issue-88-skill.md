# Issue #88: selected-experience Skill candidate validation

Date: 2026-11-17. Local-only work for
[Origence #88](https://github.com/TheLudlows/Origence/issues/88).

Base and latest remote main checked with `git ls-remote origin refs/heads/main`:
`293e2bf6af3b180d43c8050731047b552ac07519`.
Branch: `p0/skill-candidate`; isolated worktree: `/workspace/origence-skill`.
Implementation SHA (code, tests and CLI contract):
`b15519184921f3460003016becd69d9b064261df`.
The subsequent validation-only commit does not change the tested code.
Read repository `AGENTS.md`, STATUS, Skill/Run contracts and the architecture
plan before implementation. The repository has no `.agents/skills` directory;
`/workspace/.agents` is empty in this environment.

## Scope and contract

- Independent `origence-skill` binary and public deterministic `SelectedExperience`
  module. Normal `cargo run` still defaults to `origence`. No shared navigation,
  generic DTO, Run model, database schema, migration, dependency, lockfile or CI change.
- Owner selects exactly one fixed Run prefix or exact asset/Learning version and
  explicitly confirms that method and redacted basis may be disclosed. No model,
  automatic source-content copying, cross-Run mining or automatic publication.
- Selected basis stays in the private immutable candidate revision. Server review
  submission requires that exact basis for selected-experience envelopes; basis
  edits require a new revision and invalidate old approvals. Legacy manual
  candidates keep their existing review contract.
- Existing live permissions, logical-key privacy, exact-version support checks,
  idempotent transactions and withdrawal checks remain authoritative. No new SQL
  execution API or query is introduced in application code. Manual review of the
  existing single-relation Skill queries and the added single-table test UPDATE
  found no JOIN, reconstructed association, external IO or intermediate commit.
- Bounded file reading runs on `spawn_blocking`; errors omit contents/paths.
  This is ordinary local file reading, with no hard cancellation claim.

## Validation environment

Linux, repository-declared Rust 1.98.0 (`88d9e12ae`, 2026-08-18).
Fresh temporary databases and synthetic fixtures only. CLI regression uses actual
built binary → loopback HTTP → real SQLite, `Models::disabled()`, and the existing
loopback AnySeek protocol fixture. Improve/Learning tests use deterministic local
model fixtures, not a paid model or production credential. No real task corpus is used.

## Final results

All completed successfully on the implementation above:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed, no warnings |
| `cargo check --locked --no-default-features --lib` | Passed |
| `cargo test --locked --lib skill -- --nocapture` | 11 passed, 0 failed |
| `cargo test --locked --test skill-workflow -- --nocapture` | 1 passed, 0 failed |
| `cargo test --locked --test local learning_ -- --nocapture` | 20 passed, 0 failed |
| `git diff --check` | Passed |
| `origence-skill --help` | Standalone command inventory rendered successfully |

Cargo used `/workspace/.cargo`, Rustup `/workspace/.rustup`, and the reusable
build cache `/workspace/Origence/target`; test databases remained new temporary
paths. No full-suite or production/real-engine ranking acceptance is implied.

The 11 Skill-filtered tests cover text-only packages, native export regression,
explicit disclosure, deterministic prefill, private notes, bounded input/error
sanitization, selected-basis mismatch rejection, basis/step edits invalidating
approval, idempotent save/publish, source withdrawal, revoked reviewers, foreign
scope and owner isolation, and same-schema restart. The standalone CLI regression
covers owner-only prefix selection, unchanged fixed prefixes after later Run
append, confirmation refusal, retry without duplicate revision, administrator
isolation, pending publication refusal, appointed reviewer enforcement, changed
basis invalidating old approval, explicit publication and online withdrawal.
The Learning regressions include transitive/private-support withdrawal and live
permissions through existing APIs, using synthetic local provider responses.

## Remaining acceptance

- Windows behavior and Windows CLI execution not run here; the portable tests are
  committed for subsequent coordinated validation. No CI result is claimed.
- No real Claude Code/Codex task, native managed loading or business benefit trial.
  Existing native export is unchanged; offline exports are not remotely recalled.
- Run-prefix CLI path is end-to-end tested. Exact Learning selection reuses existing
  asset read/source guards and Learning regression coverage; no new Improve-trigger
  UX or direct private-turn selection is added.
- No push, PR, Issue mutation, merge or deployment. Publication remains subject to
  root-thread review of the five-route joint contract.

## Learning-source follow-up (2026-11-17, draft PR #94)

Test-only implementation: `a709dfe2702efcb223d7765fc9ae0b7a71a838c9`, based on
`e08e0e2c0e418757486c3899b47d869a0366a89e`. Remote main remains
`293e2bf6af3b180d43c8050731047b552ac07519`. No product, schema, Run interface,
SQL, model configuration or CI change was needed.

`learning_skill_cli_exact_source_private_support_and_withdrawal` creates a real
published Learning through the existing synthetic Improve/ingest fixture and real
SQLite. A different same-workspace writer uses the actual CLI over loopback HTTP
to select asset v1 with its exact content hash; nonexistent v2 returns NOT_FOUND.
That member saves a candidate and submits two explicit packets to a separate
reviewer. Both are approved, one is explicitly published in the temporary test
workspace, and the other remains unpublished. Neither writer nor reviewer can
read the original owner's turn, feedback, raw source event or private Learning
detail. Each denied HTTP read has an owner HTTP 200 positive control. Selected
packets contain no original answer, session/turn identity or private candidate notes.

Deleting the original supporting archive event through authenticated HTTP leaves
cleanup pending. The previously approved unpublished candidate now fails publication
with NOT_FOUND; the previously readable package, candidate and exact asset source
also fail with NOT_FOUND. This validates transitive support withdrawal before index
cleanup, rather than merely withdrawing a Skill version. Skill-stage proposal,
verification and embedding fixture counters remain unchanged. All data and
credentials are generated in fresh temporary test storage; no paid model or actual
business Skill is used.

Results under Rust 1.98.0:

- `cargo test --locked --test local learning_ -- --nocapture`: **21 passed**, including
  the new Learning CLI case (replaces the earlier 20-test coverage count).
- `cargo test --locked --test skill-workflow -- --nocapture`: **1 passed**, existing
  Run CLI workflow retained.
- `cargo clippy --locked --all-targets -- -D warnings`, `cargo fmt --all -- --check`
  and `git diff --check`: passed.
- Initial test incorrectly required HTTP 403 for every private read; a writer
  without the owner's session received the existing 404 privacy response. The
  final test accepts 403/404 with owner-positive controls. No authorization behavior
  was changed to satisfy the assertion.

This closes the earlier exact-Learning CLI coverage gap. The original local-only
stage above is historical; this increment updates the same draft PR #94. Windows,
real Claude Code/Codex tasks, native loading and business outcomes remain unaccepted.
No Issue mutation, merge or deployment is performed.

## 2026-11-18 coordinated integration after #93

Merged main `9c4ed2ea6fdf17ed8b931ddcddf4f081c4367719` into original PR #94
head `013dfb59130f53cad68d3e158564d186908f0ded` without rewriting history.
Only `docs/VALIDATION.md` conflicted; both historical entries were preserved.
No Skill, schema, API or test behavior was changed during conflict resolution.
Linux Rust 1.98.0: `cargo fmt --all -- --check`, `git diff --check`,
`cargo test --locked --test skill-workflow` (1),
`cargo test --locked --test local learning_loop` (21), and
`cargo clippy --locked --all-targets --all-features -- -D warnings` passed.
Tests used fresh temporary data and local fixtures, not paid models or real-client
acceptance. Remote CI must pass on the new merge HEAD before squash merging.
