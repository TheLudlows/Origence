# #91 Local Verification Records

**English** · [简体中文](VALIDATION.md)

## Public Dual-Scenario Cross-Client Replay (2026-11-18)

The Windows run used product main `e024b78`, AnySeek `6a46fc7`, Claude Code 2.1.327 (reported model `glm-5.3`), and Codex 0.170.0 (configured for `gpt-6.2-sol`). Each direction used a fresh database and workspace, separate writers for the initial and follow-up sessions, and separate reviewer credentials. The first client extracted structured rules from the public prompt. The test harness verified the rules and submitted a candidate using the immutable prefix of the first Run. The test reviewer explicitly approved and published it. Both Skills were v1. The reimbursement rule corresponds to policy v2 in the prompt. [Full final run record](public-handoff.windows.json)

| Scenario | Initial | Replay |
| --- | --- | --- |
| Database Connection Wait | Claude Code identifies a blocking session 312 in the snapshot, determines a lock wait, and recommends a read-only check for blocking sessions | Codex processes another snapshot after loading the approved rules: no blocking sessions, connection pool 12/12. Returns `pool`, references `pool_active`, and recommends checking connection pool metrics |
| Reimbursement review | Under v2, Codex deferred a duplicate request for 120 TEST, recommended an amount of 0, and extracted a decision table | Claude Code loaded the approved rule and reviewed a separate 123 TEST claim with receipts and no duplicate, recommending 97 under v2 |

The follow-up clients tried to read the initial Run and private candidates four times in total. All returned HTTP 404. Their approved packages remained available for server export. The Codex client model request record contains the complete package text and follow-up Run ID. The native Claude Code session records the successful Skill call and text. The provider-side request body was not collected. Both clients returned a random verification string available only in the package. The first follow-up Run event records the loaded version and digest. The second records an output digest. The harness reported and read back both.

| Attempt | Time Taken | Database | Reimbursement |
| --- | --- | --- | --- |
| 1 | 120.22 seconds | Initial Run timeout | Serial execution, not yet started |
| 2 | 50.08 seconds | Initial output fails JSON format check | Completed initial and replay |
| 3 | 33.22 seconds | Initial decision table failed validation and was not published | Skill loaded, but the follow-up result did not match |
| 4 | 40.91 seconds | Initial and follow-up sessions completed | Initial and follow-up sessions completed |

From the second run, both scenarios used independent new databases and ran concurrently. Later runs clarified field formats, enum values, and integer units. The final run's expected business outcome was the same as the first three. Four reports total 244.43 seconds. 19 Python tests, script compilation, Rust 1.98.0 formatting, and all-target Clippy with warnings as errors passed. No Rust or SQL changes. [Reproduction](PREFLIGHT.en.md#rerun-cross-client-handoff-in-both-scenarios)

Both follow-up prompts and judgment rules are public. The initial reimbursement task received the policy in the prompt, and database diagnosis used synthetic snapshots only. Neither path ran production operations, formal held-out cases, a control without Skill, or independent business judgment. Both scenarios are recorded as `NOT_RUN` under the formal denominator.

## Dual-client native Skill loading (2026-11-18)

