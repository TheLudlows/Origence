# Origence · Current verification

**English** · [简体中文](VALIDATION.md)

The latest local verification was 2026-11-27. Each section identifies its exact source revision. The 2026-11-17 records are synchronized to main [`92258fc`](https://github.com/TheLudlows/Origence/commit/92258fc578bbdb1c21ca949ddbbcfb1729a5813a). The latest local results appear first. Historical results apply only to their listed revisions and do not describe current CI. See [STATUS](STATUS.en.md) for capabilities and follow-up work. This page records evidence and limits.

## 2026-11-27 · Project cleanup-after-change skill

Based on main `241374ed0f960a4aff67acb3e62a4c09ff5cea0f`. This change only updates project instructions, the skill, and documentation references.

- Reviewed the 5 sections and 27 rules in both skill languages, YAML frontmatter, interface metadata, and implicit invocation. `git diff --check`, `git diff --cached --check`, and Python 3 / PyYAML checks passed.
- Maintain only the root `AGENTS.md`. Confirmed that its translated mirror had no unique constraints before removing it. Audited all 80 Markdown files and 120 script, workflow, and configuration files at commit `845aeab839716c6a32dc7741cfa727ad13019ed7`, repaired current navigation and the SQL-constraint anchor, and documented the exception to bilingual maintenance. No bilingual check script depended on the mirror.
- Reference and anchor checks passed; other historical validation entries remain intact. Runtime code was unchanged, no tests were added, and Rust or Node.js builds and test suites were not run. These checks do not establish runtime or CI validation results.


## 2026-11-27 · Private Q&A on fresh databases

Based on `c385544f8723f30f51654ac79d1c64143ff47716` on Linux x86_64 with Rust 1.98.0.
The standalone AnySeek server used CI pin `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`.

- `cargo test --locked` passed 481 tests. Eight real-backend AnySeek contracts also passed.
  All-target Clippy passed with `-D warnings` for SQLite only and all features.
  Formatting, documentation links, and diff checks passed.
- `tools/private_archive_smoke.py` exercised the CLI, HTTP, and MCP with both backends.
  It covered private search and shared search isolation, ordered reads, credential rotation,
  failed commits and lost receipts, withdrawal races, Run replay, SQLite rejection of
  private Q&A writes, and a paired restore with `archive-verify`.
- Synthetic databases from before #168, from #168, and from the preceding #170 commit
  were rejected by both `serve` and `archive-verify`. The SQLite file SHA-256 remained
  unchanged. Local validation covered Linux and stopped copies.

## 2026-11-27 · Native private turn directory and direct experience publication

Based on main `a542ff0af5cd6ac8a315d6fe8d4a6c5f99734f57` on Linux x86_64 with
Rust 1.98.0. Standalone AnySeek used CI pin `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`.

- The default Rust suite passed 482 tests and the real AnySeek contracts passed another 8.
  All-target Clippy with warnings denied passed
  for SQLite only and all features. Formatting and links passed.
- The private archive smoke passed through the actual CLI, HTTP, and MCP on both backends.
  It checks the absence of a SQLite directory row for ordinary turns, exact and ordered reads,
  key and workspace isolation, lost receipt recovery, late writes and withdrawal races, mixed
  Run source checks, and stopped paired restore.
- Isolated tests ran `copy / verify / purge` for the pre-#168 SQLite bodies and explicit
  admission of a #168 store. They compared original SQLite bytes, old IDs, withdrawals, and
  mixed pagination order. Five concurrent first submissions returned the same receipt.
- With models disabled, HTTP and MCP published experiences from human confirmation and
  a pinned document version without a private turn archive. Both backends returned the
  published experiences and hid the document-based experience after source withdrawal.
  Existing Improve and effective-date fact paths retained their separate behavior.

All checks used disposable synthetic data. Native Windows, production data migration,
and online restore remain outside this validation.

The follow-up review starts from `97d0bc6e264250c8cc939d3c66a294f22a4c19c7`.
The actual CLI, HTTP, and MCP smokes ran again with embedded and standalone AnySeek.
A missing known archive group returns 503 on source, turn, and context reads. Withdrawing
an unreferenced turn by source ID creates no SQLite dependency proof. The restore manifest
changes with a logical withdrawal and stays fixed when cleanup removes the withdrawn record.
A concurrent Run preparer replays the committed result and leaves proofs only for the
selected turns. Direct publication and Improve Learning regressions also passed.
The follow-up default Rust suite passed 482 tests and the real AnySeek contracts passed eight.
Strict SQLite-only and all-feature Clippy, formatting, and link checks passed.

## 2026-11-27 · AnySeek private turn archive

Origence is based on `af9030310bd5d313e23d9ac42373fa0b1e7d38a7`. The embedded SDK remains
`5626dadfd5414b294a3cf0f6b845f5261760f68b`. Standalone AnySeek was built from the CI pin
`6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a` with Rust 1.99.1. Origence verification used
Rust 1.98.0 on Linux x86_64 and synthetic turns in disposable stores.

- `cargo fmt --package origence -- --check`, `cargo clippy --locked --all-targets -j 2 -- -D warnings`,
  `cargo test --locked -j 2 --no-fail-fast` (484 passed), and `git diff --check` passed.
  `python3 tools/anyseek_smoke.py --anyseek-binary <CI fixture> --cargo-tests` passed 8 additional
  real-backend contracts with just `local-storage`. Compilation checks passed with no default
  features and with the standalone `sqlite` and `local-storage` feature sets.
- `python3 tools/private_archive_smoke.py --binary <origence> --legacy-binary <af90303-binary>`
  passed through the embedded CLI, HTTP, and Streamable HTTP MCP. Adding
  `--anyseek-binary <CI fixture>` passed against the pinned standalone server. Both covered exact
  and ordered reads, UTF-8 limits, keyword, synthetic vector and hybrid search, scope separation,
  credential rotation, shared search isolation, Run replay and legacy prefix verification, batched migration, explicit purge,
  repeat execution, and stopped paired restore. Restore compared the ordered directory digest,
  SQLite confirmation sequences, engine version, and AnySeek visible sequences.
- The standalone run injected a pre-write failure, an expired epoch, a lost write receipt, missing
  group members, a hash mismatch, a late write after withdrawal, and revocation during delivery.
  It killed the offline migration process after the native commit and before SQLite confirmation.
  Re-running recovered the same turn. SQLite files and Run replay were checked for leaked bodies.
  The Improve regression checked that snapshots keep references and durable quotes are digests.
- Windows, real embedding quality, object storage and online/cross-version snapshots, and migration
  of real sessions remain outside this validation.

The PR was rebased onto main `8d6605ff6dcb4ceaa60c72cd5a9106dcd2525461`. The embedded
and pinned standalone private-archive smokes, all 8 real-backend contracts, and the full live
HTTP MCP smoke ran again. Mixed context kept #165's before/after values and exact provenance
and denied the private route to readers. A new standalone test withdrew a cited shared source
while a mixed Run replay waited on a private body read. Replay returned 404 and the authorized
private turn remained readable. The purged synthetic authority files passed the body check
and the untouched pre-cutover backup still exposed its original to authorized test code.
Verification covered active files and logical visibility. Physical erasure of SQLite pages,
historical WAL, and backups was outside the test.

The #168 cleanup starts from `4bfab721e981348024b14a7053d6e4568443e720`. Strict all-target
Clippy passed with SQLite only and all features. The default Rust suite passed 478 tests.
Seven internal tests for the retired SQLite body scan were removed. Current Context, Run replay, and Improve tests
still cover authorization and evidence references. The embedded and pinned standalone archive
smokes verify that an invalid copy limit leaves the schema untouched, an unmatched legacy source
blocks purge, and a source of the wrong kind cannot be read or retrieved. The standalone smoke
also withdraws a turn while Context waits on the private body read and checks that final delivery
omits it. Purge leaves the pre-cutover synthetic backup intact.

## 2026-11-27 · Dated reimbursement policy in everyday context

Based on main `1a8544b647613864582354bc9f5942e69426b463` on Linux x86_64 with Rust 1.98.0,
Node.js 24.22.1, and Chromium 156.0.8078.79. HTTP `/v1/context` and MCP `context_bundle`
covered a natural Chinese query, effective dates, same-day conflicts, missing evidence, byte
budgets, workspace isolation within a tenant, and recomputation after withdrawal. Chromium
used the built console, live local host, and worker to verify original UTF-8 highlighting for
writers, pinned published excerpts for readers, and denial when reopening a withdrawn citation.

The default Rust suite passed 485 tests with 8 AnySeek tests ignored by default. Formatting,
strict all-features all-targets Clippy, 31 UI tests, embedded HTTP MCP smoke, 3 MCP schema tests,
and both Chromium context citation regressions passed. [Commands](VALIDATION.md#2026-11-27--日常上下文召回带日期的报销上限)

An extraction fixture supplied synthetic responses. Windows, live models, native Agent clients,
and the official pilot remain unverified.

PR #165 review added a confirmed explicit value alongside a dated topic with the same full
identity. Before the fix, the HTTP historical response returned both values. The HTTP and MCP
regression covers historical, current, unknown, conflicting, and budget-limited reads. It also
checks that an extra identity condition keeps a separate value eligible and that conflict
reporting follows the selected business time.

After syncing main `af9030310bd5d313e23d9ac42373fa0b1e7d38a7`, HTTP and MCP Run preparation
returned `INVALID_ARGUMENT` for `as_of_ms` after checking write access and ownership. Neither
request recorded an event, and preparation without the date succeeded using the same event ID.
The standalone HTTP/MCP date recall and identity/budget regressions passed on the updated main.

## 2026-11-27 · Draft Skill candidates from Run details

Based on main `1a8544b647613864582354bc9f5942e69426b463` on Linux x86_64 with Rust 1.98.0,
Node.js 24.22.1, and Chromium 156.0.8078.79. The real console and Origence ran on loopback
with a fresh temporary directory and synthetic data. Account sign-in and Run navigation
verified the exact prefix, explicit disclosure, idempotent retry after a lost save response,
reopening the candidate, invalidation of approval after a basis-only edit, and narrow layout.
The same host verified reviewer isolation, publication permission, package contents,
cross-workspace denial, and source withdrawal. The official pilot and native client loading
were outside this run. See the [commands](VALIDATION.md#2026-11-27--从-run-详情起草-skill-候选).

## 2026-11-27 · Restore AnySeek default keyword AND

On main `0c1f815bf579c8c653e47e2cc8ea254bc7e182b7`, a partial-term query matched incorrectly.
The original real-backend suite had 7 passes and 1 failure. The fix was tested on main
`1a8544b647613864582354bc9f5942e69426b463` on Linux x86_64 with Rust 1.98.0. The standalone AnySeek
fixture was built from the CI pin `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a` with Rust 1.99.1.

`python3 tools/anyseek_smoke.py --anyseek-binary <anyseek_server> --cargo-tests --binary <origence>`
passed all 8 real-backend contracts. The public Rust library, HTTP, and Streamable HTTP MCP covered
default keyword queries with all, some, or none of the terms present. The suite also checked cross-workspace
isolation, key revocation, source withdrawal, citation locations, publication and Run replay, and restart.
The default Rust suite passed 484 tests, with the 8 real-backend contracts run separately. Formatting,
strict all-target Clippy, embedded AnySeek startup and restart, and separate HTTP, MCP, and Node UI smokes
passed. All 24 synthetic keyword seed queries completed with zero query errors. Verification used synthetic
data and zero model calls. Windows, real-model retrieval quality, and the official pilot remain open.

## 2026-11-27 · Run overview and roadmap cleanup

Based on main `1a8544b647613864582354bc9f5942e69426b463` on Linux x86_64 with Node.js 24.22.1, Chromium 156.0.8078.79,
and Rust 1.98.0. `npm --prefix ui run check` and `npm --prefix ui test` passed (31 tests). Chromium passed
`check-navigation-focus-browser.cjs` and `check-console-session-browser.cjs`, covering trends, metering, responsive navigation,
retry focus, and account sessions. Browser sign-in against local Origence, the Node console, and fresh SQLite verified the
24-hour and 7-day trends for 26 synthetic Runs, reported duration, tokens and cost, and loading the 26th Run through the Run list.
The overview displayed service readiness with the page-count card removed. Replaying the legacy field with the same idempotency key
returned the same receipt. Changing only `publish_if_authorized` returned HTTP 409.
`cargo fmt --all -- --check` and `cargo clippy --locked --offline --all-features --all-targets -- -D warnings` passed.
`cargo test --locked --offline --no-default-features --features local-storage --lib --test local --test memory-consolidation --quiet`
passed 239 library, 218 local integration, and 6 long-term memory tests, including Run statistics and metering and both legacy
extraction endpoints and controlled extraction. Private integrations and formal pilots were untested.

## 2026-11-27 · Simplify standalone evidence capture and display

Based on local extraction commit `0d84a6c99ad6271af8c74f4274c0b3489e2f043d`. Local HTTP
tests used synthetic model responses and checked version-free targets, the saved admission
version, idempotent replay, stale explicit versions, and concurrent first publication.
Older standalone extraction evidence remained readable by fixed version.

239 library tests, 217 local integration tests, 6 long-term memory tests, formatting, and
strict all-feature all-target Clippy passed. Node type checking and 31 UI tests passed.
The UI tests covered the capture form, standalone evidence details, and fixed references.
Chromium passed the asset tabs and version comparison regression at 320, 375, 736, and
1024px in both color schemes and reader/writer roles. The verification ran on Linux.

On `a989e7f2fcb4bb5f642ee28590f6ddd914b82e8c`, the two older extraction endpoints now share
publication logic, and asset reads fetch identity in the existing query. The Rust, Node, and
Chromium checks above passed again on Linux, including authorization, fixed-version reads,
withdrawal, and replay.

## 2026-11-27 · Append extracted evidence

Based on main `c1ab0d5a82b5e098f699288c78dcb6b1f99f0cf2` on Linux x86_64 with Rust 1.98.0.
239 library, 217 local integration, and 6 long-term memory tests passed. Formatting, strict
all-target all-feature Clippy, Node type checking, and 30 UI tests passed.

A local HTTP service with synthetic model responses verified separate conflict evidence, rejection
of the former `allow_update` field, explicit correction, retry and withdrawal. Both older extraction
endpoints kept the existing version while saving separately retrievable evidence. Long-term memory
tests checked effective dates, same-day conflicts, and recomputation after withdrawal.

## 2026-11-26 · Incremental private turns for Codex and Claude Code

On main `a5fbcd230795e2132362b6d5ae9a0240a8b36e53` with Linux x86_64 and Rust 1.98.0,
synthetic hooks in separate processes and a real HTTP host covered pairing, redaction, offline staging,
byte-for-byte replay, concurrency, conflicts, scope, withdrawal, and unknown evidence use in retrieval
and Improve. Formatting, strict all-features Clippy, 239 library tests, 216 local integration tests,
and 3 Python MCP schema tests passed. HTTP MCP smoke passed with embedded AnySeek and the CI-pinned
standalone AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`, built with Rust 1.99.1.
Native clients, live models, and Windows and macOS paths await verification.

On 2026-11-27, review cleanup on main `224e1897ffe9fd886b706adb3d007a7e1e24e694` passed formatting,
strict all-features Clippy, 239 library tests, 216 local integration tests, 3 Python MCP schema tests,
and standalone AnySeek HTTP MCP smoke. The standalone AnySeek contract suite had 7 passes and 1 failure.
The missing-term search assertion at `tests/anyseek.rs:145` also failed on clean main.

## 2026-11-26 · Management console visual update

Based on main `a5fbcd230795e2132362b6d5ae9a0240a8b36e53`. Node.js 24.22.1 type checking,
30 UI tests, and Rust 1.98.0 formatting and strict Clippy passed. Chromium 156.0.8078.79 used
loopback synthetic data to verify the side-by-side chart and metering at 1440px, stacking at
1024px and 320px, light and dark navigation at 736px and 375px, sign-in, workspace switching,
keyboard focus, and retry. Measured text contrast on knowledge details ranged from 6.0259:1
to 8.1043:1. A coarse pointer was emulated at 320px, 375px, and 736px to check hit targets for
sign-out, dialogs, and disclosure controls.

See the [Chinese record](VALIDATION.md#2026-11-26--管理控制台视觉更新) for the commands.

## 2026-11-26 · Run creation chart

Based on main `78f08ab95494eb9cca9944e3a87fa0181374e1ef`. Node.js 24.22.1 type checking and 30 UI
tests passed. Chromium 156.0.8078.79 used loopback synthetic data at 1024px in light mode and
320px in dark mode to check the 24-hour and 7-day intervals, zero baseline and proportional columns,
local timestamps with millisecond boundaries, keyboard and hover, an emulated tap, collapsed
details, and retry focus. The 736px and 375px light and dark navigation checks passed. Under
Rust 1.98.0, formatting, strict Clippy, and two local integration tests for Run counts and metering passed.

```sh
npm --prefix ui run check
npm --prefix ui test
NODE_PATH=/path/to/playwright/node_modules CHROMIUM=/path/to/chromium \
  node ui/scripts/check-navigation-focus-browser.cjs
cargo test --locked --no-default-features --features local-storage --test local catalog::run_
```

## 2026-11-26 · Separate Agent setup and local deployment

On main `13e2a4cb7bea7e97f67525d138453b6f9a0fdba1`, Linux tested native Claude Code, Codex, and
Chromium. Deployment started the API and console. Agent setup connected both clients while the
console was running, with Codex using a non-default API port. The smoke test checked legacy journal
upgrade, repeat install, invalid credentials, Hook Run readback, independent rollback, retention
of a user-edited Codex MCP entry, and data backup. Node type checking, 30 UI tests, Rust formatting,
and strict Clippy passed. The native Windows workflow includes the split test, which has yet to
run on Windows or macOS. [Installation guide](INSTALLER.en.md)

Run `python3 tools/installer_smoke.py --binary target/debug/origence --claude /path/to/claude --codex /path/to/codex`.
Add `--browser` and set `NODE_PATH` and `CHROMIUM` for Chromium.

## 2026-11-26 · Origence local installer

Based on main `b4e8879fa952b5afd729fd5b89a630c6d5705696`, Linux x86_64, Rust 1.98.0,
Node.js 24.22.1, native Claude Code 2.1.330 and Chromium 156.0.8078.79.
The installer started the live local API and console on non-default ports with spaces in the binary
and data paths. MCP initialization, all 11 tools, and session identity passed. Claude Code reported
the installed plugin connected. Chromium signed in with the generated account and read the Run
recorded by the real hook executable from a synthetic event. New Bash and Zsh processes loaded the
persisted settings. Reruns reused listeners and the probe Run, the reader key skipped the write,
an invalid key returned 401, reinitialization backed up the old database, and uninstall restored
shell settings while preserving a subsequent unrelated edit. A new install reused a running API
and left it up on uninstall. A clean distribution built the console and installed the pinned Claude
package via npm. Strict Clippy, 215 local Rust integration tests, Node type checking, 29 UI tests,
and the existing native two-client synthetic plugin check on a non-default port passed.

Reproduce with `python3 tools/installer_smoke.py --binary target/debug/origence --claude /path/to/claude`.
Set `NODE_PATH` and `CHROMIUM` and add `--browser` for the Chromium check. The run used loopback
and synthetic events with no live model. The native Windows acceptance workflow can exercise registry
rollback and fresh process inheritance. Windows and macOS paths have not yet been executed for this installer.
[Installer guide](INSTALLER.en.md)

On installer commit `6f8bf7a`, reran the Linux native Claude Code and Chromium acceptance.
`origence uninstall` rolled back shell and PATH changes from outside the checkout and preserved the database
and backup. A repeated call skipped, and legacy state pointed to the script. Rust formatting and strict Clippy passed.

Based on main `2c8a7147a7b4f2c6fadd447a256a726d65ba3abc`, standalone `origence-setup install`
and `origence-setup uninstall` passed the native Claude Code and Chromium integration on Linux.
Setup launched outside the checkout, new Bash and Zsh processes found it on PATH, and uninstall
ran outside the checkout. Repeated uninstall skipped. Rollback restored user settings and preserved
the database and backup. The backend help had no setup commands. Rust formatting, strict Clippy,
Node type checking and 30 UI tests passed. The Windows .cmd entry is wired into the native workflow
and has yet to run.

Using main `620188115c89b20bc1411f5ec1e48b93f68548eb` as the base, fix commit
`4e3d95fc3ea495e6beaf7138d4cec6c52a894dc6` passed installer acceptance
on Windows Server 2025 (10.0.26100), Node.js 24, and native Claude Code 2.1.330. A missing
`origence.exe` produced the `--origence` diagnostic before workspace data was created. A first install
through `.cmd` verified the Claude MCP connection, console sign-in, and Hook Run readback. A new
process received the credentials and PATH from the user registry. Reruns reused services and the
probe Run, an invalid key returned 401, and forced reinstallation backed up the original database.
Uninstall from outside the checkout restored the original registry values and types, removed the
Claude Code plugin, and preserved the database and backup. Repeated uninstall skipped. The same
commit passed the native Claude Code and Chromium acceptance on Linux, Rust formatting, and strict
Clippy. These runs used loopback services and synthetic data with no live model calls.
[Windows acceptance](https://github.com/TheLudlows/Origence/actions/runs/45813998221)

## 2026-11-26 · Codex commands and Claude SDK cancellation

Tested `2396dfa3ffd303c0cbfbd63068930fb36926ab8b` on main
`b4e8879fa952b5afd729fd5b89a630c6d5705696`, Linux x86_64, Rust 1.98.0 and
Node.js 24. Formatting, strict local-storage Clippy, 236 library tests, 215
local integration tests, Node type checking, and 30 UI tests passed. Twelve
managed Run fixtures covered command correlation, missing fields, explicit
zero, byte-identical retry, the metadata allowlist, out-of-order cancellation
receipts and results, other prompts, older receipts, timeouts and late
cancellation. The pinned SDK was installed and its `query` export verified.

A loopback check ran the actual Origence HTTP host, temporary SQLite, the Node
relay and Chromium. Synthetic Codex notifications went through the CLI and
were replayed. A Claude SDK fixture exercised all three cancellation stages.
Chromium showed exit codes, durations, unknown values, acknowledgement and
confirmed interruption separately. Private payloads were absent from the Run
and the business outcome stayed unknown. This check used synthetic protocol
events. Managed execution with native clients and live models is unverified.

## 2026-11-26 · #89 End-to-end expense trace preflight

Based on main `3984085ca5872dc68ea44b062e2d0791f92102a3`, using Linux x86_64, Rust 1.98.0,
Codex 0.171.0, and Chromium 156.0.8078.79. The native Codex app-server used HTTP MCP to create a Run,
prepare policy v2, and record a synthetic completion report for 120 units and unknown model-use coverage.
The fixture derived a failed external transcription from the v2 cap of 80, bound it to the first two events,
and verified a successful identical replay and 409 for a conflicting conclusion.

The built console used its Node proxy and account sessions to read the same Run from live Origence.
Chromium distinguished preparation, client report, owner-transcribed failure, and unknown model use. After v3
published, the Run still opened v2. A second key was denied the private Run. Withdrawing the v2 source
blocked reads of both the Run and pinned v2 while v3 remained readable. This was an engineering preflight
with synthetic data and no model generation or independent sign-off. The formal #91 pilot remains
`NOT_RUN`. [Reproduction (Chinese)](../evals/pilot/P6-EVIDENCE.md#端到端追溯预检)

All 214 local Rust integration tests, 28 UI tests, and 58 pilot tooling tests passed on the same baseline,
along with the existing Chromium check for the verification UI.
After syncing main `39d8075e90914e0c51f2416e9309847b3679b419`, the rebuilt Origence and console passed
the end-to-end trace preflight, the existing Chromium verification check, 215 local Rust integration tests,
Node type checking, 29 UI tests, and 58 pilot tooling tests.

## 2026-11-26 · Admin Agent connections

Tested `fd7209dadcde3559e5271d4b8a1c269e202bf943` on main
`3984085ca5872dc68ea44b062e2d0791f92102a3`, Linux x86_64 and Rust 1.98.0.
Formatting, strict `--no-default-features --features local-storage --all-targets`
Clippy, 236 unit tests, and 215 local integration tests passed. MCP and admin
API tests checked grouping, empty results, administrator access, tenant isolation,
workspace grant removal, key rotation, and rebuilding the HTTP router. The unit
test checked concurrent request counts and idle expiration.

Node type checking and 29 UI tests passed. Chromium exercised sidebar keyboard
navigation at 320, 375, 736, and 1024px. An additional loopback test used the
actual Origence HTTP host, the Node relay, and a temporary SQLite console
account. It verified grouping from synthetic MCP handshakes and denied access
after switching to a reader workspace, including direct navigation.

## 2026-11-26 · #76 whole-Run metering

Tested source `abff834cf610d2dcc5c453e34893f32b5c55e5cb` on main
`c6e0b8437f2b4c983deb24e565b403deaccb271d`, Linux x86_64 and Rust 1.98.0.
Formatting, strict `--no-default-features --features local-storage --all-targets`
Clippy and all 214 `--test local` integration tests passed. Real SQLite,
loopback HTTP and Streamable HTTP MCP covered nearest-rank P95, window
boundaries, exact micro-unit totals, zeros and missing data, permission
isolation, key rotation, replay and old-schema rejection. The SQL review
checked that projection writes, grouping and ranking each use one relation.

Node.js type checking and 28 tests passed. Chromium with a loopback fixture
verified display, window switches, removal of stale values on errors and
retry. The executor supplies whole-Run metrics. This verification used test
usage and charges with no live model or provider bill.

## 2026-11-26 · Native Windows plugins and pilot preparation

Product baseline main `318e17178a8c4a24d3000bc3caa6d3250019eb10`, with acceptance script revision
`7116d909df38d3248de9914fc47c3f713d63ba60`. GitHub's native x64 Windows Server 2025 runner
(NT 10.0.26100.0) built Origence with Rust 1.98.0 and ran `tools/plugin_smoke.py`. Claude Code 2.1.330
and Codex 0.171.0 installed from the local marketplace and passed on port 8080 and a non-default loopback port.
One reader key worked in the default and explicitly selected workspaces. Each client found v2 and retrieved pinned
v1 without Origence on its PATH. The writer hook recorded a private Run only in the selected workspace.
The test also covered no reader Run activity, uninstall, and missing and invalid credentials. The log lists the three
native binary SHA-256 digests. [Windows run](https://github.com/TheLudlows/Origence/actions/runs/45778860377)

The same fixture passed on Linux on both ports. The clients used isolated settings, synthetic data, and scripted
loopback models. #89 passed the HTTP and Chromium checks for prepared v2, the exact prefix, transcribed conclusions,
withdrawal, and cross-key denial. #76 passed Chromium touch emulation, focus and accessibility-state checks, and 28 UI tests.
Actual screen readers and physical touchscreens still await acceptance. #91 passed 58 Python pilot tests and a public
preparation-sheet run. The formal freeze is `BLOCKED` on materials, model authorization, and independent roles. The
official trial and independent sign-off are `NOT_RUN`. [Freeze readiness (Chinese)](../evals/pilot/FREEZE-READINESS.md)

## 2026-11-26 · Client plugins move to HTTP MCP

Based on PR #139 at `3d99e7e1c6220a2e7810e4b2921a707730cff6e4`, Linux x86_64 with Rust 1.98.0. All-target Clippy with warnings denied passed for all features, SQLite-only and the cloud build. The default Rust suite passed 469 tests with 8 existing live AnySeek contracts ignored. The stdio startup test for old hosts that ignored workspace selection was removed. Live HTTP tests still cover default and explicit scope, disallowed selection, revocation, and an empty header from a plugin with no workspace selection.

- `python3 tools/mcp_smoke.py --binary target/debug/origence --local` passed negotiation, all tool schemas, safe errors, private archives, revocation, and reads after host restart.
- `python3 tools/plugin_smoke.py --binary target/debug/origence` used Claude Code 2.1.330 and Codex 0.171.0. Both installed the plugin, retrieved a pinned published version without Origence on the client PATH, reported a Run through the default hook with the binary and a synthetic writer key, and uninstalled. A rerun with `--bind 127.0.0.1:0` verified the Claude Code origin variable and Codex's same-name endpoint override. The clients used isolated settings and scripted loopback model responses. Windows clients, an external HTTPS proxy, and live models were outside this run.
- The older standalone AnySeek `6614bbf` available locally returned a dependency error during fixture publication, before any MCP request. The CI-pinned `6a46fc7` still awaits GitHub verification.

## 2026-11-26 · Streamable HTTP MCP

Based on main `85d6264ab720437d8620d3cdb5289ace85a5942c`, Linux x86_64 with Rust 1.98.0. Format and all-target Clippy with warnings denied passed for all features and the cloud build (`--no-default-features --features local-storage`). `cargo test --locked --offline` passed 470 tests with 8 existing live AnySeek contracts ignored. Two new live HTTP regressions cover negotiation, Host and Origin checks, request size, credential revocation, workspace isolation, reader write denial, private archives, and replaying a Run preparation receipt through the public Rust library.

- Native Claude Code 2.1.330 and Codex CLI 0.171.0 used the [direct connection commands](CLIENTS.en.md#direct-http-mcp). Each used temporary settings, a non-default port, a synthetic reader key supplied through `OC_API_KEY`, and a client `PATH` without `origence`. Both called `context_session`, `context_search`, and `context_get` and returned the expected workspace and pinned published version. Claude Code used scripted model responses on loopback. Codex called the tools through its app-server.
- Windows, an external HTTPS proxy, live models, and plugin hooks were outside this run. A local HTTP protocol test covers matching a configured HTTPS origin and non-default port.

The first PR #139 `fast-check` and main `85d6264` both failed SQLite-only Clippy because the `Administrator` variant was unused. The fix compiles the administrator workspace path and its dedicated query under the same `local-storage` feature as the entry point. SQLite-only all-target Clippy with warnings denied passed on Linux, along with 245 tests and 5 existing ignored cases.
[Initial PR failure](https://github.com/TheLudlows/Origence/actions/runs/45771086287/job/139743624086) · [Main failure](https://github.com/TheLudlows/Origence/actions/runs/45642868907/job/139305927994)

## 2026-11-25 · Workspace Run status and statistics for administrators

Based on `607bcdb7b03b9299a9741b4b24014ae9e5b43245` and the child workspace authorization changes in the same checkout. Native Windows with Rust 1.98.0 and Node.js 24.15.0.

- `cargo fmt --all --check`, `cargo clippy --locked --offline --no-default-features --features local-storage --all-targets -- -D warnings`, and `git diff --check` passed.
- `cargo test --locked --offline --target-dir target/run-admin-check --test local --quiet` passed 212 tests. Public Rust library and real HTTP/SQLite checks covered admin UUID pagination, the latest reported result from another owner, hidden summaries, 404 on private detail, non-admin scope, workspace and tenant isolation, and the 24-hour and seven-day windows. Manual review verified that each new SQL statement accesses one relation and that the catalog, status and trend use their authorized read transactions. The database schema is unchanged.
- `npm.cmd run check` and `npm.cmd test` passed, with 28 tests. The DOM fixture verified another owner's reported status and logical key with no detail action on the row.
- The local development API restarted gracefully with the same data directory. An authenticated Node console request returned the workspace scope for `admin`, and direct HTTP returned the logical-key scope for `writer`. Both services reported ready. Linux and Chromium acceptance were not run for this change.

## 2026-11-25 · Child workspace creation by root administrators

Based on `607bcdb7b03b9299a9741b4b24014ae9e5b43245`. Native Windows with Rust 1.98.0 and Node.js 24.15.0.

- `cargo fmt --all --check` and `cargo clippy --locked --offline --no-default-features --features local-storage --all-targets -- -D warnings` passed.
- `cargo test --locked --offline --no-default-features --features local-storage --test local --quiet` passed 212 tests. Real SQLite and HTTP covered creation by ordinary and explicitly granted root admins, same-UUID retries, writer access to the new child, isolation of existing children, and denial for writers and child admins. Public-library owned-workspace regressions continued to cover the explicit capability.
- `npm.cmd run check` and `npm.cmd test` passed, with 28 tests. The console proxy used an HTTP fixture to check root-admin creation, the effective child role, and denial from an unauthorized workspace.
- Manual review confirmed that each changed SQLite statement accesses one relation and that listing and creation use their original transaction boundaries. Chromium acceptance was not run for this change. WSL only had Docker Desktop's internal distribution, and the Linux container engine was unavailable, so Linux regression tests were not run.

## 2026-11-25 · Console API key management

Based on main `b8dc33b77f474f5fc453061c755b9c710c0576ea`. On Linux with Rust 1.98.0, formatting,
all-target all-feature Clippy with `-D warnings`, 212 local tests, Node type checking, and 28 UI tests
passed. Public library and HTTP regressions checked pagination, admin authorization, workspace and
tenant isolation, rotation, and inactive grants after revocation.

`node ui/scripts/check-key-management-browser.cjs <ORIGENCE_BINARY>` used Chromium 156 against the
Node console and a disposable Origence `serve --local` host. It checked issuance, copying, role changes,
restoring default access, rotation, revocation, cross-workspace grants, reader denial, and one-time token
display. Upstream authentication confirmed old-token rejection. Browser storage and host logs contained
no fixture token. The account, focus, and narrow viewport scripts passed. Native Windows was not run.

## 2026-11-25 · Default plugin Run hooks

Based on main `b7ce2679ddadda045f35fb108b63cc822a68029c`, on Linux with Rust 1.98.0.
Formatting, Clippy with all targets and features and `-D warnings`, and all 211 local tests passed,
including six hook regressions.
The CLI fixture covered explicit and default workspace selection, mismatched scope, reader access,
the disable switch, retry after receipt loss, and exclusion of raw prompt text.

`python3 tools/plugin_smoke.py --binary target/debug/origence --claude <CLI> --codex <CLI>`
installed both plugins from a local marketplace in fresh projects using Claude Code 2.1.330 and Codex
0.171.0. Reader retrieval, writer prompt and Stop events, absence of handwritten project config,
uninstall, and startup diagnostics passed. Claude Code also recorded the plugin's own search and
get calls. Codex first showed four enabled hooks awaiting trust. The fixture allowed them for a
single isolated turn. Both model endpoints were scripted on loopback. Native Windows installation,
remote marketplace fetches, and complete lifecycle coverage remain unverified.

## 2026-11-25 · Visible workspace list after sign-in

Based on main `cadce521a09a65e9c59f8feb50c192f5b7649c41`. On Linux with Node 24.22.1,
type checking, 27 UI tests and the diff check passed. Chromium 156 with synthetic accounts verified
that the sidebar shows every granted workspace, including an existing child. A separate account saw
only its single workspace. Keyboard switching, mobile touch emulation, child creation retry, restoring
the active workspace on reload, and sign-out passed. Navigation focus and narrow viewport scripts passed.

## 2026-11-25 · Default console account sign-in

Based on main `c3a28a851b06e4831eae16ff1cff630316f3d3bc`. On Linux with Node 24.22.1, type
checking, 27 UI tests and the diff check passed. Tests covered startup with no account database setting,
API-key sign-in rejection, creation of the default account database, and account authentication. An
isolated Node console and synthetic HTTP upstream also passed Chromium 156 checks for the fresh setup
prompt, first account provisioning without database flags, sign-in, reload and sign-out. Seven existing
Chromium scripts passed for workspace accounts, asset tabs, knowledge evidence, knowledge jobs,
narrow viewports, navigation focus and Run coverage. All data and credentials were synthetic.

## 2026-11-24 · #132 main sync and CI fix

Synced main `9924ec3f31eccacc2c164129be01640923f61547`. The SQLite-only Clippy command reproduced
`dead_code` on two host-only methods. Gating them on `local-storage` fixed strict Clippy.
The SQLite-only suite passed 245 tests, with five external AnySeek cases ignored as configured.
The full host's `cargo test --locked --no-default-features --features local-storage --test local`
passed 210 tests. Strict Clippy and formatting passed.
The combined HTTP regression checks a derived writer and an explicit reader on the same child
workspace, and verifies denial for an ungranted key and a root key from another tenant.
Node type checking, all 26 UI tests, and the Chromium 156 create, retry, switch, reload, and logout fixture passed.

## 2026-11-24 · Multi-workspace API access

Based on main `8226b4c1492c2a8f480977f10fbddc7379aaca18`. Linux, Rust 1.98.0, fresh temporary databases.
`cargo test --locked --offline --no-default-features --features local-storage --lib --test local`
passed 235 library tests and 209 integration tests. The new regression uses real SQLite, Origence HTTP, a stdio
MCP process, and an AnySeek protocol fixture. It checks scoped search, workspace roles, tenant and archive
isolation, revocation without fallback, and grant and session continuity after rotation. A separate
default-feature build passed the same-tenant bootstrap and two scoped stdio session, search, and pinned
version reads on embedded AnySeek with models disabled. Formatting,
strict Clippy on all `local-storage` targets, and the SQLite-only library check passed.
Native in-chat switching and Windows plugin installation in both clients remain to be verified.

During review of #134, a loopback fixture emulated an older host ignoring `X-Origence-Workspace`.
The original MCP process started and read the default workspace. The fixed CLI rejects this response
before serving stdio tools, and the regression checks that its error omits the test credential.

## 2026-11-23 · Console child workspace creation

Base `8226b4c1492c2a8f480977f10fbddc7379aaca18`. On Linux with Rust 1.98.0, formatting, strict `local-storage` Clippy for all targets, and `cargo test --locked --no-default-features --features local-storage --test local` passed (208 tests). Real SQLite and HTTP covered administrator capability, bounded pagination, same-UUID retry, cross-tenant denial, derived roles, private Run isolation, and credential revocation. The new SQL reads only `oc_workspaces` scoped by tenant and parent. Creation reuses the existing transaction and audit path.

Node 24.22.1 passed type checking and all 26 UI tests. Chromium 156 verified retry with the original UUID after a simulated lost receipt, automatic workspace switching, reload, and hiding the entry in unauthorized workspaces. A temporary SQLite console account and real Origence HTTP covered unauthorized creation, retry, and the child catalog with models disabled.

## 2026-11-23 · SQLite configuration tables for console accounts

Base account commit `6a47e21edb2965418e9dbcd65089c3fb8aea145b`. On Linux with Node 24.22.1, type checking, all 26 Node tests, and `git diff --check` passed. SQLite tests covered reopening, password changes that retain grants, revocation, account removal, duplicate credentials, and incorrect workspace mappings. Chromium 156 passed login, workspace switching, reload, and sign-out. The provisioning CLI and real Origence HTTP verified SQLite-backed login and scoped access to two temporary workspaces with models disabled.

## 2026-11-23 · Local console accounts and workspace switching

Base `b182b84ca4d1e73b32b931713280671f13cac974`. On Linux and Node 24.22.1, `npm run check`, `npm test` (25 tests), and `git diff --check` passed. Seven Chromium 156 checks covered account login, workspace switching on a narrow viewport, reload, sign-out, navigation, sources, knowledge ingestion, and Run reads. HTTP fixtures covered unauthorized workspaces, revoked grants, stale tab sessions, and key isolation between accounts.

The real Origence HTTP server and Node console validated file-based account login, `whoami` for two temporary workspaces, session restoration, and sign-out with models disabled. The full `tools/ui_smoke.py` received a 502 while polling a publication job. The main-branch UI failed at the same step against the same backend and local AnySeek `6614bbf`. Rust 1.98.0 formatting and strict `local-storage` Clippy passed.

After rebasing the account changes onto terminology commit `dfb2b2dbb59164cbe0f0cbc23bd515188e9ca329`, the UI rebuilt successfully and passed type checking and all 25 tests. Chromium verified the updated knowledge catalog heading after account login, workspace switching, and reload. The UI source and generated distribution were checked for legacy labels.

## 2026-11-23 · Memory, knowledge, and Skills terminology

Base `b182b84ca4d1e73b32b931713280671f13cac974`. On Linux with Node 24.22.1, `npm run check` and all 22 `npm test` tests passed. Chromium 156 passed the knowledge-ingestion retry, keyboard navigation and focus, and narrow-screen touch emulation checks. Synthetic HTTP responses covered the catalogs, job receipts, and pinned-version details.

On Rust 1.98.0, `cargo fmt --all -- --check`, `cargo clippy --locked --no-default-features --features local-storage --all-targets -- -D warnings`, and `cargo test --locked --no-default-features --features local-storage --test local mcp` passed (3 tests). This covered reading a memory through MCP `context_get` with `asset_id`. The bilingual Run contract links and existing HTTP route examples were also checked.

## 2026-11-23 · Local and cloud AnySeek builds

Linux / Rust 1.98.0. The default build passed 459 Rust tests plus embedded CLI publication, Run replay, and restart checks. The cloud build uses `--no-default-features --features local-storage`. Its Cargo dependency tree excludes the AnySeek SDK. All 6 normal AnySeek tests passed, including refusing to start without remote settings and rejecting `--local`. All 8 standalone AnySeek contract tests passed. Real HTTP and MCP covered publication, pinned-version readback, and restart.

The Dockerfile build settings and Compose default entry point were inspected. No Docker engine was available. Verification used a host binary built with the same Dockerfile features.

## 2026-11-23 · Embedded storage through the AnySeek Rust SDK

Linux / Rust 1.98.0, AnySeek SDK pinned to `5626dad`. The default Origence suite passed 459 tests. All 16 Python tool tests passed, with 5 using PowerShell 7.4.13. Standalone HTTP passed 8 real AnySeek contract tests, plus HTTP and stdio MCP, pinned-version reads, and restart. The public Rust SDK tests cover exclusive open, tenant isolation, idempotent atomic replacement, shutdown, and reopen. [AnySeek SDK PR](https://github.com/TheLudlows/anyseek/pull/218)

Embedded mode started real Origence with an empty `PATH`. Verified publish and update, keyword search, Run diagnostics without HTTP status, exact replay, current and historical text after reopen, and refusal to rebuild a missing bound Catalog. Additional Rust tests cover vector retrieval, one-hop graph candidates, text reads, and deletion for the same record. All five AnySeek snapshot and recovery regressions passed, covering multiframes, original field order, write receipts, and reopen of vectors and nested JSON. Native Windows Terminal shutdown was not tested locally.

## 2026-11-23 · Origence-managed local AnySeek

Baseline `a01fc9c`. Local Linux Rust 1.98.0 formatting and `git diff --check` passed. All 16 Python tool regressions passed with PowerShell 7.4.13, including 5 covering the Windows script's argument forwarding, paths with spaces, UI readiness order, and failure handling.

Linux CI at `3a9e58d` passed strict Clippy, default Rust tests, and real AnySeek `6a46fc7` integration. `serve --local` passed first initialization, port conflict, full-text write and update, SIGTERM exit, current and historical text and retrieval after reopening the same directory, and refusal to rebuild a missing bound Catalog. Existing standalone AnySeek HTTP, MCP, and UI smokes also passed. [CI #482](https://github.com/TheLudlows/Origence/actions/runs/45109060442)

Native Windows CI build, Clippy, and local lifecycle tests passed. Foreground Ctrl+C and reopening the real AnySeek subprocess were not tested in a Windows terminal.

## 2026-11-21 · #91 offline binding of frozen tasks and P6 receipts

Baseline `1f25c9912bdc7341892aca77b906f66a23b9c14b` (#121). The synthetic cases start from an independently supplied frozen digest and trace it to the original task-set bytes, held-out observation rows, actual handoff records, and exact Run prefixes. They then verify the stored original sign-offs and owner receipts. See [P6 evidence](../evals/pilot/P6-EVIDENCE.en.md). Linux Python 3.12.14 passed 54 offline pilot tests (18 new), 42 evaluation adapter tests, and 11 tool tests with 7 skipped. The public index CLI and `git diff --check` passed. Structural linkage does not establish signature verification, live revocation checks, or business success. Live clients, models, the formal pilot and independent sign-off were outside this run.

## 2026-11-21 · #91 offline scoring and P6 index preflight

Baseline `ddf7fc99ed3c64011592d14406dc6dcdd889865b`. The prior scorer could count a synthetic row with `reviewer: true` and `evidence: [null]` as success. The fix rejects invalid reviewer and evidence references, retains preregistered denominators, and marks group metrics `OBSERVATION_SHEET_ONLY`. The P6 checker compares Run prefix fields and raw index and evidence bytes. See [P6 evidence](../evals/pilot/P6-EVIDENCE.en.md). Public examples stay `NOT_RUN`.

Linux Python 3.12.14 passed 36 pilot and 42 evaluation adapter tests, plus 11 tooling tests with 7 skipped. Public example CLI and `git diff --check` passed. No real clients, models, formal pilot, or independent sign-off. Index preflight does not verify frozen tasks, evidence truth, original sign-off, or owner receipts.

## 2026-11-19 · #92 integration with main #108

Integrated main `2094f81c990aaf7d69d4ff09295c1a2a837c4ba1`. Passed 24 offline pilot tests and a public synthetic scoring CLI smoke. Repeated score was byte-identical. The scorer rejects another batch in the same split, preparation batches, blank freeze templates, and public rehearsals. Both scoring modes use four groups with 8 tasks and 32 checks each.

Four #108 CI checks passed at `96fb05052ae70eca63ae5b8ad1fba72abef84b7f`. Integrated #92 uses its own revision's CI. Real clients, models, formal pilot, and independent sign-off remain pending. [#108 CI](https://github.com/TheLudlows/Origence/actions/runs/44439456768)

## 2026-11-19 · #92 integration with main #106

Integrated main `533658b4f378228f888c1c9b27b5d9b119e19c6b`. Passed 24 offline pilot tests and public synthetic CLI smoke. Reproduction checked independent freeze digest, byte-identical repeated inputs, rejection of wrong batches and public rehearsals, and original denominators.
#106 hook fixtures, #87 Windows integration, and #91 public rehearsal retain their original coverage.
Product clients and models did not run here. Actual hook capture, formal pilot, and independent sign-off remain pending.

## 2026-11-18 · #105 offline Codex and Claude Code hook verification

Linux x86_64 (6.18.57), Rust 1.98.0, main `e024b78eb2ecbf1aa1851783c89be8238ca819a1`. `cargo fmt --all -- --check`, `cargo clippy --offline --locked --all-targets -j 2 -- -D warnings`, and `cargo test --offline --locked -j 2` passed. Total 455 Rust passes and 8 ignored existing real-AnySeek tests. The 3 Python MCP schema regressions and JSON parsing of three integration examples and two redacted fixtures passed.

- Five hook fixtures cover disabled mode with no queue or HTTP requests, project/client boundaries, missing identities, reordering, deduplication, concurrency and capacity, durable recovery after refusal, byte-for-byte replay after missing receipts, and dequeue isolation after configuration or credential changes.
- Codex fixtures check Stop, main-thread Interrupt, structured MCP tool errors, and unknown Bash status. Claude Code fixtures check start/stop within one prompt ID, tool success/failure, optional duration, abort, API error, and explicitly correlated subagents. Both check sensitive placeholders stay out of queue and HTTP payloads.
- Synthetic stdio MCP connects to actual local Origence HTTP and SQLite to verify explicit sharing, including MCP-first and hook-first creation, other-key read denial, reader write denial, old items held after rotation/revocation, and new events in the original logical key's Run. Existing MCP, Run, PDF source locator, and regression cases also passed.

First Windows Clippy on draft PR #106 failed on `unused_mut` because the mutable directory builder was only used by Unix permissions. The fix explicitly creates a `0700` directory on Unix and performs ordinary directory creation inheriting the private parent's ACL on Windows. Linux reran format, all-target Clippy, five hooks, two targeted MCP/Run tests, and aggregate regressions successfully. [First Windows CI log](https://github.com/TheLudlows/Origence/actions/runs/44326867155/job/134880390115)

On 2026-11-19, based on main `e663c3a723262450b639680752a1e10ed69a43f8`, corrected the concurrent fixture's queue-lock assertion.
In #92 Windows CI, eight threads submitted the same event concurrently. Four did not acquire the lock within the 75 ms budget.
That produced `hook queue busy`. The fixture now requires exactly one enqueue and verifies contending submissions deduplicate after contention. Another event cannot change the queue while the test holds the lock. Existing sender isolation and capacity assertions continue. Linux Rust 1.98.0 format, all-target Clippy, 5 hooks, and 2 targeted MCP/Run regressions passed. `cargo test --offline --locked -j 2` passed 455 and ignored the 8 existing real-AnySeek cases. [Original Windows CI log](https://github.com/TheLudlows/Origence/actions/runs/44436948640/job/135268079885)

Current evidence is from a local fixture. Real Codex and Claude Code hook loading, actual event and identity coverage, Windows file behavior, real models, and the formal pilot still await acceptance. Claude Code lacks a reliable user interruption signal for the whole turn, and events without a correlation ID are skipped. Explicit MCP correlation requires the caller's exact IDs. Automatic conversation-body archiving is follow-up work. [Client integration and coverage](CLIENTS.en.md#enable-private-run-hooks-per-project)

## 2026-11-18 · #91 offline tasks and score sheets

Windows based on #92 `2f29c8c`. Two public seeds retested cross-set duplicate rejection, mismatched observation sheets, inserted cross-batch rows, and task-file replacement. All 22 pilot tests passed. The earlier evaluation suite found 42 tests, with 40 passed and 2 skipped. Rust 1.98.0 `cargo fmt --all -- --check` and `cargo clippy --offline --locked --all-targets -j 2 -- -D warnings` passed. [Reproduction](../evals/pilot/RUNBOOK.en.md#question-sets-and-scoring-tables)

## 2026-11-18 · #91 public two-scenario cross-client handoff

The public Windows rehearsal completed two-way handoff for database troubleshooting and expense auditing. [Individual results, failed attempts, and permission checks](../evals/pilot/VALIDATION.en.md#public-dual-scenario-cross-client-replay-2026-11-18)

## 2026-11-18 · #91 both clients load the same Skill version

Native loading succeeded in both clients. After source withdrawal, new export returned resource not found. Existing local files still matched the published digest. [Evidence and limits per client](../evals/pilot/VALIDATION.en.md#dual-client-native-skill-loading-2026-11-18)

## 2026-11-18 · #87 cross-client archive reads and independent Runs

Windows, main [`4160fd5`](https://github.com/TheLudlows/Origence/commit/4160fd5a7cbd7e21af54903a9471fcda15f5f08c), AnySeek `6a46fc7`. Fresh workspace, same temporary writer key provided to both clients. Claude Code 2.1.327 reported `glm-5.3`. Codex CLI 0.170.0 selected `gpt-6.2-sol` and its events did not report precise model revision. Total 62.26 seconds.

- Claude Code explicitly archived Q&A containing a random answer, then created and prepared the first Run. Codex read back by turn ID, archived a separate Q&A, and created and prepared its own Run. A fresh Claude Code session read by the second turn ID. Neither prompt supplied its target answer. Both final outputs contained the other's random answer. The server verified Q&A and both preparation receipts by ID.
- Replaying preparation over HTTP with the same credential retained receipt and event list. Reusing the event ID with a changed query returned 409. A vector preparation without embedding returned 503 and appended a server-observed preparation failure.
- A different administrator in the workspace received 404 for archive and Run. Rotation made the old credential return 401. The replacement retained session, archive, and preparation receipts. After revocation, reads and replays returned 401.

This covers [#87](https://github.com/TheLudlows/Origence/issues/87) integration acceptance. Business answers, native Skill loading, and independent verification belong to the [#91](https://github.com/TheLudlows/Origence/issues/91) pilot.

The repository [reproduction script](../tools/live_archive_smoke.py) completed the same checks on Windows using these main and AnySeek builds in 67.68 seconds. Both readbacks returned the right random answer. Receipts, conflicts, failure events, and credential isolation passed. During development an old binary returned 404 for missing session endpoint before launching clients, and another attempt could not verify the peer's answer from Codex output. The final rerun reads Codex's final message file explicitly. Native Linux execution of the script remains pending.

Rust 1.98.0 `cargo fmt --all -- --check` and `cargo clippy --offline --locked --all-targets -j 2 -- -D warnings` passed. Python script compilation and all 3 `test_mcp_schema.py` tests passed. [Dual-client archive acceptance](CLIENTS.en.md#rerun-dual-client-archive-acceptance)

## 2026-11-18 · Noninteractive Codex search and version readback

On Windows used main [`4160fd5`](https://github.com/TheLudlows/Origence/commit/4160fd5a7cbd7e21af54903a9471fcda15f5f08c), local AnySeek, and Codex CLI 0.170.0 on a synthetic expense case. Both runs used `gpt-6.2-sol`, the same prompt, read-only sandbox, and `approval_policy = "never"`. Each used a fresh workspace, separate reader key, and identical v1/v2 rules. The direct MCP control read v2 in both.

- The first run took 31.09 seconds. Codex discovered both tools, but both `context_search` calls failed due to client approval before reaching Origence.
- The second run took 34.62 seconds. With `approval_mode = "approve"` for `context_search` and `context_get`, search returned v2 and readback by matched asset ID and version returned v2 text. Asset ID, version, and 100 TEST units matched expectations.

This result covers one synthetic preflight case. Real P0 tasks and independent sign-off remain pending. [Client setup](CLIENTS.en.md#verify-read-only-retrieval-using-codex-noninteractively)

## 2026-11-17 · #88 standalone Skill candidate entry point (local)

See the [#88 verification record](https://github.com/TheLudlows/Origence/blob/a56246406a0688f646a45a85cc2954fb14a8d28a/docs/validation/issue-88-skill.md) for the standalone CLI on `293e2bf6af3b180d43c8050731047b552ac07519`, binding selected evidence to its revision, synthetic authorization and revocation regressions, and remaining acceptance gaps. This work was unpublished, preserved the historical evidence below, and did not stand in for five-way P0 integration acceptance.

## 2026-11-17: #90 Console facts and knowledge governance (local)

The baseline is `293e2bf6af3b180d43c8050731047b552ac07519`, and the independent branch is `feat/console-governance-p0`. The old local `be3474b` directory filtering and `e9dc434` task/409 feedback are selected. The current SQLite/AnySeek text storage is adapted, without moving the old schema, STATUS, or other functions. The Run UI, source verification interface, shared navigation/DTOs, CI, storage format, or permissions are not changed.

The memory catalog uses `memory_filter` and `Service::list_memories` to match persistent identity exactly by subject, attribute and complete condition set. It does not infer unknown identities. Each seek selects at most 100 candidates by scoped primary key and 1 lookahead. Within one read transaction it filters and rechecks ordinary asset sources and semantic visibility, then returns at most limit items. The candidate cursor can continue through empty pages, with no total count or unbounded scanning. The catalog omits bodies, and identity definitions and writes are unchanged. Manual SQL review found one table, `oc_assets`, bound to tenant/workspace/after. It adds no JOINs, subqueries, tables, indexes, migrations, native I/O within the transaction or intermediate commits. EXPLAIN regression confirmed SEARCH/INDEX without SCAN or TEMP B-TREE. Production-scale performance remains outside this evidence.

A 409 while editing knowledge explicitly preserves the draft and original expected_version, so the user can reread the current version for comparison. For failed, canceled or superseded jobs on known assets, the UI can reread current published metadata. Job failure does not indicate asset loss. Retry and cancel use existing APIs and permissions. The UI cannot bypass completed or revoked states.

### Automated results

Linux，Node 24.22.1 / Rust 1.98.0：

- `npm --prefix ui run check`, `npm --prefix ui test`: 19 items passed. The same pages fixture is reused, and existing assertions such as Run/PDF/association are retained. New filtering continued scrolling and task/409 sub-uses are added.
- `cargo test --locked --test local catalog::` passed 5 tests. Coverage includes catalog HTTP and Service, exact filtering, production/staging, unknown identities, revocation, rotation, cross-workspace isolation, limit 1 pagination, traversal through an empty page with 101 unpublished candidates, invalid filters and limits, and exact reverse lookup of existing Runs.
- `cargo test --locked --lib memory_identity::` and `--no-default-features`: 9 items passed each.
- `cargo test --locked --test local racing_expected_versions_supersede_the_loser`: 1 item passed.
- `cargo test --locked --no-default-features --features sqlite --test local catalog::`: 3 items for semantic_catalog passed; HTTP/native catalog tests were excluded by local-storage cfg, so not considered as double runs.
- `cargo build --locked`, `cargo fmt --all -- --check`, and `git diff --check` passed. The three groups: `cargo clippy --locked --all-targets -- -D warnings`, with `--no-default-features` and `--no-default-features --features sqlite`, also passed.

### Local real browser range

Used current main's pinned AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a` rebuilt with Rust 1.99.1, this branch's Origence and Node UI, and headless Chromium 156.0.8078.79. The current `tools/anyseek_smoke.py --anyseek-binary <pin binary> --run python3 <fixture>` wrapped three reused Playwright scripts. Each used fresh SQLite v2 and AnySeek directories, disposable test keys and synthetic documents. No API interception, paid models or production data were used. Original scripts failed on the memory body label, a response wait condition and an inapplicable assumption about highlighting ordinary chunks. Fixed the selectors and exact request waits, then reran against a real semantic fact with source_span. The backend API seeded attributes because the UI proxy does not expose the admin attributes route. The proxy allowlist stayed unchanged. Ordinary chunk offsets were not counted as source ranges, and the failed runs did not count as passing product evidence.

The memory workflow passed. Reader catalog used actual filtering to fetch 25+1 production entries and excluded withdrawn entries. Subject, predicate, full conditions, staging, unknown identity, empty conditions, and reset were checked. Refiltering after staging withdrawal returned empty. When editing production, identity fields were locked. Publishing v2 left staging under the same subject on its original text/v1. Registered a property with admin and made a real semantic write containing Chinese, emoji, and combining characters with nonzero source_span. The clicked verified mark matched the authoritative UTF-8 byte slice. Direct source reads by reader received 403. This check does not add highlighting to ordinary explicit facts. PDF page-local text remained pending separately.

The knowledge workflow passed. Corrupt PDF update and retry failed, while v1 remained readable and searchable by keyword. A stale form received real expected_version 409, retained its draft, and reread v2. Reader had no update button and writes and job access were denied. Writer could not withdraw sources. A completed job's retry/cancel returned 409, and a failed job could not retry after source withdrawal. Run was reverified without changes. A pinned v1 reference on 320px opened v1 when current was v2. Withdrawal cleared text, and clicking the old reference, refreshing a pinned URL, and reopening the Run did not reveal obsolete text. Other-key denial and real event provenance labels were checked. All 7 passed.

No new pushes, no new CI; real Windows, physical devices/assistive technologies, all semantic states, PDF page highlighting, complete #76 and external dual clients/enterprise tasks were not completed on this path. Screenshots, temporary data, and credentials were not submitted.

## 2026-11-17: #82 synchronized with #85 to resolve document conflicts (current).

From PR #82 HEAD `74de4655d5c251970be716ad873f8cfdb1ee0a3a` merged into main `92258fc578bbdb1c21ca949ddbbcfb1729a5813a` (also merged into #85). Only conflicts exist in STATUS and this document; the merge retains narrow-screen/PDF validation, automatic routing for #85 workspace, and historical evidence for each batch. Compared to the main branch, there are no changes to Rust, API, SQL, Cargo/Node dependencies, or CI processes; UI changes remain limited to CSS, source hints, and DOM regression.

This version of Linux / Node 24.19.0 and Rust 1.98.0:

- `npm --prefix ui run check`, `npm --prefix ui test` passed **17 items**; uses existing dependencies consistent with lock files.
- `cargo fmt --all -- --check`, the default feature `cargo clippy --locked --all-targets -- -D warnings` and `git diff --check` passed. No rerunning of the entire Rust test suite was required; the 435 results from the main branch were retained as originally submitted.
- Real localhost Node UI → Origence → AnySeek **passed**: identity, upload/publish, directory, history, retrieval, context, private Run isolation, and source withdrawal. The backend uses the same Rust source code as the main branch, producing binary files; uses AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`, independent fresh data directory, automatic workspace collection, and one-time service credentials. No real model is called, nor is the API intercepted or simulated.
- Headless Chromium **154.0.8037.57**. Five pages × 1440/1024/736/375/320 × light/dark. **50 layout combinations** had no whole-page horizontal overflow and visible sign-out. **10 Run detail layouts** also passed. In 320px touch simulation, theme toggle, trace navigation, visible controls ≥44px tall, and return to login after sign-out passed.
- The 375px layout called the real PDF source preview. Page 2 prompt, notice that page text was unverifiable, no generated highlight, and authorized download all passed. Download SHA-256 matched the original uploaded two-page PDF bytes. This run did not add actual page-text display or highlighting or repeat all historical keyboard and withdrawal checks.

Browser scripts and one-time data are only stored in temporary directories and do not submit tokens/screenshots or test results. Real Windows, physical touchscreens, and screen readers were not tested; this version does not claim to be a complete acceptance of #76. The remote CI will consider this merge as the final submission.

## 2026-11-17: #82 synchronized with the latest main branch, with limited retesting.

The original PR HEAD `6978745ff33fb16dce472f403818ca1db3fa29d3`, merged with the main branch `739be8cc8cf62ba80d8ea598504327e1fd63e24e`. Resolved conflicts between STATUS, this document, and pages fixture: retained the main SQLite, fixed version association with Run, Windows launcher, and AnySeek text storage; merged two fixture branches and assertions for PDF source and asset association. No changes to Rust/API/SQL/CI compared to the main branch. The classification of gaps in the historical evidence below applies only to the baseline sections; the associated Run has been addressed in #83.

- Node 24.22.1: `npm --prefix ui run check`, `npm --prefix ui test` passed with 17 items. The first installation of npm failed due to the non-writable default home cache; `npm ci` succeeded after using a temporary cache.
- Rust 1.98.0: `cargo build --locked`, `cargo fmt --all -- --check`, and the default / `--no-default-features` / `--no-default-features --features sqlite` groups `cargo clippy --locked --all-targets -- -D warnings` passed. `git diff --check` also passed.
- `cargo test --locked --test local catalog::` passed with 5 items (including semantic_catalog); `cargo test --locked --test local content_storage::` passed with 4 items; covered precise version associations, key isolation/withdrawal, reopening, text corruption, and permission changes during reading. No rerunning of the entire Rust suite was performed.
- Rebuilt Origence and the Node UI after the merge, and built actual AnySeek from exact pin `d813b915b7f12e7139ca316adc15699688493196`. Used separate fresh SQLite v2 directories and collections, synthetic data, and one-time keys, without external models. The existing Playwright script used `tools/anyseek_smoke.py --anyseek-binary … --run python3 …` for separate layout, revocation and PDF scenarios. HTTP calls were not intercepted and responses were not simulated.
- Headless Chromium 156.0.8078.79. Five pages × 1440/1024/736/375/320 × light/dark yielded 50 combinations with no whole-page horizontal overflow and visible sign-out. Simulated small-screen touch buttons ≥44px and tapping navigation passed. Desktop keyboard navigation and 10 Run failure-event keyboard selection/focus combinations passed.
- At 320px, pinned v1 with current v2, withdrawal, old Run click, pinned URL refresh and sign-in, refusing old text on reopen, other-key denial, and event provenance labels all passed, 7 checks. Real two-page PDF at 375px passed page-two location, notice of unverifiable page text, no false highlight, and authorized download SHA-256 matching original bytes.

Scripts/screenshots/temporary keys remained local and were not submitted; no real Windows, physical touchscreen, or screen reader tests were conducted. This section does not claim to be a complete acceptance of #76. The remote CI is based on the actual runs submitted with this merge, and cannot use the results from the old HEAD.

## 2026-11-17: #85 synchronized the main branch with the merged conflicts (current).

The main `739be8c` (including #84 Windows initialization and #86 AnySeek text storage) was merged into #85. The `origence-content-sha256-v1` payload schema is retained, along with immutable historical content/Source verification and automatic workspace routing. A new text reading interface is added to pass the authenticated Scope with each query. Routing returns different texts saved with the same chunk/source ID in three tenant/workspaces, and the respective return values are checked. The backend binding is changed to `origence-content-backend-v2`, with the text format and automatic routing being fixed. Old bindings are rejected, not converted, or overwritten.

The Windows launcher no longer requires or forwards `OC_ANYSEEK_COLLECTION`, instead using `/api/v1/live` to verify the version with service credentials; Origence continues to check published collections. The initialization documentation removes old tenant registration/Key issuance/manual collection requests. Test assertions correctly detect paths, service credentials, process environment isolation, exit cleanup, and failure propagation; no port, directory, output suppression, or process ownership constraints are relaxed.

Linux / Rust 1.98.0; AnySeek is built normally with a fixed `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a` (Rust 1.99.1); no real user data or model services are used.

| Check | Result |
| --- | --- |
| Default full regression | PASS: 222 lib + 5 protocols + 14 cores + 194 local = 435 items; 8 real contracts executed separately |
| Core library / Independent SQLite authority | PASS: 87 / 233 items; 5 real contracts ignored and not counted as passes |
| Clippy | PASS: default, no-default, no-default + sqlite, three groups for all targets, all with `-D warnings` |
| Format/patch | PASS: `cargo fmt --check`, `git diff --check` |
| Real AnySeek | PASS: 8 contracts, HTTP/stdio MCP, historical version/Run replay, and text/index consistency after AnySeek process restarts |
| Real HTTP / Node UI | PASS: Two sets of smoke tests, including upload, publishing, retrieval, context, history, private Run, and source revocation |
| Linux PowerShell 7.4.13 | PASS: 7 simulated sub-service/process contract regressions; officially released SHA-256 verification followed by execution |

The conflict resolution added no SQL or dependencies. Main's single-table transactions, asynchronous I/O boundaries and authorization and source rechecks were retained. This entry is new; evidence below keeps each batch's original scope. PowerShell on Linux does not establish real Windows console signal handling or native MSVC acceptance. Remote Windows/Linux CI applies to the merge commit recorded here.

## 2026-11-17: Workspace automatic mapping (synchronizing before main)

`AnySeekConfig::new(base, service_key)` takes only connection details and the service credential. Tenant and collection settings were removed. The authenticated Scope's tenant UUID is the namespace and the workspace maps to `ws_<无连字符 UUID>` (UUID without hyphens). Service startup uses no placeholder collection. First use creates the fixed schema. Startup strictly validates published collections, and a missing published collection is never recreated. The v6 binding stores endpoint, routing rules, model and the analysis digest persisted on first use. It does not store credentials.

Linux / Rust 1.98.0; the actual engine is AnySeek [`6a46fc7`](https://github.com/TheLudlows/anyseek/commit/6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a) with normal production builds (Rust 1.99.1, no fault-injection). The CI fixture is the same for all commits.

| Check | Result |
| --- | --- |
| `cargo test --locked --no-fail-fast` | PASS: 431 items, 8 additional items with actual engine contracts ignored and executed separately |
| `cargo test --locked --no-default-features` | PASS: 87 items, 5 additional items with actual engine contracts ignored, not counted as passes |
| Default with no default Clippy features | PASS: `cargo clippy --locked [--no-default-features] --all-targets -- -D warnings` |
| Formatting and patches | PASS: `cargo fmt --check`, `git diff --check` |
| Actual AnySeek contracts | PASS: 8 items; full text/vectors/graphs, publication retry, response loss recovery, and reopening of public Services |
| HTTP / stdio MCP | PASS: Search, reading, referencing, updating, history, user key revocation, Run diagnostics, and process restart |
| `tools/smoke.py` tested with actual AnySeek | PASS: Public HTTP, identity scope, publication, retrieval, and reopening |
| Node UI → Origence → True AnySeek | PASS: Upload, publish, directory, history, retrieval, context, private isolation, and source revocation |

Protocol regressions cover tenant UUIDs starting with digits, different workspaces within one tenant, reuse of a workspace UUID across tenants, concurrent initialization, strict readback after a lost creation response, refusal to recreate a missing published collection, analysis digest drift, and credentials kept off disk. Fixtures scope data by namespace and collection. Real acceptance no longer precreates collections. AnySeek returns 403 for an unknown scoped ID. The client only creates a collection after confirming its absence in the namespace list with service credentials. Other permission errors do not establish absence.

Enumeration of published workspaces reuses bounded single-table reads and same-transaction source checks. No SQL statement, JOIN or table was added. Remote collections are accessed once the transaction ends. An earlier offline CLI fixture injected projections directly and bypassed initial binding. It now initializes using the public publication path. Full regression and live acceptance were both rerun after correction. This round verifies isolated fresh stores, same-version restart and recovery. It excludes native Windows builds, real cloud S3 and live model quality. Earlier verification records remain below, and this section's automatic routing supersedes their explicit namespace and collection settings.

## 2026-11-17: AnySeek service credentials and explicit tenant namespaces

Baseline main `c70a1aa`. `AnySeekConfig::new` explicitly takes the service credential, namespace, and collection. The environment adds required `OC_ANYSEEK_TENANT`. Requests send `apikey` and `X-AnySeek-Tenant`. Namespace and collection are validated as lowercase AnySeek path components. The service credential remains sensitive and is never persisted. The v5 backend binding pins the namespace. A namespace change is rejected before remote I/O, preserving the old binding. Rotating credentials for the same backend does not change the binding. Origence business UUID scopes, logical user keys, private sessions and authorization rechecks are unchanged.

Linux / Rust 1.98.0, compatible with AnySeek Catalog 7, built for production (Rust 1.99.1, no fault-injection). Final commit is [`800e3f1`](https://github.com/TheLudlows/anyseek/commit/800e3f1c2862c72d7de6afecde1ab921cef24671). All real-engine fixtures use localhost, independent temporary data root/namespace/collection, and one-time service credentials. No real models are called, and no external S3 or user services are accessed.

| Check | Result |
| --- | --- |
| `cargo test --locked --no-fail-fast` | PASS: 431 items; 8 real-engine contracts were explicitly ignored and executed separately |
| `cargo test --locked --no-default-features` | PASS: 88 items; 5 real-engine contracts were ignored, not counted as passes |
| Real AnySeek contracts | PASS: 8 items, including full text/vectors/graphs, missing responses, publication verification, recovery, and public Services |
| Real HTTP / stdio MCP | PASS: Search, read, reference, update, history, Origence user key revocation, reopening, and Run diagnostic replay |
| `tools/smoke.py` passed by real AnySeek | PASS: Public HTTP paths and process reopening |
| Node UI → Origence → Real AnySeek | PASS: Identity, upload, publish, directory, history, search, context, private Run isolation, and source revocation |
| Default and no-default Clippy features | PASS: `cargo clippy --locked [--no-default-features] --all-targets -- -D warnings` |
| `cargo fmt --check`, all-targets check, normal service build, `git diff --check` | PASS |
| Node 24 UI | PASS: `npm --prefix ui run check`, `npm --prefix ui test` (16 items) |

The protocol fixtures also enforce service credentials and namespaces to prevent mock scripts from ignoring new request headers. New regression verification namespaces were added, backend binding isolation was maintained, binding files remained unchanged, and service credentials were not included in bindings. No new SQL or query execution entries were added; changes were only made in HTTP adaptation, configuration, existing bindings, and acceptance fixtures. The original JOIN-free constraints were retained.

The first follow-up library test build exhausted workspace disk space through the compile cache. Kept the failure record, cleared only the regenerable Cargo incremental cache and set `CARGO_INCREMENTAL=0`. All checks above passed on rerun. Data directories were not rewritten. Windows, live external Agents or models, production capacity, cross-host behavior and power-loss acceptance were not run. Node UI HTTP smoke only checks that path. Historical browser evidence applies to its own commits.

## 2026-11-17: #86 was reviewed and useless metadata and word segmentation backtracking were removed.

Continuing from PR #86's `70c7281`, the main `8fe01a5` (#84 Windows development launcher) was merged. After manually verifying the entire call chain, `oc_chunks.search_terms` was removed: candidate reads only consumed IDs, the old implementation only passed terms before discarding them; `oc_summaries.model_revision` was removed: there was no SQL reader, and the model source was saved in the AnySeek payload. Corresponding write parameters, SELECT/decoding, candidate tuple fields, and directories that did not have content fields were also removed.

All release types were changed to freeze the main text/document terms once outside the transaction. Emptyable chunk terms were removed, and re-segmentation was performed during restoration. Document segmentation no longer occurred within the release transaction. Document recall terms, original sources, locators, owners/index/ledgers, and complete restoration plans still had actual consumers, maintaining the original validation. Existing historical validation records were not rewritten; the schema was still developed within the fresh-only scope of this PR, without automatically converting to the old layout.

Local Linux / Rust 1.98.0: default **433 passed** (221 lib + 4 protocols + 14 cores + 194 local); pure-core **87 passed**; independent sqlite authority **233 passed**. All three groups of Clippy passed with warnings denied, and fmt/diff checks were passed. Added retry rejection for damaged freeze terms, proving that re-segmentation or release would not occur. Column missing assertions covered the newly removed items. Existing sources, history, cross-scopes, cancellations, retries, and private Run/Skill tests were passed.

**8 real AnySeek d813b91 contracts passed.** Actual HTTP and stdio MCP, historical reads, Run diagnostics, reopen, and current/historical text and index checks after restarting AnySeek passed. Use the exact current PR #86 run for Windows/Linux CI. This run adds no production performance, model quality, or browser-layout acceptance.

## 2026-11-17: Main text and index share the AnySeek collection

The baseline is `c70a1aa`, the branch is `refactor/anyseek-content`, Rust version is 1.98.0, operating on Linux. AnySeek is fixed to `d813b915b7f12e7139ca316adc15699688493196` (with a Rust 1.99.1 fixture binary already available). This section records the local working tree of this branch. Future Windows/Linux commits will be based on the CI status, subject to changes in the PR. Only synthetic data, temporary directories/collections, and one-time credentials are used; no migration or rewriting of existing user data is performed.

- `cargo fmt --check`, `git diff --check`, and three groups of `cargo clippy --locked --all-targets -- -D warnings` (the latter two with corresponding feature parameters) passed.
- `cargo test --locked --no-fail-fast`: 221 lib + 4 AnySeek protocol + 14 core + 193 local = **432 passed**; 8 real engine contracts are ignored by default and will be tested separately in the next step. No doctest used.
- `cargo test --locked --no-default-features`: 71 lib + 2 protocol + 14 core = **87 passed**. `cargo test --locked --no-default-features --features sqlite`: 138 lib + 2 protocol + 14 core
  + 79 local = **233 passed**; The pure core does not introduce SQLite, and no new dependencies are added to Cargo.toml/Cargo.lock.
- `tools/anyseek_smoke.py --cargo-tests --binary target/debug/origence` passed **8 live AnySeek contract tests**. New assertions check storage and retrieval of original UTF-8 and CRLF, digest and model version, and locator with the payload. Normal HTTP/stdio MCP query, version updates, historical reads, key revocation, Run diagnostics and reopen passed.
- The `--binary` command added assertions for normal shutdown/reboot of the real AnySeek process, followed by restarting Origence to read the current/historical text and retrieve data. **Passed**. No claim to consistency across storage or online snapshots in case of power failure.
- The actual AnySeek instance wrapped `tools/smoke.py` and `tools/ui_smoke.py`. HTTP and Node UI → Origence → AnySeek passed end to end, covering upload, search, Context, version history, private Runs and source revocation. `ui/npm run build` and `ui/npm test` passed (**16 passed** for tests). Browser layout and accessibility acceptance was not added.

The new `content_storage` regression confirms body and index share one Record. Three old body or summary columns were removed. Pinned past versions and restored data retain original bytes after Origence reopens. Missing or tampered payloads fail explicitly. GET, Search and Context deny output when the key or source is revoked during the read. Existing PDF, semantic collections, Learning, Skill, private Run, idempotency, lost-response recovery, cross-scope, SQL transaction and cancellation regressions continue to pass.

Manually reviewed each new or changed executed SQL statement, including fixtures, DDL and expanded formats. Statements read a single table or use existing relation adapters, with no JOINs, correlated subqueries, remote I/O in a transaction or intermediate commits. SQL retains original sources, locators, frozen recall terms, full recovery plans and private snapshots. Only published body and summary text moved. Formats `origence-sqlite-v2` and `origence-content-sha256-v1` target fresh data. Older formats are rejected without automatic conversion. These regressions do not establish production relevance, capacity, latency, model quality or online backup recovery.

## 2026-11-17: #76 precise version associated with Run (independent local slicing)

Baseline main `593e8fa71d7c354587cffdd1aae7c4b2e2d2a554`; independent branch, excluding changes for the first batch `fix/issue76-browser-coverage`. Added a read-only `GET /v1/assets/{id}/runs?version=N`, restricting to the current logic key, exact version, and 1–100 candidates, with a UUID cursor. The new table is only the first recorded reference projection; the reference event remains authoritative: client reporting and server preparation for updates within the original transaction, same as snapshot reading assets, sources, owners, and re-verifying the exact reference in the event; no scanning of all Runs, no return of private content. Non-readable proof is conservatively omitted, not claimed to be fully associated/collected; specific boundaries are defined in the Run contract. All newly added SQL statements have been reviewed individually, all are single-table operations, with no JOINs.

Real localhost Node UI → Origence → AnySeek, fresh synthetic v4 data, disposable key and no model. AnySeek pin `d813b915b7f12e7139ca316adc15699688493196` and headless Chromium 156.0.8078.79 at 1440×1000. Playwright used actual clicks without response interception for Run → pinned v1 → recorded association → original Run. Current v2 had no association. Another admin in the same workspace saw no association and was denied direct Run reads. Rereading after source revocation cleared text and stale links. Clicking old citations, refreshing sign-in or reopening the Run still denied old evidence. All 10 real browser assertions passed. This slice did not rerun five-page responsive acceptance or use live external clients.

Local Node 24.22.1 `npm --prefix ui run check`, `npm --prefix ui test` PASS (build/16 items). The existing DOM fixture only adds sub-tests for exact version association. Rust 1.98.0:
- `cargo test --test local catalog::` passed 5 tests, including 3 existing semantic_catalog tests. New cases cover v1/v2, isolation from another admin in the same workspace, key rotation, idempotent deduplication, cursor and limit, cross-workspace behavior, reopen and source revocation. The exact test passed again after adding an empty-page cursor assertion for missing authoritative links.
- `cargo test --test local workspace_schema_rejection::` PASS (5 items); `cargo test --no-default-features --test local workspace_schema_rejection::` PASS (5 items). Rejection occurs when new tables are missing, while the database byte remains unchanged.
- `cargo test --test local retrieval_diagnostics::run_diagnostics_are_private_durable_and_replay_original_evidence` PASS (1 item).
- `cargo fmt --check`, default/no-default `cargo clippy --all-targets -- -D warnings`, `git diff --check` PASS. Not run on Windows; no new CI (not pushed). No pushing, no creating PRs, no closing #76; Production metrics, alerts, PDF text within pages, and complete external acceptance are still lacking.

## 2026-11-17: #76 first batch of responsive and PDF source acceptance (local checkpoint)

Baseline main `593e8fa71d7c354587cffdd1aae7c4b2e2d2a554` includes #80. Fixed real browser panel-grid minimum content widths. The Run catalog at 320px had 350px, and the knowledge catalog at 320/375px had 430px. The tables now scroll within their containers. Small screens retain sign-out. Coarse-pointer and mixed-input links and theme buttons reach 44px. PDF provenance distinguishes page/page-local bytes, missing verified page text, and download. It does not display empty source event text as a highlight. No API, permission, or storage changes.

Local Node 24.22.1 `npm --prefix ui run check`, `npm --prefix ui test` PASS (build and 16 items). Only DOM sub-case verification of PDF page numbers, missing text descriptions, download entries, and no error highlighting generated. Rust 1.98.0 `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo clippy --all-targets --no-default-features -- -D warnings`, `git diff --check` PASS.

Actual localhost Node UI → Origence → AnySeek, fresh v4 synthetic data, disposable key, models disabled, engine pinned `d813b915b7f12e7139ca316adc15699688493196`. Playwright drove headless Chromium 156.0.8078.79 without intercepting the API. 50 layout combinations of five pages, 1440/1024/736/375/320 × 1000, light/dark. document.scrollWidth equaled viewport and sign-out was visible. 736/375/320 used touch and actual taps. CDP reapplied touch for each page, asserted any-pointer:coarse, and measured visible buttons ≥44px. The original script reset input simulation, so its mouse state cannot count as touch coverage. Desktop Enter navigation worked. All 10 Run layouts allowed selecting real preparation failures by keyboard with outline=solid and server-observed/not-prepared. This is not full physical-touch, screen-reader, or all-control keyboard-order acceptance.

A separate fresh v4 flow uploaded a real-xref synthetic two-page PDF and waited for actual ingestion. At 375px, keyword search reached page two and clicking verified locator page=2, parser=pdf-oxide-0.3.78-v1, page text unavailable, and no mark. Authorized download SHA-256 matched uploaded bytes. Screenshots were retained locally and are not in the repository. The source event API does not return extracted page text, so page-local highlights were still unimplemented. Do not apply page-local offsets to the concatenated asset body.

After checkpoint `78d39f57017860c0ceccb53b38b4f72cfe958def`, real 320×1000 browser testing was run: The Run reference still opens the fixed v1 when the current asset is v2; the source is cleared immediately; previous opened Run is clicked again, the fixed v1 URL is refreshed, logging in again, and the original Run is reopened, all rejecting the old content; another logical key reading of the Run was rejected. Both real failure labels and client-reported labels were passed. All 7 assertions passed.

### #76 no evidence classification for selected items

| Item | Current Status and Boundaries |
| --- | --- |
| Object/List/Aggregate/Alert Contract | Partially: Asset directory and current logical key Run directory exist; Aggregates, alerts, and reverse check contracts missing |
| Memory identity and status queries | Partially: Existing directory/retrieval/explicit identity entry and semantic status guards; Complete filtering and real status matrix not accepted |
| Knowledge Upload/Retained from Old Versions/Source/Restoration | Part: Current genuine PDF uploads, task releases, source downloads verified; all formats and updates failed/Restoration matrix not rerun |
| expected_version/409/Duplicate/Task/Role | Part: Existing DOM overrides for retry identities, server-side contracts exist; full role genuine browser concurrency/Task matrix not accepted |
| Pinned version / PDF / UTF-8 | Partial: the preceding section has actual pinned v1 and revocation evidence. This batch verified PDF page two and original download. UTF-8 Chinese/Japanese/Korean and emoji boundary unit tests passed. An API to verify in-page PDF text is still missing |
| Genuine Budget/Token/Costs | Part: Actual UTF-8 byte budget displayed; no credible Token/pricing contracts, costs not calculated |
| Key/Role/Withdrawal/Expired References | Part: Another genuine key refused in previous section, fixed reference withdrawals verified; all roles/expired combinations not completed |
| Keyboard/Touch Screen/Theme/Size | Part: 50 layout for this batch, touch screen buttons/navigation and 10 Run keyboard combination passed; complete focus order, accessibility technologies, physical devices not accepted |
| Frozen Genuine Client/Enterprise Tasks | External acceptance: No genuine Claude Code/Codex client or enterprise tasks run |

Source code verification for the next batch of backend contracts: A single preparation diagnosis can record up to 128 query observations with omitted numbers. It cannot achieve a complete Run success rate/P95. Contracts without a trusted lifecycle, Token, price, aggregation window, or alarm status are not included. There is no reverse interface/index from assets to Runs, making it impossible to enumerate across keys or fill in from front-end caches. Boundaries must be established based on owner keys, with precise versions and rechecks against the same snapshot source. Returns must cover/span the cursor. PDF pages with verifiable text must be read in an immutable version, and the continuous UTF-8 range, parser, and source permissions of the chunks within the page must be verified. The current parser cannot reinterpret old offsets. These contracts have not been implemented in this batch, and #76 is still unfinished.

## 2026-11-19 — #76 Narrow-screen touch target areas and interactions (Chromium simulation)

Based on the main `2ad69260522676ecf70f9cd86fc3e3e47d453c8a`. In Chromium's coarse pointer touch screen simulation, the "exit" button under 320 / 375 / 736 CSS pixels was originally 38×44 pixels, with the context source expansion control being only about 24 pixels high. The fix only applies under coarse pointers: the button is at least 44×44 pixels, and the native source expansion control has a minimum height of 44 pixels. The original expansion markers are retained. The private session checkbox is small, but the 47 pixels of the surrounding label can be tapped, without any changes being made.

Linux, Node 24.22.3, Playwright 1.57.0 / Chromium 153.0.8010.0. The UI after building is synthesized as HTTP through real Node relay docking loopback. All three width (light / dark / light color) original size assertions failed, but they have been fixed. Verification with browser taps confirmed that the focus is returned after closing the new knowledge dialog box, and reopening does not retain a draft state. The private session line edge can be switched repeatedly, and the session is accurately carried when submitting. The source can expand/collapse from the target edge, and the fixed v1 link and routing focus are correct. Finally, clicking to exit clears the private interface. No full-page lateral overflow was found on the page. The existing navigation keyboard/focus and context knowledge have returned to Chromium, UI type checking, and 21 Node tests have passed.

```sh
npm --prefix ui run build
NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
  node ui/scripts/check-narrow-touch-browser.cjs
```

This is not a physical touch screen, real screen reader, or complete #76 acceptance criteria. No real client, model, pilot, or deployment has been run.

## 2026-11-17: Run event source labels and real browser orientation acceptance (local)

The baseline main `01d4f82b20c986278b077e8660f9bbc0538e8d6c`. Only UI event labels and details were modified: `server_prepared` indicates successful server preparation, clearly not proving receipt/use; `server_observed`'s `context_preparation_failed` indicates that the server observed preparation failure/non-preparation context; `reported/client` indicates client reporting, while other or unknown sources are not presumed to be client. The API, permissions, and backend semantics remain unchanged. The existing pages fixture failed to expand/be client-side/unknown source. The timeline and details were verified, and no additional duplicate test suites were added.

Local Node 24.22.1: `cd ui && npm run check && npm test` PASS (Build and 15 items). Origence Rust 1.98.0 `cargo build --locked`, `cargo fmt --package origence -- --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `git diff --check` PASS.

Real browser: Playwright driver system Chromium **156.0.8078.79**, headless, 1440×1000, directly accessing the real Node UI on localhost → Origence → AnySeek. AnySeek fixes the current main pin `d813b915b7f12e7139ca316adc15699688493196`; fresh v4 temporary directory, one-time key, no external model. The API only preloads assets/Run/client events: first release v1 and prepare Run, then release the same asset v2; use a disabled model's vector to prepare requests to generate real 503 and server failure events. No browser responses are intercepted or forged.

| Browser Action | Observation Results |
| --- | --- |
| Open Run, click successfully, failed, and client events one by one | Source badge is correct; failed details include server/not_prepared, successful ones clearly indicate no confirmation of receipt or use |
| Open the Run with another key | Read refusal, original text not appeared |
| The current asset is v2, click on v1 reference in Run | URL contains `?version=1`; title/body is v1, no v2 independent markers present |
| Click "Recover source" on v1 details and confirm | Displays "Content blocked", old text cleared |
| Click on the old v1 reference in the pre-opened Run tab after recovery | Displays "Temporarily unreadable" and resource not found, no old text returned |
| Refresh the fixed v1 address and log in again; open the original Run again | Read refusal, no old text displayed anymore |

Review can be repeated in a new workspace: first prepare v1's Run, update to v2, then click on references in the browser and recover sources. Check the opened tabs, the page after refreshing, and access with another key. Screenshots and local driver scripts are not included in PR; test credentials are not saved, and services are shut down and temporary data deleted at the end.

These are real browser results for a single specified path, not a pure HTTP or happy-dom alternative acceptance; they do not cover mobile, full site accessibility, all browsers, real client collection, metering/alarm, or reverse correlation. This does not indicate that issue #76 is fully completed; this section only records local verification and does not represent CI, merging, or deployment.

## 2026-11-17: #79 merged with the latest main

Merged main `3d474d9b4efe2edb0cf0c6a4aca9c0d1c3cba3fe` into #79 at `04eb4b6774e5c9f6a8346b05b95d10754d58557a` without rewriting the branch. Text conflicts affected only STATUS and VALIDATION. Current UI, diagnostics and Run content comes from main, with historical checks retained for each original commit. After the automatic code merge, reviewed Search, Context and publication_tests. Main's diagnostic capture, route tags, bundle observations and new fixture metadata were retained, and Turso still batch-rechecks all candidates participating in native ranking. Run/API, AnySeek, UI and CI match main. Removed this branch's redundant fixture feature gate because main already has one. SQL, error priority and Run replay, failure and revocation contracts were unchanged. Earlier CI and unit tests were not restored.

The real backend for this round is set to the current CI's `d813b915b7f12e7139ca316adc15699688493196`, built in an independent clean worktree; it no longer represents the current agreement with the earlier `6614bbf` results. Local Origence Rust is 1.98.0, AnySeek Rust is 1.99.1, and Node is 24.22.1; temporary data and credentials are used.

| Merged Check | Results |
| --- | --- |
| `cargo test --locked --lib` | PASS: 216 items; including offline rank, final review/stack constraints for publication, diagnostic isolation, and storage of Run contracts |
| `cargo test --locked --test local` | PASS: 202 items; including EvidenceBundle, production storage, diagnostic configuration, successful/failed/replay/withdrawal and reopening of Run contracts |
| `npm ci --ignore-scripts --cache /tmp/pr79-npm-cache`, `npm test` | PASS: UI build and 15 items; the initial default cache directory is not writable, but it was resolved after switching to a temporary cache |
| Python tools / evals | PASS: 11 / 42 items |
| `cargo test --locked --no-default-features --lib` | PASS: 136 items |
| Default/no-default `cargo clippy --locked --all-targets -- -D warnings` | PASS; No default command with `--no-default-features` |
| `cargo fmt --package origence -- --check`、`git diff --check` | PASS |
| `cargo build --locked` | PASS |
| Latest stable version of AnySeek: `anyseek_smoke.py --cargo-tests --binary … --run python3 tools/mcp_smoke.py --binary …` | PASS: 8 real engine contracts, complete MCP forwarding/offline, HTTP/stdio, Run diagnostic correlation/retry/restart |
| Latest stable version of AnySeek: `anyseek_smoke.py --run python3 tools/ui_smoke.py --binary … --ui-entry ui/dist/server.js` | PASS: UI to backend storage/directory/history/retrieval/context, Run isolation and source retraction |

This round did not rerun the no-default complete integration suite, Windows, real browser layout, production performance, or real model/client tasks. The CI before merging or local passes were not written as remote CI passes for the new HEAD.

## 2026-11-17: #79 CI bug fixes and audit

Failed CI run [44036124032](https://github.com/TheLudlows/Origence/actions/runs/44036124032) corresponds to #79 `5ce7f4c`. After reading the actual job logs, the exact failing MCP call chain reproduced locally. `protocols → check_tools → schema_object` accessed missing `properties` on the empty-argument schema at line 299 and raised `KeyError`. The earlier passing `anyseek_smoke.py --binary` is a narrower contract check than CI's full `--run python3 tools/mcp_smoke.py` check. The actual fast-check failure was `graph_evidence_for_publication` marked dead_code without default features.

Fix commit `b6b0bf0af39f67d8cf5ed3fe49058fa90e0471c8` compiles the graph method only with `local-storage`, its sole caller's feature. The same feature gates the Service fixture. An empty schema defaults missing properties to an empty set, keeping field-set, required, type and additionalProperties checks. Moved the fixture gate from the declaration in `tests/local.rs` to file scope in `tests/support/mod.rs`, keeping the feature behavior to avoid a text conflict where main added catalog tests at the module entry point. After the move, reran all-targets Clippy in both feature configurations and the full no-default suite. No global lint allowance or failing assertion was removed. Main had advanced to `44e06df` (#77 merged) when this round began. These three minimal fixes match main, without merging its UI or #78 diagnostics into this branch.

Local results for Linux / Rust 1.98.0 (only documentation will be supplemented later):

| Check | Result |
| --- | --- |
| `cargo clippy --locked --no-default-features --all-targets -j 2 -- -D warnings` | PASS |
| `cargo clippy --locked --all-targets -j 2 -- -D warnings` | PASS |
| `cargo test --locked --no-default-features -j 2 --no-fail-fast` | PASS: 135 lib + 2 protocols + 92 local; 5 real backend cases remain ignored |
| `cargo test --locked --lib retrieval` | PASS: 14 items, including rank/publication/identity/return contract |
| `cargo test --locked --test local graph` | PASS: 4 items; graph publication recovery, one-hop/withdraw, precise identity of evidence, workspace isolation |
| `cargo build --locked -j 2` | PASS; new products are used for the following MCP verification |
| Fixed real AnySeek with `tools/anyseek_smoke.py --anyseek-binary /workspace/anyseek-fixture/target/debug/anyseek_server --run python3 tools/mcp_smoke.py --binary /workspace/Origence/target/debug/origence` | PASS: The same complete entry as the failed CI; both host-forwarding and offline paths passed |
| `python3 -m unittest discover -s tools -p 'test_*.py' -v` | PASS: 8 items |
| The existing `tools/test_mcp_schema.py` in main `44e06df` | Retrieved and run in the temporary directory, 3 PASS; empty objects, missing non-empty fields, unknown field rejection, no new duplicate UT added |
| `cargo fmt --package origence -- --check`、`git diff --check` | PASS |

CI/UT Audit: **0** items were deleted this time, and the workflow remains unchanged.

- The old linux-native no-default `memory_identity` subset and the complete no-default suite of fast-check use the same Rust/Ubuntu, features, push/PR trigger scope; the new main has removed this duplicate step, so no cleanup is needed in #79.
- The complete default/no-default suite, MCP complete offline/forwarding checks, HTTP smoke, and fixed real AnySeek ignored contracts are retained: their features, entry points, and fixtures are not equivalent and cannot be substituted for each other.
- The publication/EvidenceBundle/Source withdrawal/Isolation cases are retained; checks for old library rejections, old key conflicts, and legacy graph.db are still applicable to current read-only rejections and data retention contracts. The "legacy" in the name does not indicate test failure.
- The syscall fault UT is retained: `.lance` is just a synthetic file name, and the test still checks the sentinel/path limitations, counting, writev, pause/termination behavior of the C injector. It cannot be judged as deprecated based on the extension.
- The `manual-diagnostics` layout is diagnosed as an explicit feature + ignored. Regular CI does not execute it; its further retention or removal does not affect this access control and does not expand into historical tool cleanup.

The full default integration, Windows, or real model/production performance is not rerun locally; the new remote CI needs to be evaluated independently.

## 2026-11-17: Independent retrieval sorting and authoritative review are split.

Baseline main `dcde761ba11b34d88ce5da5f9025a22076571dfa`; implementation and local checkpoint `552989e3dc251a97dd75174348342d36084e80e8`. The following is the Linux / Rust 1.98.0 results of this code tree (submissions in the future will only supplement this section and STATUS). The independent branch does not include #77, #78.

| Command / Check | Result |
| --- | --- |
| `cargo test --locked --offline --lib retrieval::rank` | PASS: 2 items; no AnySeek, model, or database IO, covering split deduplication/upper limits, score/UUID parallel order, original score and RRF60 |
| `cargo test --locked --lib retrieval::publication_tests` | PASS: There are 6 items; delayed retraction, pending/orphan/precise publication identities, text changes, return stoppoints/hidden candidates, semantic errors, and public future stack size |
| `cargo test --locked --test local evidence_bundle` | PASS: 13 items (including Learning bundle regression); independent routing top-k, historical/retraction/permissions/budget/conflicts followed by review |
| `cargo test --locked --test local production_storage` | PASS: 8 items; replaying a single vote, conflict generation, error source candidates, projection failure/loss, and retraction |
| `cargo clippy --locked --all-targets -- -D warnings` | PASS |
| `cargo test --locked --no-default-features --lib` | PASS: 135 items; there are dead_code warnings for existing graph-based methods |
| `cargo fmt --package origence -- --check`、`git diff --check` | PASS |

No default features, strict Clippy **FAIL**: `cargo clippy --locked --no-default-features --all-targets -- -D warnings` and `cargo clippy --locked --no-default-features --lib -- -D warnings` both result in `graph_evidence_for_publication` dead_code at `src/storage/sqlite/app.rs:288`. The same error was reproduced after re-running the command in the unmodified main `dcde761` working tree; this method has not been modified in this branch. The complete integration suite without default features was not run, so the library tests were not considered a complete pass.

The actual AnySeek checkout is clean and fixed to `6614bbf29bad0041c65738831bc1a16da4c69660`. The existing `tools/anyseek_smoke.py` is used to create independent temporary directories, collections, and test credentials

```sh
python3 tools/anyseek_smoke.py \
  --anyseek-binary /workspace/anyseek-fixture/target/debug/anyseek_server \
  --cargo-tests --binary /workspace/Origence/target/debug/origence
```

PASS: 8 items of actual engine contracts, as well as retrieval, reading, referencing, updating, history, revocation, and reopening of AnySeek → Origence HTTP/stdio MCP. The Origence built using this code is used; no modifications or relaxations of fixtures were made. The targeted integration tests for EvidenceBundle/production_storage still use the repository's protocol fixtures, which are not considered true AnySeek or true model quality acceptance.

Manual inspection: The migration method still calls the same batch authority read method, with no new or modified SQL, dynamic SQL, or execution entry points; the sorting and review function bodies remain equivalent except for receiver/format. Ultimately, all native ranking participants are first fully reviewed, and then the return hits are verified at the original response stoppoints; the snapshot opening and closing, external IO order, graph seed filtering, and error order remain unchanged. The new Turso module follows the `local-storage` cfg, without introducing Service or sorting module dependencies. No Windows was run, no full integration suite was used, production performance was not tested, and no real client/model tasks were conducted.

## 2026-11-17: PR #78 synchronizes the main and conflict handling

Incorporate the main `44e06df` (including the PR #77 interface) into the retrieval diagnostic branch `0bf9748`. Resolve conflicts in CI, STATUS, and test entry points; retain Node UI build, real UI access, and fixed fixture caching. The caching key is shared with the checked-out version of AnySeek `d813b915b7f12e7139ca316adc15699688493196`. Also retain directory tests, retrieval diagnostic tests, Run list interfaces, and private preparation replay logic.

| Check | Result |
| --- | --- |
| Default Rust full test suite | PASS: lib 214, protocol 4, integration 202 |
| No default features in Rust full test suite | PASS: lib 136, protocol 2, integration 92 |
| Strict Clippy, format, and default build for two feature sets | PASS: Rust 1.98.0, all-targets / -D warnings |
| Node UI `npm ci --ignore-scripts`, `npm test` | PASS: Node 24.19.0, build and 15 tests |
| Python tools / evals | PASS: 11 / 42 items |
| Real Node UI → Origence → AnySeek | PASS: storage, directory, history, retrieval, context, private Run isolation, and source retraction |
| Real AnySeek contracts and HTTP/stdio MCP | PASS: 8 contracts, Run diagnosis, original result retry, corpus update, and restart |

All use isolated temporary data; Node accesses services via HTTP only. No modifications to the main UI source code, original evidence, dependency locks, or evaluation results. Local Linux checks do not represent real browser layout, Windows, production capacity, or model effectiveness acceptance.

## 2026-11-17: Retry and embedding space review fixes

Based on Origence `3d74879`, the backend is AnySeek [`d813b91`](https://github.com/TheLudlows/anyseek/commit/d813b915b7f12e7139ca316adc15699688493196). Linux; Origence Rust 1.98.0, AnySeek Rust 1.99.1.

- Successfully prepared to save private bundle snapshots, verifying events and request summaries before retrieval; reviewed current key/ownership, precise historical references, and transfer dependencies in replay and concurrent submission. Run reading/prefix synchronization completes the verification of the complete source of ordinary assets, avoiding only verifying Learning dependencies. Paged traces do not copy internal snapshots.
- Query diagnosis v2 checks the current embedding space's SHA256 sequentially; the same dimension and analysis digest cannot replace this verification.
- Realistic isolation reproduction: Initially 200, after the backend query changes to 503, the replay with the same ID still results in 200, with exactly the same response. Run has only one event, and the replay does not access the backend. When replaced with a different set of spaces, the vector query changes from 200 to 503.

| Check | Result |
| --- | --- |
| Default full set `cargo test --locked -j 2 --no-fail-fast` | PASS: lib 214, protocol 4, integration 201 |
| Full suite without default features | PASS: 136 library, 2 protocol, 92 integration tests. The command also used --no-default-features |
| Two feature sets with strict Clippy, default build and format | PASS: all-targets / -D warnings, Rust 1.98.0 |
| Targeted HTTP and storage regressions | PASS: replay during backend failure, reopen, conflicting requests, concurrent success followed by failure, cross-key and revocation isolation, blocking replay/trace/prefix on asset revocation, and rejection of embedding-space changes |
|Real AnySeek contract and HTTP/stdio MCP | PASS: 8 contracts and updated corpus/replay with unchanged original context and receipts |
|Python tools / evals | PASS: 8 / 42 items |

New SQL queries are all single-table queries with scope/run/event conditions; referenced related storage adapters are reused. Source checks and replays are in the same transaction, with no models or AnySeek IO. Historical evidence, dependencies, and frozen evaluation artifacts remain unchanged. This round does not declare Windows, production performance, real model effects, real S3, or power failure acceptance; previous CI results are only their respective SHA.

## 2026-11-17: Initial verification of retrieval configuration, diagnosis, and private Runs

Based on the main `dcde761`, the real engine is AnySeek [`88ab7c1`](https://github.com/TheLudlows/anyseek/commit/88ab7c154d1345c646b02a56b4dcc82e73995d0d). Local Linux, Origence uses Rust 1.98.0, AnySeek uses Rust 1.99.1.

| Check | Result |
| --- | --- |
| Default test `cargo test --locked --no-fail-fast` | PASS: lib 214, protocol 4, integration 201; 8 real backend tests run separately |
| No default feature full test | PASS: lib 136, protocol 2, integration 92; 5 real backend tests ignored according to this feature set |
| With and without defaults, `cargo clippy --locked --all-targets -- -D warnings` | PASS. The no-default command also used --no-default-features |
| Real AnySeek contract | PASS: 8 items, including full text/vector/graphs, replay, unknown writes, cleanup, and recovery |
| Real HTTP/stdio MCP, Run | PASS: configuration verification, request correlation, preparation logging, idempotent response, diagnostic consistency after rerun |
| Complete MCP smoke test | PASS: host forwarding/offline protocol, schema, security errors, revocation, EOF, and restart |
| Python tools / evals | PASS: 8 / 42 items |

New targeted regressions cover rejection of different embedding spaces with equal dimensions, verifying the analysis digest after reopen without overwriting the binding, concurrent request isolation and a 128-entry cap, successful and failed Run diagnostics, preserving original observations on same-ID replay, discarding malformed diagnostics, isolation of private reads, and reopening at the same version. Existing determinism checks now ignore only the new request identifiers and timings. They still compare content, citations, conflicts and budgets in full.

This fix gates test helper modules by feature, gates the graph verification method for local use only, and defaults missing properties in MCP empty-object schemas. Permissions, source, index, and publication checks were preserved. The v4 binding targets fresh directories. Old bindings are explicitly rejected without automatic data conversion.

Server progress is sampled separately after querying; diagnostics do not prove cross-system transactions, delivered items, or model usage. The real engine contract uses isolation test data, which does not represent the actual embedding quality, Claude Code/Codex task benefits, Windows, production capacity, real S3, or power failure acceptance. Historical segments are retained according to their respective submissions.

## 2026-11-17: Removal of independent Turso graph storage

The local `42c6e8f` is based on one-hop adaptation (same tree as `e281142` from [PR #74](https://github.com/TheLudlows/Origence/pull/74)). Local Linux / Rust version 1.98.0, code header `f0caae4c75df01c9e8e98f2c515caccbab0d032f`. Useless test re-exports found during full build have been removed, and the final Clippy pass was successfully achieved.

- `cargo test --locked --no-fail-fast`: 213 library tests, 199 integration tests, and 4 protocol tests passed; tests specific to the old graph storage have been removed.
- Application regression: New libraries do not create `graph.db`; old files are not read or modified; frozen graphs that have been written remotely and not committed can be cleaned up and replayed after a restart. The shared edges are retained with one source retained while the other is removed; neither the unpublished nor the removed evidence is disclosed.
- The actual local AnySeek `6614bbf`: one-hop direction/source/ substitution, BM25 sorting, and score comparison; HTTP/stdio MCP smoke test passed.
- Formatting, `git diff --check`, and `cargo clippy --locked --all-targets -- -D warnings` passed.
- No modification to the `context.db` schema, AnySeek code, or protocol; no GitHub Actions, Windows, or production acceptance tests conducted; no migration of old libraries provided.

## 2026-11-17: AnySeek one-hop candidate and original sorting

The baseline is `0a1a56c` from [PR #73](https://github.com/TheLudlows/Origence/pull/73); the verification version is `1094887725fe903fe1eaf0019e9e4ebf8b153842`. Local Linux, Rust 1.98.0; the actual AnySeek `6614bbf` uses a separate temporary directory.

| Inspection | Current Results |
| --- | --- |
| `cargo test --locked --no-fail-fast` | 222 library tests, 212 local integration tests, and 4 protocol tests passed; default real backend cases are recorded separately |
| Actual backend graph cases | Pass: 13 one-hop neighbors from the same source, in-and-out edges, Unicode substrings, scope/version/generation, cleanup and replay; same chunk before and after adding graphs, after clearing the graph, correlation sorting and scores are consistent |
| Actual AnySeek + Origence HTTP/stdio MCP smoke test | Pass: retrieval, reading, referencing, updating, history, key revocation and reopening; this general smoke test is not equivalent to native client acceptance tests |
| Formatting, Clippy (`--all-targets -- -D warnings`) and diff checking | Pass |
| Python tools and evaluation assistance UT | 8 + 42 Pass |

Independent engine comparison used 24 fixed chunks and the same query. Adding 40 graph-only Records without full-text fields changed BM25 ranking. Updating only graph metadata on existing Records instead produced identical ranks and scores across the 24 hits. The limited sample provides regression evidence. Public graph retrieval candidates still need exact publication proof from Turso. Stale index entries deliberately left after source updates and revoked entries stayed invisible before and after cleanup. No GitHub Actions, Windows, production-scale or actual client tasks were run. The deployment scope covered only fresh stores and the v3 collection.

## #73 Local run results for baseline

| Inspection | Result | Range |
| --- | --- | --- |
| `cargo test --locked --no-fail-fast` | PASS | lib 218, AnySeek protocol 4, integration 211; 433 items in total, with 7 items ignored by default running separately |
| Real AnySeek contract testing | PASS | 7 items, covering vectors, scope, response loss, replay, cleanup, and public service reopening |
| Real AnySeek → HTTP / stdio MCP contract | PASS | Search, reading, reference, version update/history, credential revocation, and reopening |
| `cargo fmt --package origence -- --check` | PASS | Current Rust package |
| `python -m unittest discover -s tools -p 'test_*.py'` | PASS | 8 items |
| `python -m unittest discover -s evals -p 'test_*.py'` | PASS | 42 items |
| `cargo clippy --locked --no-default-features --all-targets -j 2 -- -D warnings` | FAIL | Testing auxiliary modules with unconditional import of Services controlled by local-storage |
| `tools/mcp_smoke.py` for real AnySeek | FAIL | Empty parameter schema lacks properties, script triggers KeyError during inspection |

The actual AnySeek is fixed as [`6614bbf29bad0041c65738831bc1a16da4c69660`](https://github.com/TheLudlows/anyseek/tree/6614bbf29bad0041c65738831bc1a16da4c69660), using a isolated new collection. The vector is a contract fixture, not the semantic quality result of the actual embedding model. It can be reviewed according to the current tool interface (binary path replaced by the environment):

```sh
cargo build --locked
python3 tools/anyseek_smoke.py --anyseek-binary PATH_TO_ANYSEEK --cargo-tests --binary target/debug/origence
python3 tools/anyseek_smoke.py --anyseek-binary PATH_TO_ANYSEEK --run python3 tools/mcp_smoke.py --binary target/debug/origence
```

The failure location of MCP is [schema_object](../tools/mcp_smoke.py#L299). The diagnostic run treats properties missing from an empty object schema as an empty object, and then the host forwards the result, which is similar to an offline MCP check. **This diagnostic does not modify the repository script, and does not count as an original smoke pass. It also does not prove that the actual Claude Code/Codex task has been accepted.** The failure location without default properties is [Service import](../tests/support/mod.rs#L12); consistent with local and CI.

## Historical main CI (0a1a56c)

[Origence CI · run 43998026090](https://github.com/TheLudlows/Origence/actions/runs/43998026090) corresponds to the above exact main commit, overall **FAIL**.

| Job | Result | Failure Location or Boundary |
| --- | --- | --- |
| fast-check | FAIL | Authority-only Clippy, did not proceed to the full set of checks without default features |
| linux-native | FAIL | Default Clippy/testing, build, and HTTP smoke passed; MCP smoke reported KeyError, subsequent actual AnySeek full set and retrieval evaluation not run |
| windows | PASS | Only this commit, this workflow has executed Windows checks |

## No acceptance conclusions have been formed yet

- The actual Claude Code/Codex client is the same as the key archive query, Skill selection/loading, tool collection, and full task traceability.
- Candidate product processes generated from memory/execution experience, actual review by users, and development/operational benefits.
- Retrieval/factuality quality of the actual model, production capacity and latency, production backup recovery, and behavior in case of power failure.
- A new security audit of the current Cargo.lock. Audits of older dependencies do not establish current results.

## Historical Evidence

Phased acceptance, earlier implementation status and dependency decisions moved out of active documentation. See [VALIDATION before cleanup](https://github.com/TheLudlows/Origence/blob/0a1a56c1da74e78be0118782b04a3f5a37251060/docs/VALIDATION.md), [historical dependency audit](https://github.com/TheLudlows/Origence/blob/0a1a56c1da74e78be0118782b04a3f5a37251060/docs/DEPENDENCY_AUDIT.md) and [original artifacts](https://github.com/TheLudlows/Origence/tree/0a1a56c1da74e78be0118782b04a3f5a37251060/docs/evidence) for the original text, failures, commits and artifacts. Historical results apply to the stated commits and backends. See [STATUS](STATUS.en.md) for current progress.

## Historical Document Organization Review (PR #75)

Only Markdown has been modified; the implementation source code, configuration, dependencies, original evidence, and frozen evaluation results remain unchanged. Historical articles are retained through the fixed Git version mentioned above, without altering the original verification facts.

| Inspection | Result |
| --- | --- |
| Local Markdown destinations, anchors, UTF-8 and code fences across the repository | PASS: 24 documents, 138 local links and 30 fragment anchors |
| `git diff --check`, document change scope inspection | PASS |
| Old implementation/reviews directory and invalid activity links | Removed; current domain contracts can be accessed from the documentation entrance |
| Rust / Python execution checks | This document reorganization did not rerun; previous results were based on the same code baseline |

## 2026-11-17: Pre-merging verification of Node.js management interface (Issue #76)

This section corresponds to `3fc320ef9983aa844ae517e5a661c5045a758cac`, which is the evidence before merging the new main. Post-merging verification is listed separately, and the number of different code headers is not mixed together.

The implementation baseline is `0a1a56c1da74e78be0118782b04a3f5a37251060`. Tests were run within this change tree; precise release submissions are recorded in the PR. The environment includes Linux, Rust 1.98.0, Node.js 24.22.1, npm 11.9.0; the real AnySeek test dependency is fixed to `6614bbf29bad0041c65738831bc1a16da4c69660`. All behavior tests use newly created temporary libraries and test credentials, without invoking external models or accessing existing user data.

Passed:

- `cd ui && npm test` passed 12 tests, including 6 page-flow subtests. Coverage includes same-origin HTTP relay, concurrent user credential isolation, raw binary uploads, draft retention on 409, path/Origin/redirect rejection, timeout, UTF-8 locations for Chinese characters, emoji and combining characters, pinned-version diffs, idempotent form retry, five-page navigation and cleanup on 401. The command includes strict TypeScript compilation. happy-dom verifies only DOM and request contracts. Browser layout needs a browser.
- `cargo test --locked -j 2 --no-fail-fast`: 218 libraries, 4 AnySeek local protocols, 212 local integrations passed; 7 individually configured real AnySeek contract tests were kept ignored in this command, and no changes were made to make them pass.
- `cargo test --locked --no-default-features -j 2 --no-fail-fast`: 141 libraries, 2 AnySeek local protocols, 105 local integrations passed; 4 real backend tests were kept ignored.
- `cargo test --locked --test local catalog::catalog_pages -j 2` passed the 1 catalog case. Covers paging and cursor progress after filtering invalid sources, metadata without bodies, workspace isolation, denying other admins access to private Runs, authorized rotation retaining logical ownership, and rejecting old credentials.
- Default settings along with `--no-default-features` in `cargo clippy --locked --all-targets -j 2 -- -D warnings` and `cargo fmt --all -- --check` passed.
- `python3 tools/anyseek_smoke.py --anyseek-binary /workspace/anyseek-fixture/target/debug/anyseek_server --run python3 tools/ui_smoke.py --binary target/debug/origence --ui-entry ui/dist/server.js` passed for the real Node UI → Origence → AnySeek HTTP chain: authentication, file upload, asynchronous publishing, two types of directories, fixed version, keyword search, upper and lower text segment budgets, preparation of receipts, cross-key Run rejection, and source withdrawal. This real engine acceptance did not use a real model, so it does not prove the quality of retrieval or the benefits of Agent tasks.
- `python3 -m py_compile tools/ui_smoke.py` and `git diff --check` passed.

Correction in verification: The expected_version of the newly created knowledge request must be left empty, not 0; for idempotent deletion tests, the request key must be provided. Without a default feature, the full target check exposes the issue of unconditional loading of the Service fixture from `tests/local.rs`; only the feature gate corresponding to the caller has been added to this fixture module, without deleting any tests without a default feature.

Manual review of SQL: Added two-parameterized single-table SELECTs, reading Run for scoped asset candidates and the current logical key. The associated/visibility reuse is done using the original storage adapter within the same authorized read-only transaction; no JOINs, cross-table subqueries, additional commits, model/file/AnySeek I/O, schema changes, or migrations. The asset directory scans up to 100 candidates per page, and Run does not return events or expand the private read range. Windows behavior has not been verified in this environment.

Pending are real browser layout, keyboard, touchscreen and accessibility acceptance. The admin preview recognized standard host and port options, then reached the relay stage, where `sites-preview` reported `relay could not open browser port 4173`. Shared Chrome received a connection refusal at that address. DOM tests were not counted as browser evidence. Online deployment, production metering and alerts, real models and Claude Code/Codex tasks were also outside acceptance. Missing metering remains unknown.

## 2026-11-17: Node UI synchronization after main revision (PR #77)

Integrated with the main branch `dcde761ba11b34d88ce5da5f9025a22076571dfa`, retaining the implementation of the #74 diagram projection and the convergence with the #75 documentation. Only UI records are merged in the current document, without restoring old diagram storage or deleting the long-standing history. The final submission is identified by PR #77; the following numbers refer only to the code tree after this integration.

- `cd ui && npm test`: 12 items passed; additional coverage on knowledge pages includes file selection, raw Blob upload, format selection, and file_id storage requests. These still occur within the same page flow sub-test, without inflating the test count.
- `cargo test --locked -j 2 --no-fail-fast`: 213 libraries + 4 protocols + 200 integrations, totaling 417 passed; 8 real backend contracts remain ignored. The difference in quantity compared to the previous section comes from upstream graph implementation and test convergence. UI regression has not been removed.
- `cargo test --locked --no-default-features -j 2 --no-fail-fast`: 135 libraries + 2 protocols + 92 integrations, totaling 229 passed; 5 remain ignored.
- Strict Clippy and Rust format checking with no default features, along with `git diff --check`, are passed.
- The real AnySeek `6614bbf` is rerun on Node UI → Origence HTTP full-link smoke test, passed.
- The original `tools/mcp_smoke.py` is run on the same real engine. Both host-forwarding and offline paths are passed, covering version negotiation, schema, tools, controlled errors, active revocation, EOF/reboot; this is not a memory diagnostic substitute for the previous one.
- `python3 -m unittest discover -s tools -p 'test_*.py' -v`: 11 passed items.

For empty-argument schemas, only a missing optional properties object defaults to empty. Missing fields, changed fields and relaxed additionalProperties are still rejected. The 3 new unit tests retain these assertions. The graph helper compiles only with its caller's local-storage feature, without suppressing checks with allow(dead_code) or changing default graph search. The earlier historical FAIL remains intact.

Windows, real browser, and production/client acceptance boundaries are the same as before; still not verified. New GitHub CI results can be found in PR; local passes are claimed to be fully passed by remote CI.

## 2026-11-17: UI code review and CI duplication (PR #77)

The review baseline is `bb0e6d9876ac1d5e5c303e55368e016ee1e0bf27`. The [GitHub Actions #360](https://github.com/TheLudlows/Origence/actions/runs/44013075805) for this baseline has been completed: `node-ui`, `fast-check`, `linux-native` passed; `windows` is skipped according to the existing main-only strategy. The following modifications and local verifications belong to subsequent work trees. Exact commits and modified remote results can be found in PR #77.

Code review and correction of three asynchronous interface behaviors: After canceling credentials/routing, responses that are still decoding cannot be returned; old retrievals cannot override new queries; exiting the waiting state after canceling context preview allows re-previewing. First, two API regression tests were conducted to reproduce decodes that removed effects (all failed due to no response being rejected). After correction, the tests were run as follows:

- `cd ui && npm test`: All 15 items passed, including strict TypeScript compilation, two decodes canceled by regression, disordered retrievals, and re-previewing after cancellation, as well as the original HTTP/page contracts.
- Copy `ui/dist/` to a temporary directory separately, and start `tools/ui_smoke.py` with `server.js` in that directory, using the real Node → Origence → fixed AnySeek `6614bbf` link. This verifies that the integration job only requires UI build artifacts, without installing npm dependencies or recompiling the UI.
- Official `actionlint 1.7.7 .github/workflows/ci.yml` and `git diff --check` passed. The binary SHA-256 was verified against its official release. No application dependency was added.

CI adjustments based on log #360, rather than reducing independent acceptance:

- Removal of Ubuntu native tool installation (about 7.5 seconds, log shows 0 newly installed), toolchain version duplication assertions, and a subset of `memory_identity` that has been fully tested without default features by `fast-check` (about 12.8 seconds).
- Sharing Rust debug/incremental configurations; four job names remain unchanged. Maintain the full test with default/without default features, strict Clippy, dependency tree checks, original full MCP, real backend contracts, evaluation evidence, and Windows policies.
- Keep `cargo build --locked -j 2`: Testing Turso features with dev-dependencies cannot be replaced by test artifacts with normal service builds.
- AnySeek binary uses independent cache keys for exact source code, Rust version, Ubuntu/architecture, and build configuration, without using loose fallback keys. #360 recompiles the fixture in about 58.6 seconds, with another 6 seconds for toolchain preparation; the cache only contains executables, and all behavior acceptance still uses newly created temporary data. Measured cache hit times are available in PR.
- `node-ui` uploads the build once. `linux-native` reuses it and adds the actual UI HTTP contract gate, which previously had local evidence only. DOM tests were not counted as browser layout acceptance.

This TypeScript and workflow change did not modify Rust source, schema or SQL or rerun the full local Rust suite. The remote workflow still runs the complete Rust checks above. Windows, real-browser and production acceptance boundaries remain as recorded.
## 2026-11-17: Full SQLite replacement and metadata reduction

Baseline main `01d4f82b20c986278b077e8660f9bbc0538e8d6c`. Validation ran in the SQLite replacement worktree, with the exact commit and remote results identified by this PR's head. Linux, Rust 1.98.0, rusqlite 0.43.0 and bundled SQLite 3.54.1. Live AnySeek was pinned at `d813b915b7f12e7139ca316adc15699688493196` and built with its required Rust 1.99.1. All behavioral checks used fresh temporary databases and test credentials, without external model calls.

The replacement covers all relation and queue adapters, Service/CLI/error boundaries, tests and manual diagnostics, Cargo lockfile, CI, containers and current docs. Old engine modules and aliases were removed. Historical evidence and actual failures are preserved. Cargo generated a new lockfile with 370 packages, down from 482: 116 removed and 4 added, net −112. Versions of retained packages stayed unchanged. rusqlite disables default features and enables only bundled. No ORM, async database or third-party pool was added. The all-features production dependency tree excludes every retired storage engine. Removed the unused direct `futures` declaration. rmcp still pulls it transitively. Resolved packages and features were identical before and after this removal, and the final `cargo check --all-features --all-targets -j 2` passed.

Removed metadata columns `oc_versions.content_hash` and `oc_artifact_ledger.idempotency_key`, with redundant caller fields, parameters and string helpers. Compute content digests from the immutable body as read. The ledger uses ON CONFLICT idempotency over the existing full primary key. External frozen plans and operation epoch semantics are preserved. Tests still verify command idempotency, Run and Skill snapshot digests, authorization, sources, versions and independent owner/index/ledger states.

Passed checks

- `cargo clippy --locked --all-features --all-targets -j 2 -- -D warnings` and `cargo clippy --locked --no-default-features --all-targets -j 2 -- -D warnings` passed. The former compiles manual diagnostic targets. Running those diagnostics does not count as production acceptance.
- `cargo test --locked -j 2 --no-fail-fast` passed **424 tests**: 218 library, 4 protocol and 202 local. The 8 live backend contract tests still run separately and are ignored by the regular command.
- `cargo test --locked --no-default-features -j 2 --no-fail-fast` passed **232 tests**: 138 library, 2 protocol and 92 local. The 5 live backend tests stay ignored in that configuration.
- `cargo build --locked -j 2`, formatting, `git diff --check`, actionlint 1.7.7, Cargo metadata and the all-features retired-engine dependency check passed. The regular service binary contains only the new SQLite format marker. Linux `ldd` shows no dynamic SQLite or libgomp dependency.
- `npm --prefix ui test` passed 15 tests. Python tooling passed 11 and evaluation adapters passed 42.
- Ran `tools/smoke.py`, `tools/ui_smoke.py` and the original full `tools/mcp_smoke.py` individually against the regular service binary and pinned live AnySeek. HTTP, UI, and host-forwarding and offline MCP passed.
- `tools/anyseek_smoke.py --cargo-tests` passed **8 live engine contract tests**. Subsequent HTTP/stdio MCP, citations, version updates, revocation, reopening, Run diagnostic correlation, retry and persistence also passed.
- `evals/run.py` ran 24 keyword cases with errors=0 and resolve_errors=0. Document recall, MRR and NDCG were each approximately 0.63636. This synthetic engineering check does not establish model or real Agent task gains.

New regressions verify WAL, FULL, foreign keys and busy timeout on every connection, a five-lease cap, cancellation and FIFO rollback on real files, draining RETURNING, continuing-commit refusal after errors, read snapshots, cross-pool release, host lock lifetime, direct SQLite integrity and foreign-key checks, removed metadata columns, original Chinese byte digests and same-version reopen. Read-only tests separately cover a nonempty committed WAL that must be readable and a checkpointed path with Chinese characters, spaces and special URI characters that creates no sidecar. They do not use immutable to ignore a WAL needing recovery.

Early regressions exposed assumptions from the old engine. SQLite converts large JSON integers to REAL, keeps independent per-connection DDL caches, and can update SHM read marks during read-only access. Use original SQLite results as the oracle and retain invalid identity, decode order, authorization and evidence budget assertions. No compatibility branches for retired engines were added. Corrupt-schema fixtures reopen their deliberately changed DDL. Active-store checks compare the database and WAL, without treating ephemeral SHM coordination bits as durable business bytes.

Manual SQL review confirmed removal of two metadata columns and their bindings and reads. Version digests are computed in the same authorized read transaction. There are no JOINs, cross-table subqueries, extra business commits or model, file or AnySeek I/O inside transactions. Database I/O runs on the dedicated blocking FIFO connection. Cancellation does not forcibly stop SQL. An unknown outcome after COMMIT was issued still requires checking the original idempotency key. The change targets fresh stores. Existing files are neither migrated nor rewritten or automatically converted. Older formats are rejected read-only before opening for writes.

This CI extends Windows checks to PRs. See the PR for remote results. Linux evidence does not apply to Windows. The first standard Docker build failed fetching registry/config.json because of this environment's self-signed CA chain. The same Dockerfile resumed with a temporary build-time CA secret added to the trust chain. TLS verification stayed enabled. See the PR for the final container result. Browser layout, live models and dual-client tasks, and production performance were outside this acceptance.

## 2026-11-17: Core library without a metadata database and on-demand access (PR #81)

Synchronized main `593e8fa` on SQLite replacement baseline `01c0d4c` and kept #80's Run source UI fix. The following results apply to this worktree. The PR head and run links identify the exact commit and new Windows/Linux CI. Previous #372 success and container images do not establish this round's results.

Actual dependency and access reductions

- `rusqlite` and `fs2` are enabled only by the optional `sqlite` feature. With defaults disabled, the core library has no SQLite implementation or SQLite error type. Full host `local-storage` enables SQLite automatically, and a standalone authority explicitly enables it.
- Split Blob/PDF core integration tests so they always run. Core and host share the PDF-byte fixture. No existing tests were deleted or run twice in the same configuration. Manual diagnostics depend on `sqlite` when their actual callers need it.
- Service entry points reuse identity reads from the same transaction for authorization. Commit rechecks authorization. Keyword candidates share the initial authorization snapshot, which closes before external I/O. Vector and Hybrid retain separate authorization snapshots before and after model calls.
- An empty read set, relation lookup or summary query does not prepare unrelated tables. Learning stops unrelated queries when data is absent, a prerequisite is false, or a parent job is canceled. Required data and nonempty evidence failures still reject operations. Opening the store and explicit health checks validate the full schema. The change narrows which empty operations consume data without suppressing required query errors.
- Pure vector requests skip the lexical path. Other tokenization runs in the blocking pool outside transactions.

Passed on Linux Rust 1.98.0

| Configuration / command | Result |
| --- | --- |
| `cargo test --locked --no-default-features -j 2 --no-fail-fast` | 71 library + 2 protocol + 14 core = **87 tests**. The 5 live engine checks remain ignored as configured |
| `cargo test --locked --no-default-features --features sqlite -j 2 --no-fail-fast` | 138 library + 2 protocol + 14 core + 78 local = **232 tests**. 5 ignored |
| `cargo test --locked -j 2 --no-fail-fast` | 221 library + 4 protocol + 14 core + 188 local = **427 tests**. 8 ignored |
| All-targets Clippy across three configurations (all features in the full configuration) | All passed with warnings denied |
| Regular service build, format and diff checks, actionlint 1.7.7 and Cargo metadata | Passed |
| `cargo tree --locked --no-default-features --edges normal,build --prefix none` | No rusqlite, libsqlite3-sys or fs2. CI adds the same gate |
| Node UI | 15 tests passed, including source distinctions after merging #80 |

New resource regressions use per-test store counters compiled only into unit tests, with no production metadata or latency threshold. An empty read set on a store without application tables yields **0 client driver calls**. Actual evidence requests still return a storage error there. A keyword query with an empty corpus uses **3 authorized read transactions** (previously 4) and at most 20 client driver calls. This path needs neither the index/Summary tables nor reachable AnySeek, while explicit `check` still rejects an incomplete schema. Transaction entry needs only BEGIN and key and workspace reads (3 driver calls), without duplicate identity reads. Revoking its own key within the transaction rejects commit and rolls back. An authorization object from before external revocation cannot start a new transaction.

With pinned live AnySeek `d813b915`, the regular service binary passed HTTP, Node UI and full MCP host-forwarding and offline smoke checks. Separately passed 8 live backend contract tests and their HTTP/stdio, revocation, reopen and Run diagnostics persistence acceptance. All data was created in temporary fresh stores, with no external model calls.

Manual SQL review confirmed that parameterized single-table SQL for nonempty queries stayed unchanged and association still occurs in the same adapter transaction. Only empty batches, predetermined branches and duplicate initial authorization reads were reduced. No JOIN, extra commits, cross-request authorization cache, model/file/AnySeek I/O in a transaction or business metadata was added. DDL and format were unchanged. The container release build was not rerun. Existing SQLite container interoperability evidence stays tied to its original commit. See the PR for remote results.

## 2026-11-17 · PR #83 synchronization with merged SQLite #81

Merged main `f62168b4cbedfbe6a3a97dbb3161f9e6375fb2df` into the existing #83
head `537fdbd982740c1283f3ef0e01dc47ef6a651347` without rewriting history.
Coordinated Run storage with SqliteTx/sqlite_db/sqlite_err and updated the tests'
old driver paths. Client events and server-prepared events both retain the reverse
projection insert in their existing write transaction. Fresh-only schema admission,
first-reference coverage and current-logical-key/source guards are unchanged.

Promoted the narrowly scoped combination diagnostic into the existing catalog
regression: an injected projection-insert failure rolls back the client event and
server-prepared event, then retrying the same server event ID succeeds and yields
the exact v2 association. Withdrawing independent v1 leaves that v2 association
visible. This proves ordinary transaction failure handling, not power-loss durability.

Executed on the actual synchronized worktree, Linux / Rust 1.98.0 / Node 24.22.1:

- `cargo test --locked --test local asset_run_references_are_exact_bounded_private_and_withdrawal_gated`:
  1 passed, 189 filtered (fixed version, other-admin/key/workspace isolation,
  rotation, pagination/limits, authoritative proof recheck, reopen, withdrawal,
  client/prepared projection rollback and successful retry).
- `cargo test --locked --test local workspace_schema_rejection`:
  5 passed, 185 filtered.
- `cargo test --locked --no-default-features --features sqlite --test local workspace_schema_rejection`:
  5 passed, 74 filtered; includes missing reverse table and old-layout rejection
  without repair. These integration tests use the existing protocol peer for
  AnySeek and are not real-engine ranking acceptance.
- `cargo build --locked`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo clippy --locked --no-default-features --features sqlite --all-targets -- -D warnings`: passed.
- `cargo clippy --locked --no-default-features --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`, `git diff --check`: passed.
- `npm --prefix ui run check`; `npm --prefix ui test`: passed, 16 UI tests.

Real AnySeek pin `d813b915b7f12e7139ca316adc15699688493196`, models disabled,
fresh temporary v4 data, disposable keys, rebuilt synchronized Origence binary:

- `python3 tools/anyseek_smoke.py --anyseek-binary "$ANYSEEK_BINARY" --run python3 tools/mcp_smoke.py --binary "$ORIGENCE_BINARY"`:
  passed both host-forwarding and offline MCP (protocol/schema/tools, safe errors,
  live/invalid/revoked credentials, EOF and reopen).
- Reused the real Chromium fixture via the same AnySeek launcher with the
  synchronized UI/server. Chromium 156.0.8078.79 passed Run -> fixed v1 after v2
  publication -> recorded association -> same Run; v2 and another same-workspace
  admin cannot enumerate v1-owned associations. Browser source withdrawal,
  association refresh, stale citation re-click, URL reload and Run reopen all
  deny old content; server preparation failure remains server-observed/not-prepared.
  No route interception, production data, paid models, screenshots or credentials
  were added to this commit.

Windows and full combined CI were not run locally. GitHub results must be checked
on the new pushed HEAD; the old separate PR green runs and the earlier temporary
combination tree are not evidence that this new HEAD passed remote CI. No complete
#76/native Claude Code/Codex task or production acceptance is claimed.

## 2026-11-17 · Windows-native development launcher (local)

Independent branch based on main `c70a1aaa0b3cb75872109406dc17b1b461d1f771`
(includes SQLite #81 and reverse references #83); no older feature branches included.
`tools/start-dev.ps1` targets foreground PowerShell 7.4+ and prebuilt native executables.
It requires already provisioned separate data directories, ordinary Tenant key and
collection; no startup provisioning, migrations, directory creation, overwrite,
remote writes, secret arguments or persistent logs. Loopback only, models disabled.
AnySeek readiness reads the authorized collection metadata; Origence's own full
backend admission and /health/ready gate the optional UI. Startup/HTTP/grace waits
are bounded. Only owned Process handles are stopped; no process-name/port killing.

Commands and results:

- PowerShell 7.4.6 Linux parser check of start-dev.ps1: passed. Initial pwsh startup
  could not create its default cache on the read-only home; XDG_CACHE_HOME,
  XDG_CONFIG_HOME and XDG_DATA_HOME were set to disposable paths for validation.
- `ORIGENCE_TEST_PWSH=/path/to/pwsh python3 -m unittest discover -s tools -p test_start_dev.py -v`:
  7 passed on Linux pwsh. Fake HTTP subprocesses verify ready/API/UI ordering,
  early exit, not-ready timeout, auth denial, redirect refusal, missing-data refusal,
  occupied-port noninterference, owned-process cleanup, spaced argument paths,
  credential separation and suppression of deliberately secret-bearing child output.
  These are process/HTTP contract fixtures, not real backend or Windows results.
- `python3 -m unittest discover -s tools -p 'test_*.py'`: 11 passed, 7 opt-in launcher
  cases skipped without ORIGENCE_TEST_PWSH. No CI scope/configuration changed.
- `cargo build --locked`, `cargo clippy --locked --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, `git diff --check`: passed under Rust 1.98.0.
- `npm ci --ignore-scripts --prefix ui` (disposable cache), `npm --prefix ui run build`:
  passed with Node 24.22.1.

Real local validation used pwsh 7.4.6 on Linux, actual AnySeek built at
`d813b915b7f12e7139ca316adc15699688493196`, Origence rebuilt from this worktree,
and the built Node UI. A separate fixture explicitly created a temporary Tenant,
ordinary key/collection and offline workspace, stopped bootstrap AnySeek, then
invoked the unmodified launcher with paths containing spaces. Twice it observed
all readiness gates, authenticated /v1/whoami retaining the same workspace, UI
HTTP 200, group SIGINT shutdown without force warnings, closed ports and retained
context.db; the second start reused the first data. Captured parent output was
checked not to contain master secret, master key, ordinary key or workspace token.
No production data, paid model/provider calls, Windows emulation or WSL was used.
The local real-service harness is external to the repository; the reproducible
manual setup/commands and Windows checklist are in WINDOWS-DEV.md.

Validation found and corrected a PowerShell case-insensitive variable collision
that replaced the component display name with an environment variable name.
The real bootstrap fixture also initially precreated an empty AnySeek directory:
the pinned engine interpreted that as recovery and exited. Documentation now uses
an absent path for explicit first initialization, while the launcher requires an
existing nonempty initialized directory. No automatic repair was added.

Not executed: native Windows/MSVC/Windows Terminal Ctrl+C, ready-stage interruption
on Windows, Windows process-reuse races, production recovery or power-loss tests.
Shared-console Ctrl+C is best effort until verified there. If signal delivery fails
or grace expires, owned-process termination is explicitly a forced stop; closing
or killing the parent terminal may bypass finally and leave children alive. The
launcher is not a production service supervisor. No remote PR/push/deployment.

## 2026-11-17 — #87 minimal MCP Run host adapter (local only)

Base and read-only latest-main check: `293e2bf6af3b180d43c8050731047b552ac07519`.
Independent branch `p0/mcp-host-adapter`, worktree `/workspace/origence-mcp`.
Read repository AGENTS.md, current status/design, Run contract and client/API
implementation; no repository `.agents/skills` exists and workspace `.agents`
is empty. Interface and two-host observation boundaries are frozen in
[the host contract example](../examples/run-host-contract.md).

Four MCP tools delegate through typed HostClient methods to existing Run HTTP
routes. Shared DTOs, Run storage, authentication, SQL, navigation, CI and STATUS
are unchanged. Manual execution-input review found no added SQL or storage calls;
all authority/source/version/replay decisions stay on the existing HTTP path.
Embedded local MCP explicitly rejects these new Run operations with a server-URL
requirement; the documented remote CLI/MCP path is covered.

Linux, Rust `1.98.0 (88d9e12ae 2026-08-18)`, rmcp `1.8.0`, models disabled,
fresh temporary SQLite, synthetic content and loopback AnySeek protocol fixture:

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test --test local mcp_runs -- --nocapture`: 1 passed.
- `cargo test --test local foundation_http -- --nocapture`: 1 passed.
- `cargo test --test local retrieval_diagnostics -- --nocapture`: 2 passed.
- `git diff --check`: passed.

The new test launches two actual Origence stdio processes and verifies tool
listing, same-logical-key archive and turn readback, independent Run IDs, other
writer/admin denial, unchanged retries and changed-payload conflicts, all seven
existing report variants, explicit unknown, rejection of forged server events,
HTTP preparation success/replay/failure, source withdrawal, credential rotation
and revocation of the replacement credential on an already-open MCP connection.
Initial test failure was a fixture mistake: it revoked the old credential ID
after rotation. The corrected test revokes the returned new credential ID;
server key semantics were not changed. Final rerun passed.

These synthetic peers are not Claude Code/Codex task evidence. Claude Code is
not installed in this environment; no real host/model task was invoked. Actual
two-host/version-frozen task observations, native Windows and enterprise task
outcomes remain unaccepted. No production credentials/data, paid model calls,
migration, push, PR, Issue update, merge or deployment was performed.

### #87 publication follow-up — explicit HTTP feedback/Improve and MCP gates

Continues local implementation `3877322753ff8bd2b921f5fe1a43e6a33352b108`.
Read #87 and #89: the four Run tools do not replace explicit turn archival,
session/evidence feedback or Improve, and do not add independent verification.
[Runnable synthetic HTTP commands](../examples/run-host-contract.md#explicit-archive--feedback--improve-over-existing-http)
use the same logical-key session and preserve explicit opt-in for Improve.
Run feedback remains a report; helpful feedback is not verified task success.
No service API, persistence, generic DTO, CI, navigation or STATUS change.

New protocol coverage: MCP archive → HostClient HTTP feedback → readback,
idempotent feedback and changed-payload conflict, other writer/admin denial,
explicit Improve with disabled models returning unavailable and leaving no accepted
improvement. The existing 20 local synthetic-model Learning regressions cover
positive Improve HTTP acceptance, processing, exact citations, recovery and source
withdrawal. No real provider or paid model is used.

The old exact seven-tool gates were updated to the exact eleven-tool contract,
including required fields and nested context schema. Report is now explicitly an
object in the adapter schema and still decoded into the existing strict Run DTO.
The first full smoke found a second stale assumption: revoked calls defaulted to
retrieval `{query}` arguments. Valid per-Run-tool arguments now exercise the same
strict unauthenticated error assertion for every tool; no error gate was relaxed.

Final local results (Linux, declared Origence Rust 1.98.0):

- `cargo test --test local mcp_runs -- --nocapture`: 1 passed.
- `cargo test --test local local_host_api_cli_mcp_models_and_recovery -- --nocapture`: 1 passed.
- `cargo test --test local learning_loop -- --nocapture`: 20 passed (local synthetic model fixture).
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo build --locked`, `cargo fmt --all -- --check`, `git diff --check`: passed.
- `PYTHONPATH=tools python3 -m unittest discover -s tools -p test_mcp_schema.py`: 3 passed.
- Actual stdio eleven-tool schema check with a temporary synthetic auth stub: passed.
- `python3 tools/anyseek_smoke.py --anyseek-binary /workspace/anyseek-fixture/target/debug/anyseek_server --run python3 tools/mcp_smoke.py --binary target/debug/origence`: passed host-forwarding and offline modes, all protocol negotiations, exact schemas/tool lists, sanitized errors, live revocation, EOF and reopen.

The smoke used a clean checkout of the CI-pinned real AnySeek source
`6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`, rebuilt with Rust 1.99.1,
`--locked --no-default-features --bin anyseek_server -j 2`. Binary SHA256:
`8e9eb15edd86fb38b4a837476ed219bc6c7bc85a89d9f7460bb4c18316a57beb`.
Upstream build warnings were not changed. All databases, collections, content and
credentials were disposable synthetic fixtures. This is real service/protocol
coverage, not real Claude Code/Codex task or independent business-result evidence.

Immediately before publication, remote main remained
`293e2bf6af3b180d43c8050731047b552ac07519`; no branch-matching PR existed.
No rebase or force push is needed. User authorization now permits a normal push
and independent draft PR with Refs #87; the earlier local-only record above is
historical. Exact published HEAD and CI state are recorded in the PR/handoff.
Real two-host/version-frozen tasks and Windows remain unaccepted. No Issue update,
merge or deployment is authorized by this follow-up.

## 2026-11-17 — #89 External Run verification records (local P0 slice)

- Fetch confirmed main/base at `293e2bf6af3b180d43c8050731047b552ac07519`. Commit `5965f50848a17a6206c91110ffee8d2636efd32c` contains the implementation corresponding to the tests below. Separate branch `p0/run-verification` and worktree `/workspace/Origence-run-verification`.
- See the [run-verification contract](contracts/run-verification.md). Only the owner transcribes external conclusions. `reporter=user/test`, external verifier labels, self-reported success and prepared states do not establish certified roles or platform verification. Prefix digest, recording credential, server time, claimed verifier and time, conclusion, evidence sequences and digests, and revocation state are stored separately. Unknown versions do not default to latest. A verifier role grants no Run body access. MCP and integration-tool APIs stayed unchanged.
- Linux, Rust `1.98.0 (88d9e12ae 2026-08-18)` and Node `v24.22.1`. Used `CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup CARGO_TARGET_DIR=/workspace/target-run-verification`, fresh tempdir stores, synthetic text and identities, Models::disabled and loopback AnySeek protocol fixtures. No production credentials, real tasks or paid model calls.
- `cargo test --lib run --features sqlite` passed 8 tests. Covers strict DTO rejection of forged roles, exact-prefix conflicts, failed external verdict with self-reported success, unknown and exact claimed versions, retry and conflict, revocation, pagination, same-schema reopen, denial for another admin key, and read/write/replay denial after credential revocation.
- `cargo test --test local run_verification --features sqlite` passed 2 tests. Public Service and HTTP verified prepared asset v2, self-reported success, an owner-transcribed failure and unknown actual Skill version. Subsequent Run appends preserve the verified prefix. Source revocation blocks verification reads, replay and revocation calls. Exact schema validation rejects missing tables without changing the original database bytes, migrating or backfilling.
- `npm --prefix ui test` passed 17 tests, including tsc and build. Verified external verification display, revocation, unknown versions, untrusted text not rendered as HTML, clearing stale verification after a failed refresh and subsequent recovery, and exact GET relay credential forwarding. The relay adds only the `/v1/runs/{UUID}/verifications` GET allowlist. Shared navigation, generic DTOs, CI and STATUS are unchanged.
- `cargo fmt --check` and `git diff --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed. The first run found a `collapsible_if` in a new DTO. The equivalent conditions were combined as suggested and checked again at the final implementation commit.
- Manual SQL review found only single-table SELECT/COUNT/INSERT/UPDATE in the verification module, test database setup and credential revocation. No JOINs, comma FROM, cross-table subqueries or CTEs. Prefix links, sources and logical owner are rechecked in the existing authorized transaction, with the existing live authorization check before commit. No external I/O or intermediate commit.
- Pending acceptance includes Windows behavior, actual browsers and accessibility, dual-client and enterprise tasks, real model quality, and independent verifier sign-off. Current UI is read-only with refresh. Writes and revocation use separate HTTP/Service APIs. This slice does not complete #89 or provide tracing, cost measurement or production alerts. Integration and pilot owners still need to use the separate contract. Release follows confirmation of the joint contract.
- Local commits only. No push, PR, issue update, merge or deployment.

## 2026-11-17 — #89 Targeted recheck before draft publication

- Continued from local `87b284c8fa3b9c324db2aa039af69a026b2c1dd5`. After fetching again main was still `293e2bf6af3b180d43c8050731047b552ac07519`. Test and contract commit for this round: `ddca4c52b3d71b2650f0ad13ce1e613089a42858`.
- Comparing with the published Issue #87, the collection contract has been explicitly archived, with separation of prepare and verification processes. There are no changes to `src/run.rs` or the MCP integration interface. No public implementation PRs for joint operation in #87 were found, so joint acceptance is not declared. This path only corresponds to the "Run Owner Record External Verification Basis" in the PRD; no independent verification, platform verification status, or business acceptance rate is generated. Users cannot elevate themselves to verification authorities using the owner function; #89 is not closed.
- Running `cargo test --lib run --features sqlite` again: 8 passed; `cargo test --test local run_verification --features sqlite`: 2 passed. Strengthening the real Service/HTTP readback assertions: separation of prepared, self-reported success, failed transcription, `platform_verification:not_performed`, `verifier_authentication:unverified_claim`. Non-owner reviewers have no right to revoke; after source withdrawal, both verification and Run text return NotFound/Forbidden.
- `npm --prefix ui test`: 17 passed; `cargo fmt --check`, `git diff --check`, `cargo clippy --all-targets --all-features -- -D warnings` all passed.
- Chromium `156.0.8078.79`: Running `node ui/scripts/check-run-verification-browser.mjs` after `npm --prefix ui run build` passed. Real Chromium DOM, built UI, and relay were used to synthesize HTTP responses. Covering preparation/report/external conclusions showed different versions, revocation, untrusted text encoding, refresh to clear old verification content when source was unavailable, and text/verification not displayed after another Key refusal. This browser fixture does not replace the real SQLite/Service verification, nor does it represent real dual-client/production task acceptance.
- The schema difference only adds `oc_run_verifications`, continuing the precise fresh-only admission. This round passed again, with table refusal and unchanged database bytes tested. The application SQL remains single-table, while cross-table associations and authorization/review remain in the original transaction.
- It is allowed to push this branch normally and create a draft PR for main (Refs #89). No changes to the issue, no rejection, no deployment. Windows, full accessibility, real dual clients/mission tasks, and external independent verification are still not accepted.

## 2026-11-17: #88 Additional verification for Learning source (draft #94)

`a709dfe2702efcb223d7765fc9ae0b7a71a838c9` adds deterministic tests only. Another writer in the same workspace selects exact published Learning v1 over the real CLI and HTTP. Reading private supporting evidence is denied. After revoking the supporting source archive, both the candidate awaiting publication and the published package return NOT_FOUND, while cleanup remains pending. The 21 Learning regressions, existing Run CLI regressions, formatting and Clippy passed. See the [#88 follow-up record](https://github.com/TheLudlows/Origence/blob/a56246406a0688f646a45a85cc2954fb14a8d28a/docs/validation/issue-88-skill.md#learning-source-follow-up-2026-11-17-draft-pr-94). Product code stayed unchanged. Results do not establish live client or business acceptance.

## 2026-11-18 — #95 coordinated integration after #93/#94

Merged main `a56246406a0688f646a45a85cc2954fb14a8d28a` into original #95
`dfbaade8eb8848d6085cf5b9f61f986f2c8b9428` without rewriting history. Only this
validation history conflicted; both entries remain. Automatic module/UI-test
merges preserve the existing source, exact-prefix, logical-key and fresh-only
schema contracts; no implementation change was needed.
Linux Rust 1.98.0: fmt and diff checks, `cargo test --locked --lib run_verification`
(2), `cargo test --locked --test local run_verification` (2),
`cargo test --locked --test local workspace_schema_rejection` (6), and
`cargo clippy --locked --all-targets --all-features -- -D warnings` passed.
Node UI build/tests: 19 passed. All database fixtures were newly created temporary
stores, not old stores converted to the new schema. No external model or production
material was used. New exact-HEAD CI is required before merging; full pilot remains NOT_RUN.

## 2026-11-18 — #96 coordinated integration after #93–#95

Merged main `e78b6ba5f3d6fcb4730789981a092c85cdc4c12a` into original #96
`07ddb1640c652bdd82c8ba168fe86bac9563cfa9` without history rewriting. Only this
validation history conflicted; all records remain. No MCP/HTTP, storage, DTO or
CI behavior changed during resolution.
Linux Rust 1.98.0: fmt/diff checks, `cargo test --locked --test local mcp_runs::`
(1), `cargo test --locked --test local foundation_http::` (2), full-feature
all-target strict Clippy, and `cargo build --locked --bins` passed.
`PYTHONPATH=tools python3 -m unittest discover -s tools -p test_mcp_schema.py`:
3 passed. Databases and protocol/model fixtures were fresh and synthetic. These
checks do not certify real native clients or business outcomes. The new exact-HEAD
CI must pass before merge; actual final main will be independently retested.

## 2026-11-18 — #76 Routing focus and dialog name (local)

Based on main `48cfa7c643b8c077ad33c55981def51979afde05`. When switching pages, focus is placed on the new main area. Invalid addresses or versions focus on incorrect content, and the target does not enter the Tab order. Focus moves once during page replacement, and the user-selected controls are retained after subsequent requests are completed. The shared edit and extract dialogs provide accessible names based on existing titles, and the behavior when closing follows the native dialog. Interface layout, routing structure, and API remain unchanged.

- Linux, Node 24.22.1. `npm --prefix ui run check` and `npm --prefix ui test` passed, 19 items. Reusing the page DOM process asserts the association between the main area focus and the dialog title.
- Chromium 156.0.8078.79, built UI, actual Node forwarding, and synthetic loopback HTTP. Both 1024px and 320px passed keyboard page switches, repeated navigation, browser back/forward, invalid paths, focus and input retention during slow directory calls, accessible names for edit/extract dialogs, focus restoration on Escape and close button, and no whole-page overflow with a dialog open. Run `ui/scripts/check-navigation-focus-browser.cjs` separately using Node 24, Playwright 1.62.1, and Chromium. Run below from repository root with local Chromium path. Playwright is installed under a temporary directory.

  ```sh
  browser_deps=$(mktemp -d)
  npm install --prefix "$browser_deps" --ignore-scripts playwright@1.62.1
  npm --prefix ui ci --ignore-scripts
  npm --prefix ui run build
  NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
    node ui/scripts/check-navigation-focus-browser.cjs
  ```
- `node ui/scripts/check-run-verification-browser.mjs` passed. `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`, and `git diff --check` passed.

This round of testing is limited to the aforementioned UI regression. Screen readers, physical touch screens, other browsers, the complete #76, and the actual product client tasks are still pending acceptance. Recordings and implementations are submitted locally.

## 2026-11-18 — Fixed version of #76 private Run within PDF pages

- The latest main verification is `48cfa7c643b8c077ad33c55981def51979afde05`. Independent local branch `fix/issue76-pdf-page`. The local object library did not find candidate `56989af`, which is not considered a deliverable. Refer to the repository AGENTS.md and the source/Run contract. There are no applicable `.agents/skills` in the repository and workspace.
- Linux x86_64, Rust/Cargo 1.98.0, Node 24.22.1, Chromium 156.0.8078.79. Testing uses only synthesized PDFs, new temporary SQLite, dynamic loopback ports, and AnySeek protocol fixtures. The model is disabled. Testing of Codex/Claude Code product clients is suspended.
- `cargo test --locked --offline --test local run_pdf_source`: 1 passed. Verified page 2 of v1 using authorized origin and published fragment hashes. Still returns v1 origin bytes after v2 publication; returns page numbers and no spans when the same text is repeated. The origin can still be authorized to open. Rejection is applied according to the contract for duplicate acquisitions across Key, reader, unrelated evidence ID, requesting specified version, and source withdrawal. Overlapping public Services, upload/publish/search pipelines, and actual HTTP requests are also considered.
- `cargo test --locked --offline --lib pdf_page_span`: 1 passed. Overlapping UTF-8 multi-byte boundaries, fragment hashes, unknown parsers, duplicate matches, and pure grid pages without highlights. Non-ASCII fixtures without declared character encoding were not declared in the first round. The fixture with WinAnsi encoding was corrected and passed again. Existing `cargo test --locked --offline --no-default-features --test core pdf_`: 9 passed, including multilingual, pagination, corruption, operator truncation, file/page/total text limits, and rejection of partial publications.
- `npm --prefix ui test` passed 19 tests. Then `node ui/scripts/check-run-pdf-browser.mjs` passed from the repository root. Actual Chromium and the built UI/relay used synthetic HTTP responses to check the v1 in-page text mark, original bytes, actual new-tab Blob URLs for pages 2 and 3, fallback without highlighting and denial of a new open after revocation. No fallback to the latest asset version or raw file URL. The initial browser behavior assertions passed before a profile-directory cleanup race. Normal browser shutdown with cleanup retry made the complete run pass. `node ui/scripts/check-run-verification-browser.mjs` passed the existing Run verification browser regression.
- `cargo fmt --all -- --check`, `cargo clippy --locked --offline --all-targets --all-features -- -D warnings`, and `git diff --check` passed. Reused existing single-table queries without new SQL, dependencies, permissions, schema or migrations. New responses after revocation require fresh authorization and source checks. Already-open files follow existing download semantics.
- Highlights appear in verified text extracted from the relevant page, while the browser opens the original on that page. Coordinate overlays and OCR were not added. Browser behavior and live HTTP backend behavior were checked separately. Windows and complete #76 acceptance did not run. This slice was committed locally on a separate branch with no push or PR.

### #76 Combined PDF verification with the real service and Chromium

After synchronizing main `8cdf88f709d708b75689768e6caada45fde56155`, reverified at `47b73234de9d57bb3b996b1a244421a06276e4f2`. Rust, Cargo, test tools and CI are identical to the original PDF commit `a95b931642372211575f7a11c57668208ffe43c1`. Rebuilt the UI to include #98's page changes on main. This follow-up affects only the verification docs.

Live AnySeek was built with Rust 1.99.1 from the CI-pinned source `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`. AnySeek binary SHA256 `ef581925d9afa44ce7be3694144ef010354925304e63ca737fc7b33d1687cc3a` and Origence Rust 1.98.0 test binary SHA256 `e05e5f930d94bd003fa337d0ba8fe01c24375f504d8e61810496ce6eaf678f52` both matched before and after the run. Node 24.22.1, Chromium 156.0.8078.79, dynamic loopback ports and temporary synthetic data, with models disabled.

- The same Run `a1d2cca4-f58b-4bac-a378-cf036fa218db` pinned a v1 citation with real AnySeek, then published v2. Following normal workspace sign-in, the built UI used the relay and real authorized HTTP API to read v1 page 2. Highlighted extracted text was `NeedleUnique historical fixed PDF page`. The original file opened in the native viewer at page 2/2 with SHA256 `0604486708bf7f3959dcdd53b5af8d4817b32cd1b738beb06d9fb8461bc69fc7`, matching v1 bytes. No pixel overlay was added to the original PDF page.
- Duplicate snippets fell back to page 1 and the authorized original without highlighting. The existing rules rejected publication of a raster-only PDF, with authorized download still supported. A missing evidence ID returned 404, reader access to the original returned 403, and another admin reading the private Run or source got 404.
- After v1 revocation the browser still showed the old button. Clicking again returned a real 404 and removed the button and highlight. v2 remained current. Retained original-file hashes, a native viewer screenshot and HTTP statuses.
- Local formatting, Clippy, 1 PDF unit test, 1 Service/HTTP case, 9 no-default PDF core tests, and 19 UI tests passed. The synthetic Chromium regression and combined live service check above both passed.

## 2026-11-18 — #76 Run creation counts and trends (local)

Based on main `3c66b5b06fbf40cc22641b373f892da1b617aff0`. Overview counts creation for the current logical key using one server cutoff and one authorized read snapshot. Half-open rolling UTC windows return 24 one-hour buckets or 7 buckets of 24 hours, with 0 for empty buckets. The Run catalog still pages by UUID. Duplicate creation requests and evidence revocations do not change creation counts, and creation does not establish business outcomes.

SQLite adds one tenant/workspace/owner/created_at covering index. The existing exact schema admission rejects older directories without the index before writing. It does not add indexes or migrate them. The actual EXPLAIN in tests was `SEARCH oc_runs USING COVERING INDEX oc_runs_owner_created_at (tenant_id=? AND workspace_id=? AND owner_key_id=? AND created_at>? AND created_at<?)`. Each bucket runs one single-table scalar count, for at most 24 nonoverlapping range queries, without reading Run or event bodies.

- Linux / Rust 1.98.0. `cargo test --locked --no-fail-fast` passed 450 tests, excluding 8 engine-specific tests ignored by default. Reran `cargo test --locked --test local run_creation_ -- --nocapture` for query plans, fixed-time bucket boundaries and sums, idempotency, scope and logical-key isolation, read permissions, rotation, invalid window and range parameters, counts beyond the first page and empty counts. Existing Run verification and evidence revocation tests separately confirm that detail access is denied while the Run still appears in the catalog and creation counts.
- `cargo fmt --all -- --check` and `cargo clippy --locked --all-targets -- -D warnings` passed on full default and all-feature configurations, authority-only and core-only. In authority-only, `cargo test --locked --no-default-features --features sqlite --test local missing_run_creation_index_is_rejected_without_repair` verified old file bytes were preserved.
- Node 24.22.1. `npm --prefix ui run check` and `npm --prefix ui test` passed 19 tests covering both windows, independent listing, errors and retry. Existing Chromium route focus regressions passed.
- Chromium 156.0.8078.79 with actual Node relay, Origence/SQLite, and pinned AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`. Both windows correctly showed 30 Runs from temporary synthetic data while the directory first page had 25. Another administrator and reader both showed 0. Empty buckets, sums, UTC boundaries, and 320px layout passed. Browser runner uses `tools/anyseek_smoke.py` for an isolated engine.

Business success, end-to-end P95, tokens and costs remained uncollected. This slice provides no alerts, production capacity acceptance, Windows execution evidence or actual client verification. Local commits only.

Before draft publication, verified schema rejection both ways. Built Origence from baseline `3c66b5b` and indexed `a43c270`, created one temporary database in each with `--offline workspace-create`, then ran the same open operation from the other binary. The new version rejected the store without the index, and the old one rejected the store with the new index. Both opens returned incompatible schema and kept the SQLite bytes unchanged. Implementation and tests stayed identical to `a43c270`. Draft preparation only added compatibility boundaries to the docs.

## 2026-11-18 — #76 Keyboard navigation for asset detail tabs (local)

Based on `origin/main` at `3c66b5b`. Arrow keys and Home/End move tab focus. Enter, Space or clicking activates the tab. Tab leaves the group for the current panel, and subsequent Tab navigation reaches version comparison. Readers can view body text and still compare versions. Original sources are fetched only on activation. Leaving the source tab or asset route cancels pending source requests. The comparison has its own region, showing only the latest request. Existing APIs and role boundaries are retained.

- Linux, Node 24.22.1. `npm --prefix ui run check` and `npm --prefix ui test` (19 tests) passed.
- Headless Chromium 156.0.8078.79, Playwright 1.62.1, the built UI, and the actual Node relay used a synthetic loopback HTTP fixture. All 16 combinations passed at 1024/736/375/320 px, light and dark, writer and reader. Verified Tab and Shift+Tab, wrapping with left and right arrow keys, focus outline, activation by keyboard and click, the tab/panel relationship in the accessibility tree, and no horizontal overflow. Readers sent 0 requests for original files. Delayed original and historical-version responses confirmed cancellation when returning to the body, comparing again quickly or switching to another pinned version. Interleaved original and comparison responses rendered independently, and the comparison preserved pinned versions, additions and deletions.
- #98 route and dialog browser regressions at 1024/320 px, the existing run verification script and PDF source script all passed.
- Rust 1.98.0 `cargo fmt --all -- --check`, `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` and `git diff --check` passed.

Rerun from the repository root. Install Playwright in a temporary directory and adjust the Chromium path for the host.

```sh
browser_deps=$(mktemp -d)
npm install --prefix "$browser_deps" --cache "$browser_deps/npm-cache" --ignore-scripts playwright@1.62.1
npm --prefix ui ci --cache "$browser_deps/npm-cache" --ignore-scripts
npm --prefix ui run check
npm --prefix ui test
NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
  node ui/scripts/check-asset-tabs-browser.cjs
```

Browser evidence covers keyboard, mouse and Chromium's accessibility tree. Screen readers, physical touchscreens, real business accounts and Codex/Claude Code product-client tasks were not used. #76 stays open.

### Recheck before merge

Synchronized main `63bdcdf4691c9ba761e4581b4aa62e94c89e94f1` with #100 and retained both verification entries above. Existing asset page and script changes stayed the same. Reran UI type checks, 19 tests, 16 Chromium asset-tab checks, #98 at two widths, and the existing Run verification and PDF source browser scripts. All passed. Rust 1.98.0 formatting, offline all-targets/all-features Clippy (warnings denied) and diff checks passed.

## 2026-11-18 — #76 Asset version comparison status and text contrast (local)

Based on `origin/main` at `6dcd03336c1d0605032a9083e9a4a2a02a972f1c`. Version comparison has a persistent `role=status`, `aria-atomic=true` node mounted in advance. Loading and completion notices share the same polite status region. Diff text displays separately. When the current request fails, loading text clears and an error appears in the results. Replacing the request or leaving details cancels it. Stale successes and errors never update the status or diff.

Chromium 156.0.8078.79 composites foreground and background using calculated styles and computes contrast from sRGB relative luminance. Thresholds use unrounded values. In light mode the only color changes are the three detail elements below, with foreground moving from `#137f6d` to `#106f60`. Dark values, theme colors and background definitions stay at baseline.

| Element | Light background | Light before | Light after | Dark |
| --- | --- | ---: | ---: | ---: |
| Selected body / original-source tab | `#f1f5f6` | 4.4656:1 | 5.5219:1 | 7.0595:1 |
| Added diff text | `#eef6f5` | 4.4666:1 | 5.5232:1 | 7.2363:1 |
| Positive version status badge | `#eaf3f2` | 4.3417:1 | 5.3687:1 | 6.9543:1 |

Light theme loading, completion, and difference summary reach 4.7429:1, and error/delete text reach 5.0615:1. Node 24.22.1 and Playwright 1.62.1 reran the same synthetic loopback fixture with the command above. All 16 combinations passed. The status node exists before request. Loading and completion share one Chromium AX node, with completion separate from differences. Tested delayed 503 and retry, same-version repeats, stale responses after switching versions, and comparison button focus. Writer tab keyboard navigation and original-text request behavior passed. Reader made 0 original-text requests.

UI type checking, 19 tests, #98's two widths, Run verification and PDF source browser regressions, Rust 1.98.0 formatting and offline all-targets/all-features Clippy passed. Chromium covers accessibility semantics and focus. Actual screen-reader announcements require a screen reader that can be observed, and no physical touchscreen was authorized for the test. Both remain pending. #76 stays open.

## 2026-11-18 · Data foundation documentation alignment

Used main `4160fd5` to cross-check product docs, public APIs and implementation for memory, knowledge, Skill and Trace. Changes cover documentation and the Cargo package description.

- Windows / Rust 1.98.0 `cargo +1.98.0 fmt --all -- --check` and `cargo +1.98.0 clippy --locked --offline --all-targets --all-features -- -D warnings` passed.
- `cargo +1.98.0 test --locked --offline --no-fail-fast --target-dir target/doc-positioning-check --quiet` passed, with 449 passed, 0 failed and 8 tests requiring an isolated real AnySeek ignored by default. The first attempt used the default build directory, where a running `origence.exe` locked the target file and caused Windows error 5. The rerun passed in a separate build directory.
- Checked 125 local links and their anchor targets across 12 active Markdown files. No repository link referenced a removed anchor. `git diff --check` passed.

This round covers documentation consistency and the Rust default suite. Actual dual-client, two-scenario pilot and independent sign-off remain tracked in [#91](https://github.com/TheLudlows/Origence/issues/91).

## 2026-11-18 · Remove the AML protocol adapter

Based on `4160fd5`, removed the protocol module, HTTP routes, startup switches, and UUIDv5 and `sha1_smol` used only for protocol-derived IDs. Message batches, owning workspace and original-source locations still use generic interfaces. SQLite's existing full schema validation rejects extra objects. Reviewed the executed single-table catalog queries and test DDL/DML. No association queries were added. Existing-store rejection tests cover both check and reopen and compare bytes before and after rejection.

- Windows / Rust 1.98.0 `cargo +1.98.0 fmt --all -- --check` and `cargo +1.98.0 clippy --locked --offline --all-targets --all-features -- -D warnings` passed.
- `cargo +1.98.0 test --locked --offline --no-fail-fast --target-dir target/doc-positioning-check --quiet` passed 448 tests, with 8 live AnySeek tests ignored by default.
- `cargo +1.98.0 test --locked --offline --no-default-features --target-dir target/doc-positioning-check --quiet` passed 90 tests, with 5 ignored by default.
- `cargo +1.98.0 test --locked --offline --no-default-features --features sqlite --test local --target-dir target/doc-positioning-check --quiet workspace_schema_rejection` passed 6 tests.
- In a fresh temporary directory, ran `tools/anyseek_smoke.py --test-executable` against pinned AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`. All 8 live engine checks passed. The real HTTP host verified old routes return 404, the CLI rejects old switches, and normal publication and full-text search pass.
- `docker compose --env-file .env.example config --quiet` passed. The machine had only Docker Desktop's WSL distribution. A Linux Docker engine was unavailable and Linux tests were not run in this check. Existing CI still covers Linux and Windows.

## 2026-11-18 · CI deduplication

Preserved all four jobs and existing test, Clippy and live-engine commands. `fast-check` owns the shared format check. Windows CI skips duplicate formatting, while local Windows `Validate` still checks formatting by default. `linux-native` reuses the host built by default-feature integration tests and checks that it is executable before smoke tests.

- Windows / Rust 1.98.0 `cargo +1.98.0 fmt --all -- --check` passed.
- In fresh `target/ci-binary-check` set `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` and `CARGO_INCREMENTAL=0`, then ran `cargo +1.98.0 test --locked --offline -j 2 --no-fail-fast --quiet`. 448 tests passed and 8 live AnySeek tests stayed ignored by default. There was no separate `cargo build`. `serve --help` verified the integration-test `debug/origence.exe`.
- Local Windows PowerShell 5.1 `powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File tools/build.ps1 -Action Validate -Jobs 1 -SkipFormatCheck` passed. Clippy passed. 448 default tests passed and 8 stayed ignored. Workflow Linux and GitHub runner evidence remained pending CI.

## 2026-11-18 — #76 Run creation trend retry focus (local)

Baseline `origin/main` was `e024b78eb2ecbf1aa1851783c89be8238ca819a1` with #98–102. Before the fix, at 1024 px in real Chromium, Tab moved from “Statistics range” to Retry after the first 503. Enter removed the button immediately and focus fell to `body`. After a second 503 another button appeared and focus remained at `body`. Refocusing it and pressing Space on a successful retry still moved focus to `body`.

After the fix, the initial failure still shows an error and retry button. While the request is pending the same button keeps focus and its visible outline. `aria-disabled` indicates that retry cannot be triggered again yet. Failure reenables the button. Success hides it and moves focus to “Statistics range” in the same region only if that button still held focus. Focus moved away with Shift+Tab remains where the user left it.

- Linux, Node 24.22.1, Playwright 1.62.1, and Chromium 156.0.8078.79. `ui/scripts/check-navigation-focus-browser.cjs` passed at 1024/320 px. The built UI and Node relay used a synthetic loopback HTTP fixture. It controlled response order and checked the initial failure, keyboard retry, repeated input while waiting, a second failure, two focus paths on success, and the existing #98 route and dialog focus behavior. All 16 asset-tab browser regressions passed.
- `npm --prefix ui run check`, `npm --prefix ui test` (19 tests), `git diff --check`, and Rust 1.98.0 `cargo fmt --all -- --check` and `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` passed.

Rerun from the repository root with the UI build and Playwright installation steps above.

```sh
NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
  node ui/scripts/check-navigation-focus-browser.cjs
```

### Actual screen-reader and physical touchscreen acceptance pending

Start the same fixture in manual mode from the repository root. Use the printed local address and synthetic key `synthetic-key`. The service listens only on loopback. Restart the fixture before each acceptance run. HTTP checks confirmed five manual-mode responses in order, 503, 503, 200, 503, 200, each delayed about 1.2 seconds.

```sh
npm --prefix ui run build
ORIGENCE_FIXTURE_PORT=43176 node ui/scripts/check-navigation-focus-browser.cjs --serve
```

1. Connect to the fixture in a screen-reader session that can record actual announcements. Record the initial failure announcement, then Tab to “Retry creation counts” and press Enter. Record loading, repeated failure and button focus. Retry with Space, record the success announcement and confirm focus moves to “Statistics range”. Then switch to “Last 7 days” and move focus away before the next retry completes. Confirm focus stays there after success. Record the device, screen reader, browser version and actual announcements.
2. On an authorized physical touchscreen that can reach the fixture through the existing local connection, rerun failure, retry, repeated failure and recovery. Use actual taps and verify readable status during the wait, no duplicate action on repeated taps, reachable results after recovery, and working scrolling. Record the device, browser, viewport and gesture results.

This round did not run screen-reader software or a physical touchscreen. The formal pilot, live models and product-client Codex/Claude Code tests remained paused. #76 stayed open.

## 2026-11-18 — #76 Run detail collection declarations (local)

Based on `origin/main` at `e024b78eb2ecbf1aa1851783c89be8238ca819a1`. The baseline showed `coverage` only in the selected event's raw JSON. Run details now use structured events from existing Trace pages to display sequence, component, declared status and reporting source in evidence order. Later reports for the same component preserve prior gaps. Declarations are unverified and overall Run coverage remains unknown. If more events exist, the view explicitly limits its claims to loaded events. When a successful read contains no declaration, the view says “No declaration in the loaded events. Coverage unknown.”

Existing routes, authorization, and raw event details are retained. Read failures such as 403 clear old declarations and show the original error. Retry fetches the first event page. Switching Runs and 401 logout clear private views using the existing route and credential lifecycles.

- Linux, Node 24.22.1, `npm --prefix ui run check` and `npm --prefix ui test` (20 tests) passed. Synthetic HTTP regressions cover ordering of repeated components, four declaration states, escaping untrusted text, pagination scope, invalidation and retry after 403, clearing old content while waiting for another Run, and clearing private views on 401.
- Chromium 156.0.8078.79 and Playwright 1.62.1 used the built UI, actual Node relay and isolated loopback fixture. At 1024 and 320 px the browser checked sign-in, pagination, 403, retry, switching Runs and 401 logout. #98 route focus and 16 asset-tab regressions passed. The old Run verification CDP script occasionally produced `ERR_ABORTED`, also reproducible on main with the original script. The separate Playwright check below verified declaration behavior.
- Rust 1.98.0 `cargo fmt --all -- --check`, `cargo clippy --locked --offline --all-targets --all-features -- -D warnings`, and `git diff --check` all passed.

Rerun from the repository root using the UI build and Playwright installation steps above.

```sh
NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
  node ui/scripts/check-run-coverage-browser.cjs
```

This round used synthetic data for Chromium and DOM verification. Actual screen-reader and physical touchscreen checks remain pending. Live clients, models and the formal pilot remained paused. #76 stayed open.

### #76 Selected event and pinned Run prefix digest (Chromium)

Based on main `56682df8613f8cd19c91ef2f7751eee931600f14`. On baseline, reading the pinned prefix of event 1 then switching to event 2 could show event 1's late digest in event 2's details. The fix binds each request to the selected event and cancels it when selection changes. Responses must match Run ID, event sequence and SHA-256 digest format. Reread clears the old result. Authorization denials or mismatches never leave the old digest visible. Server ownership, permission checks, the pinned-prefix digest algorithm and interpretation of client reports are unchanged.

- Extended the existing Run Chromium script. The actual UI and proxy accessed a synthetic loopback HTTP fixture. The baseline reproduced late attribution to the wrong event. Both 1024/320 px passed on Chromium 153.0.8010.0 after the fix. Coverage included late success and 403 for an old selection, clearing old values while rereading, active 403, incorrect response sequence, retained button focus, deduplication and successful retry.
- Node 24.22.3 type checks and 21 UI tests, existing pinned-version PDF Chromium checks, and `git diff --check` passed. Live Agent clients, screen-reader speech, physical touch, models and the pilot were not run.

### #76 Run event pagination and recovery of external verification

Based on main `2fcde29eabdf5a5ed67c56ad5fd7a5d0baf31501`. On baseline, loading page three reset the selection from page two to event 1. Authorized retry after 403 did not restore external verification reads. The fix appends events without reconstructing the selected detail view and preserves the pinned prefix and focus. The final page moves focus to new events only if focus is still on “Load more events”. Pagination failures clear old evidence and cancel in-flight verification. An authorized retry fetches page one again. Server permissions, digest rules and the evidence scope of external verification are unchanged.

- Node 24.22.3 `npm test` (21 tests) and Chromium 153.0.8010.0 `ui/scripts/check-run-coverage-browser.cjs` (1024/320 px) passed. The same new regression reproduced both baseline failures, missing verification after retry and selection reset on page three.
- Fixtures checked clearing after 403, cancellation of old verification requests so a late 401 cannot log out the retried session, logout on a current 401, and preserving focus and the pinned prefix. All cases were synthetic. Live clients, screen readers, touchscreens and the pilot were not run.

## 2026-11-19 — #76 Main navigation keyboard regressions at intermediate widths

Based on `origin/main` at `5c3a423f013fec0141fb8278612fa305e3805612`. Historical layout checks and asset tabs already had evidence at 736/375 px. The reproducible main navigation and focus script previously covered 1024/320 px. Added four cases for 736/375 px in light and dark themes to the existing script.

- From “Run Overview”, navigate in both directions with Shift+Tab and Tab through all five navigation entries and adjacent logout/theme controls. Check focus order, `focus-visible`, outlines, and controls staying in the viewport. Press Enter on “Context Workbench”. Verify focus moves to the main region, only one `aria-current` remains, and the next Tab enters “Task and query”.
- Measure real elements on the overview and workbench. At 736 px the main navigation is beside the workspace. At 375 px it is above the workspace. Neither page has whole-page horizontal overflow.
- Linux and Node 24.22.1, with Playwright 1.62.1 driving actual headless Chromium 156.0.8078.79. The built UI and Node relay used a synthetic loopback HTTP fixture. The four new cases passed twice consecutively. Original 1024/320 px route, dialog and overview retry focus regressions passed in the same run.
- UI type checks, 20 UI tests, Rust 1.98.0 formatting, strict all-targets/all-features Clippy and `git diff --check` passed. All four [mainline CI jobs](https://github.com/TheLudlows/Origence/actions/runs/44441569348) passed for the baseline. Run the Chromium script separately with the command below.

Rerun from the repository root using the UI build and Playwright installation steps above.

```sh
NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
  node ui/scripts/check-navigation-focus-browser.cjs
```

This round verified keyboard interaction and browser layout. Screen-reader software and physical touchscreens still need acceptance. Live clients, models and the formal pilot were not run. #76 remains open.

During #110 review, strengthened two rendering assertions. At each width, compare the workbench's computed background in both color schemes, and check at every keyboard focus that the outline width is above zero and the outline lies within the viewport. The delayed knowledge catalog fixture keeps the 4-second request deadline and fails explicitly if the request is absent. The old script passed when temporarily forced to light mode and stripped of the focus outline. The new script detects both regressions and a missing knowledge catalog request. All four actual Chromium scenarios without injection, and the original 1024/320 px regressions, passed in two consecutive runs. UI type checks and 20 UI tests passed.

GitHub CI's `node-ui` job runs `npm test`, which builds the UI and runs Node tests. The Chromium focus script runs separately locally using the command above. The `node-ui` results cover only the build and Node tests it ran.

## 2026-11-19 — #76 Pinned context knowledge versions and source byte verification

Based on main `1c5e00297b5074a420acfcf70a84db103ab38a86`. In baseline Chromium, context workbench links could open specific asset versions, but the original text of ordinary Text/Markdown knowledge evidence could not be highlighted. Verification uses existing HTTP permissions for source events and original files. Highlighting requires a `source-ranges-v1` UTF-8 byte slice that matches the evidence text. If a range splits a character or the quotation differs, the UI shows a notice. Asset links continue to read the version in the reference.

- Linux, Node 24.22.1, Playwright 1.62.1 and headless Chromium 156.0.8078.79. The built UI used its Node relay against a loopback synthetic HTTP fixture. Main timed out waiting for inline source highlighting. After the fix, both runs passed. Tests opened v1 while v2 was current, and checked nonzero byte ranges for inline and uploaded sources, Chinese characters, emoji, combining characters, UTF-8 BOM, both cases that must decline highlighting, and that a reader did not request the original source.
- UI type checks, 20 UI tests, strict all-targets/all-features Clippy and Rust formatting passed. Existing synthetic Chromium regressions for asset tabs and private Run PDFs passed. This knowledge source verification covers Text/Markdown. See the original Run verification record for in-page PDF locations.
- The same Chromium release, built Node UI, same source built with Rust 1.98.0, and CI-pinned AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a` (Rust 1.99.1) also passed together. Inline Text and uploaded Markdown each used fresh SQLite/AnySeek data, a temporary workspace and a synthetic document. After the browser previewed v1, v2 was published through the actual API. The v1 evidence came from actual `/v1/context`, with a nonzero range beginning at byte 2400. Highlighting exactly matched Chinese characters, emoji and combining characters. The evidence link opened v1 and the body omitted v2 content. HTTP was not intercepted. The role and invalid-range assertions come from the synthetic HTTP regression above.

To rerun browser verification, use the UI build and Playwright installation steps above from the repository root.

```sh
NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
  node ui/scripts/check-context-knowledge-browser.cjs
```

Screen-reader software and physical touchscreens were not tested. Live product clients, models and the formal pilot remained paused.

### #76 Original citation verification status and focus (Chromium)

Based on main `2ad69260522676ecf70f9cd86fc3e3e47d453c8a`. For text source verification, fetch, location, unavailable highlight and failure states enter a short status region mounted in advance. The original document and location details remain ordinary page content. Refetch clears the previous result. Stale responses cannot replace the current result. Focus stays on the verify button.

- Chromium 153.0.8010.0 used the built UI, same-origin relay and loopback synthetic sources. The extended context knowledge script could not find the pre-activation status region on baseline and passed after the fix. The Chromium AX tree confirmed the `status` node, `polite` and `atomic` attributes, updates to the same node after activation, retry after 403, and focus. A new authorization denial during download clears the old source preview. Both unverifiable UTF-8 cases remain unhighlighted, and the reader does not request the original file.
- Node 24.22.3 type checks and 21 tests passed. Built-browser regressions for asset detail tabs, contrast and private Run PDFs passed. Pinned-version handling, permissions and in-page PDF verification contracts were unchanged.
- This captures browser accessibility tree evidence. It does not establish screen-reader behavior or audio announcements. The cloud environment had no Orca or speech dispatcher, and it could not open the local D-Bus socket. Physical touchscreen and actual screen-reader checks remain pending.

## 2026-11-19 — #76 Refresh and retry ingestion jobs (local)

Based on main `df330cf`. Manual refresh cancels a previous pending status request. Stale responses cannot update the page or decide when polling stops. Explicit retry after a failure resumes polling for the same job. Repeated clicks during an in-flight operation do not send another retry. The server determines the current version. Closing the dialog leaves the background job running.

- With Node 24.22.3, `npm --prefix ui run check`, `npm --prefix ui test` (21 tests), and `git diff --check` passed.
- With Chromium 153.0.8010.0, build and run `CHROMIUM=<浏览器路径> node ui/scripts/check-knowledge-job-browser.cjs` (requires Playwright). The actual UI relay connects to a loopback HTTP fixture. A failure still shows v1. After retry, automatic polling shows publication complete and opens v2. A deliberately delayed old processing request is canceled and does not overwrite completion. Escape returns focus to the trigger, and forward/backward navigation does not reopen the old dialog. Both baseline regressions failed on unpatched main.

These local synthetic HTTP/browser results are separate from the actual service checks above. Production, live clients, physical touchscreen and screen-reader acceptance remain outside their scope. Server APIs, storage, permissions and deployment remained unchanged.

## 2026-11-19 — #76 Traceable input tokens for the context query embedding

Based on main `df330cfdf389eaefec463d745e0aeca779eaf4fe`. The existing context preparation retrieval diagnostics record provider-reported query embedding input tokens and the configuration digest. No model calls, tables or pricing logic were added. The UI shows only a single preparation event. Successful replay preserves the original receipt and pinned prefix. Standalone preview does not write to the Run.

- Rust 1.98.0 passed 3 retrieval diagnostics integration tests, 2 diagnostic unit tests, formatting and strict all-targets/all-features Clippy. A loopback model protocol fixture and real local SQLite verify attribution and spoofing protection, one shared call, replay with no request, correct attribution of rejection or failure, previews with no Run, explicit 0, omitted or invalid counts, and serialization of old evidence.
- Node 24.22.3 type checks and 20 UI tests passed. DOM assertions preserve explicit 0 in server events, leave absent or null counts unknown, and do not display client reports as model server receipts. Existing Run capture and navigation regressions passed in Chromium 153.0.8010.0 against the built UI and loopback fixtures, at widths of 1024/320px and navigation widths of 736/375px.
- Live models and product clients were not run. End-to-end or Agent token counts, costs, business success rates, end-to-end P95, alerts and the formal pilot were not assessed. These counts do not calculate charges or populate overview metrics.

## 2026-11-22 — #76 Pinned-version links for archived client citations

Based on main `66a1720624af64ff939a958a7e893cbcaee09a94`. Client-reported non-PDF citations from `context_reported` were previously shown as JSON only. An archived reference returned by Run reads now opens the pinned version. Unarchived client claims do not create links. Access is rechecked by the existing asset read endpoint, with no fallback to the current version and no added raw source entry point. The “client-reported, not independently verified” label and PDF verification behavior are preserved.

With Node 24.22.3, the new DOM regression failed on unpatched code because the pinned link was absent. After the fix `npm --prefix ui run check`, `npm --prefix ui test` (22 tests) and `git diff --check` passed. Fixtures open v1 when v2 is current, omit a link when the reference is unarchived, and after revocation deny v1 without displaying either body. The run used synthetic HTTP and DOM checks. It did not involve live clients, models, a formal pilot or production deployment.

## 2026-11-23 — #91 Context replay for public reimbursement samples

Based on main `41cb852b468d1eba2574f27e6f355687043c9a70`, using a fresh local Origence database and pinned AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`. Before the fix, initial preparation succeeded and repeated replay of the same event ID reproduced HTTP 503. Decoding original snapshot bytes read search score `1.9766286611557007` as `1.976628661155701`, changing the serialization digest. Enable round-trip float parsing in `serde_json` and verify with the existing digest rules. The new regression covers snapshot serialization and parsing of this score.

- The public seed generated 16 cases and 32 observation rows. The 8 reimbursement prompts are separate from the policies and answers.
- The real AnySeek/HTTP/MCP paths published 16 versions of 8 policies. For every case, retrieval found v2, exact v1/v2 could be read, Run citations and identical request replay matched, and another logical key was denied access to the Run. v2 remained readable after v1 revocation. After revoking v2, the version and original preparation receipt were both inaccessible.
- With Rust 1.98.0, 231 library tests and formatting passed. The repository's live AnySeek HTTP/MCP smoke and restart checks passed. With Python 3.12.14, 58 pilot tests and 42 evaluation tests passed. The public P6 evidence index check returned `INDEX_LINKAGE_ONLY`.

This was a public synthetic engineering preflight. No live clients or models were called, and the formal pilot and independent sign-off remain `NOT_RUN`.

## 2026-11-23 — Origence MCP plugin installation and retrieval

Based on main `8226b4c1492c2a8f480977f10fbddc7379aaca18`. The real Claude Code
**2.1.330** and Codex CLI **0.171.0** installed from a local marketplace on Linux.
Codex CLI **0.167.0-alpha.4** also passed. Origence was built with Rust 1.98.0 and
published a synthetic item with embedded AnySeek before issuing a `reader` key.

- Claude Code used `claude plugin install origence --marketplace <REPO>`.
  Codex used `codex plugin marketplace add <REPO>` and
  `codex plugin add origence@origence-tools`. Both used fresh projects and
  isolated settings. The binary directory contained spaces and the host used a
  dynamic loopback port.
- Both called `context_session`, `context_search`, and `context_get`. The session
  matched the reader key's binding and `/v1/whoami` confirmed the workspace.
  Search found published v1 and the exact hit ID and version returned the expected
  content. Codex used its real app-server and Claude Code used a synthetic local
  Messages response to drive its real tool calls. Uninstall passed for both.
- The packages contain no Hook definitions and the private Run list stayed empty.
  A missing executable was detected by PATH lookup. Direct MCP launch diagnostics
  distinguished missing and invalid keys and an unreachable API. The fixture
  checked that output did not contain the test keys.
- `cargo build --locked --offline --bin origence`, formatting, strict
  all-targets/all-features Clippy, and `git diff --check` passed.

For a native Windows rerun, install the CLI executables and run from PowerShell:

```powershell
cargo build --locked --bin origence
python tools/plugin_smoke.py --binary .\target\debug\origence.exe --claude (Get-Command claude.exe).Source --codex (Get-Command codex.exe).Source
```

Native Windows execution is pending. The available executor ran Linux; remote
marketplace fetching, desktop GUI, real model providers, and the formal pilot
were outside this validation.

## 2026-11-23 — Separate embedding and extraction model configuration

Based on main `9cc1e929bc0c657f8b9e7408461eda25374ac2e6`. Each model route can
use its own URL and API key. Models on the shared URL retain legacy configuration
behavior. Dedicated URLs receive only their own keys. The embedding profile
still uses the original embedding URL, model name, and dimension. The extraction
job fingerprint uses the selected extraction endpoint.

- Linux with Rust 1.98.0 passed all-features `cargo check`, formatting, strict
  all-targets/all-features Clippy, and `git diff --check`. The 4 `models::tests`
  passed against two loopback HTTP fixtures. They verify request routing and
  credentials, preserve the legacy profile, and preserve the embedding profile
  when only the extraction URL changes.
- All 8 `semantic_capture::` tests, 21 `learning_loop::` tests, and
  `local_host_api_cli_mcp_models_and_recovery` passed. The last test injected
  invalid dedicated URLs and synthetic keys into the parent environment and
  verified that the child used the fixture's legacy settings.
- Docker Compose v2.40.3 `config` confirmed that the fixture `.env` passes both
  URLs and keys to the host service. Model calls used synthetic loopback fixtures.
  Live providers and container deployment were outside this validation.