On Windows with product baseline [`e024b78`](https://github.com/TheLudlows/Origence/commit/e024b78eb2ecbf1aa1851783c89be8238ca819a1) and AnySeek `6a46fc7`, a public synthetic reimbursement of 110 TEST units ran in a fresh workspace. The script created a v1 source. Separate writer and reviewer test accounts submitted and approved the candidate. Two new projects each exported the same Skill v1 from the server. Manifest entries and file bytes were verified individually. The final rerun took 8.83 seconds. [Machine-readable record](native-skill.windows.json)

| Client | This round of observation |
| --- | --- |
| Codex CLI 0.170.0, specified `gpt-6.2-sol` | The native `skills/list` found the exported file accurately, and `turn/start` explicitly selected it. A request record sent to the model includes a Run ID, skill name, and complete text. The final answer provides 100 and a random string. |
| Claude Code 2.1.327, `haiku` reported as `glm-5.3` this round | The native session of the new project records a `Skill` call matching the skill name, a successful reply, and injected text. The final answer provides the same amount and random string. The supplier-side request text was not collected. |

Both clients created Runs. After checking native evidence, the harness wrote `skill_loaded_reported`, read it back by Run, and verified `reporter=test`, event content and `reported` attribution. It then read the immutable prefix ending at the first event. The JSON retains the approval digest, exact version, file hashes, session and Run IDs, and request or output-stream digests. Original model requests and client transcripts in the temporary directory were cleaned up afterward.

After withdrawing the sole supporting source, the package is read back and returns an HTTP 404 error. Both new client exports return "resource not found," with zero files in the new directory; the manifest and SKILL.md in the original two projects remain consistent with the released version. Open sessions, withdrawn contexts with injected context, and managed online reloads are still pending verification.

The first two runs produced no loading evidence under Claude Code's `--bare` configuration. The first revocation request also returned 422 because the idempotency key was missing. After supplying it, switching to normal startup with only project settings, and using the `Skill` tool, both clients were observed loading successfully twice. Exact immutable model revisions are still unfrozen. This run verified one test-harness-authored Skill. Bidirectional initial work, held-out reuse, independent business judgment and formal scoring remain NOT_RUN.

Rust 1.98.0 passed `cargo fmt --all -- --check` and `cargo clippy --offline --locked --all-targets -j 2 -- -D warnings`. A Windows executable in the default target directory was in use. Built and ran in a separate `target/ci-binary-check` directory with `cargo build --offline --locked`. Passed 15 acceptance-evidence unit tests, including 2 tests for attribution of native request evidence, and the script compilation check. Rust and SQL were unchanged. Native loading on Linux remains unverified.

## Initial preparation (2026-11-17)

Date 2026-11-17. Baseline `293e2bf6af3b180d43c8050731047b552ac07519` was confirmed against `git ls-remote origin refs/heads/main`. The original checkout was `c70a1aaa0b3cb75872109406dc17b1b461d1f771` and was not edited. Separate worktree `/workspace/Origence-pilot`, branch `eval/p0-independent-pilot`. The final SHA is recorded after commit in the root thread report or `git rev-parse HEAD` to avoid self-referencing hashes.

Read the root AGENTS.md, current STATUS, architectural design, Skill/Run/storage contracts, CLIENTS, OPERATIONS, evaluation criteria, and related client_skill/runs implementations. No `.agents/skills` was found in the base directory or `/workspace/.agents`, and no missing Skills were claimed to be loaded.

- `python3 -m unittest discover -s evals/pilot -p 'test_*.py' -v`: 10 tests passed.
- `python3 evals/pilot/pilot.py generate --seed public-development-1 --output /tmp/origence-pilot-91-smoke`: Successfully generated 16 questions and 32 baseline/reuse empty observation records.
- `python3 evals/pilot/pilot.py score /tmp/origence-pilot-91-smoke/observations.json`: 4 groups of 8 cases with 32 required checks per group. Both success and verification counts were 0, returning NOT_CERTIFIED / NOT_CHECKED. This records that the run was not executed and does not measure product failure or model performance.
- `cargo fmt --all -- --check`: No successful execution, the environment reported `cargo: command not found`. The Rust toolchain was declared to be 1.98.0; the Clippy/Rust suite was also not run. This route does not modify Rust, produce SQL, services/DTOs, UI, CI, or share state.
- Manual SQL scope review: This Python route does not have SQL execution/generation interfaces, no database connections, network or model calls; it does not involve JOIN-free transactions, permissions, or contract issuance.

Since this route explicitly defines a new acceptance directory, records are kept here. The shared `docs/VALIDATION.md` and `docs/STATUS.md` are not modified. The synthesized smoke output is retained in the /tmp directory, created by the generator; no product database, collections, credentials, or client copies were created.

## No acceptance/release boundaries

Joint numbers/contracts A1–A7 are still pending coordination and confirmation. The four routes' functions must be re-accepted after the same integration SHA. The actual Claude Code/Codex, services/models freeze, native Skill loading and retrieval, browser/Windows, actual SQLite/AnySeek/Blob versions, DBA/system owner/independent verification, all NOT_RUN. Unrunned billing/production access does not constitute a blocking bypass reason. No migration work is performed. Only local submission; no push, PRs, issue updates, integration, or deployment.

## #92 Draft review fixes (2026-11-17)

Fixing valid over-limit examples: Normal reimbursement, valid over-limit, duplicates, and two instances each with missing vouchers. The preparation/non-visible groups remain unchanged. Over-limit instances are non-duplicates and have vouchers, independently asserting that v2 truncation is below the application amount, and does not equal the full reimbursement result according to v1. The total number of questions is 16, the number of observations is 32, and the denominator for necessary checks remains unchanged. Each scenario/group is renumbered after being stably rearranged with a seed. The original seeds, group/category labels, and oracle answers are not included in tasks.json. The public test seeds are not actual sealed non-visible seeds.

13 Python tests passed, including 102 seeds × prepare/held-out coverage, deterministic same-seed output and changed category mapping with other seeds, group isolation, and absence of oracle data in Agent input. Generation and scoring smoke passed for prepare and held-out and both are NOT_CERTIFIED. Main still matched 293e2bf6af3b180d43c8050731047b552ac07519. Remote PR head before repairs was 045843a970944eec502b3c415805cf64b69b869b, matching the local parent. Models and formal pilot were not run. NOT_RUN and independent sign-off requirements remain.

## PRD v0.3 Local Pending Document Patches (2026-11-17)

Exact parent 810f6900ff7bc990cdacd95862d5cf2da1688b75, isolated local branch `docs/pilot-prd-v03-local`. No push, PR/Issue update, or remote CI for that work. The original PRD v0.3 PDF was retrieved through the required process. Its SHA and chapters and precise reading boundaries for issues #87–#91 and drafts #94/#96 are in PRD-MAPPING.md.

Renamed the original pilot A* to pilot.P*, and listed prd-v0.3.A* separately. P5 and P7 cannot substitute for PRD A5 and A7. Template changes apply only to new review forms. Existing results were not migrated. `pilot.py`, tests, and observation data formats were unchanged. Pending text remains compatible and its meaning is documented. Template compatibility has not been tested with a production consumer. Local searches found no generator or scorer reading the template. The difference between PRD and offline metric denominators is documented, with the original scoring logic preserved.

Minimum verification: The original 13 Python tests have passed; two JSON parsers, P/A with 7 independent namespaces each, all NOT_RUN, pre-check does not score, managed online reloading NOT_RUN assertions passed; `git diff --check` passed. The patch only contains documentation/templates; there are no changes to production code or SQL, and no new Rust checks have been added. No real clients, models, installations, logins, production data, recoveries, or managed reloading have been run; no real non-visible seeds have been generated. The publicly synthesized examples are for future pre-checks only, not scored or acknowledged. Still waiting for four-way SHA consistency, joint mapping freeze, and independent acknowledgment.

## #92 Document Patches Rechecked Before Release (2026-11-18)

Reused local 6e0ac96a277be92ae7734396a51411bcdc9ad3cf. Remote #92 remained at 810f6900ff7bc990cdacd95862d5cf2da1688b75 and main at 293e2bf6af3b180d43c8050731047b552ac07519. Subsequent authorization allowed only a fast-forward to the existing draft branch. The root AGENTS.md was checked, /workspace/.agents was empty, and neither repository nor worktree contained .agents/skills.

PREFLIGHT added requirements for fresh Windows DataDir, a separate AnySeekDataDir and workspace, and initialization. #95's rejection of an old database without oc_run_verifications was classified as startup schema incompatibility. It separately lists authorized real v2 retrieval using prompts without rules or answers and an unauthorized clean-session control. Neither enters the formal denominator. Client triggers and JSONL signals still need inspection on the frozen version. Passed 13 Python cases, namespace and status checks for both templates, and diff --check. Code and score format remained unchanged. No model, installation, sign-in, business sign-off, Windows run or actual retrieval preflight was performed. #91 remained NOT_RUN, with no merge or deployment in this round.

## Main and independent test trees have been integrated (2026-11-18)

The remote main and independent test trees were verified to be `48cfa7c643b8c077ad33c55981def51979afde05`. Four implementation PRs have been integrated, and the GitHub integration status matches the local ancestor commit:

| Implementation Scope | PR | Main Branch Commit |
| --- | --- | --- |
| #90 Console Governance | [#93](https://github.com/TheLudlows/Origence/pull/93) | `9c4ed2ea6fdf17ed8b931ddcddf4f081c4367719` |
| #88 Skill Candidate | [#94](https://github.com/TheLudlows/Origence/pull/94) | `a56246406a0688f646a45a85cc2954fb14a8d28a` |
| #89 Run Verification and Review | [#95](https://github.com/TheLudlows/Origence/pull/95) | `e78b6ba5f3d6fcb4730789981a092c85cdc4c12a` |
| #87 MCP and Private Run | [#96](https://github.com/TheLudlows/Origence/pull/96) | `48cfa7c643b8c077ad33c55981def51979afde05` |

[Main Branch CI 44217031418](https://github.com/TheLudlows/Origence/actions/runs/44217031418) has completed/successful for `windows`, `node-ui`, `fast-check`, and `linux-native`. [#92](https://github.com/TheLudlows/Origence/pull/92) remains a draft. The official pilot is still **NOT_RUN**.

### Version-specific recovery

This Linux x86_64 run used the Origence mainline above and CI-pinned AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`. SQLite and Blob used a fresh temporary authoritative directory, with distinct authorized and unauthorized synthetic workspaces. AnySeek used a dedicated loopback port and new storage root. After the host stopped writes and shut down normally, the complete authoritative directory was copied within the same write pause. Native AnySeek backup `POST /api/v1/service-backups` and restore `POST /api/v1/service-restores` produced an independent destination root. The restored instance started from the new root after the source test instance stopped, keeping the binding by reusing the dedicated port. Test processes, directories, snapshots and restored copies were cleaned up afterward.

Followed the [runbook's same-version recovery procedure](RUNBOOK.en.md#same-version-of-sqlite--anyseek--blob-recovery-experiments) to verify the following **10 sets of engineering checks**.

1. The complete authoritative directory copied during shutdown matches the file hashes of the source directory, including SQLite and Blob files.
2. When the AnySeek service backup is retried using the same operation ID, the same response is returned.
3. AnySeek restoration generates independent roots that have not yet been activated. The same response is returned when the same restoration request is retried.
4. The restored SQLite is checked against an offline structure of the same version.
5. The historical v1 and current v2 text can be read according to the exact version; the current pointer remains at v2.
6. Both the original Blob bytes from v1 and v2 are consistent with those uploaded at the beginning.
7. After restoration, the exact version, source, block, locator, and text of the keyword hits are retained.
8. Retracted source fixed-version reads return 404, with zero hits in keyword searches.
9. The original Run prefix and preparations for the same event are returned consistently.
10. Fixed-version assets from the workspace without permissions and original Run reads also return 404, with zero hits in keyword searches.

This Linux engineering check used synthetic data. Missing or conflicting object cases, restored Skill digests and job sets, a Windows-specific restore drill, and production recovery still need acceptance. Formal pilot.P7 sign-off remains **NOT_RUN**. Independent DBAs need to review the stopped-write boundary, matching snapshots, complete assertions and cleanup. The policy owner and the independent reviewer have not signed off.

The acceptance materials themselves passed 13 Python cases, template namespace/non-authenticated status checks, and `git diff --check`. Testing of the Codex and Claude Code product clients was paused, and both formal trials and scoring remain `NOT_RUN`.

## #92 Independent freeze value review (2026-11-18)

The review draft `7238499241867bc3ed3e055ebfb06aa32d5a1478` and its main branch ancestor `e024b78eb2ecbf1aa1851783c89be8238ca819a1` were reviewed. The draft had 22 offline tests passed, covering input deduplication, line-by-line input summaries, and line error checks. Public synthesis CLI verification confirmed that after switching to a consistent set of tasks, tables, and lists, the old CLI still returned scores because it did not read the independent freeze records.

The local patch only added observation-sheet grouping and independent verification of the frozen SHA-256 for formal held-out scoring. Passed 24 offline tests and a CLI smoke check. The same input scored twice produced byte-identical output. Wrong-batch rows in the same group, preparation sheets, public handoff records, and empty frozen hashes were rejected. The editable observation rows passed validation for four groups of 8 cases and 32 required checks per group. In the synthetic completed example the success rate was 1/8 and required check coverage was 6/32. Formally measured PRD outcomes and sign-offs are recorded independently. The formal pilot and P7 sign-off remain NOT_RUN.

Restore was not run on the integrated mainline in that round. The recorded historical results for `48cfa7c6` were retained. For any new synthetic rerun against the then-current mainline, build from `e024b78eb2ecbf1aa1851783c89be8238ca819a1` and AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a` and retain both binary hashes. In a fresh temporary database, pause writes and record the same checkpoint for the SQLite/Blob copy and AnySeek service backup. Use the same build to restore to an independent root and verify versions, Blob, revocation, isolation and replay as described in RUNBOOK. If the final frozen version changes, drill that version separately.
