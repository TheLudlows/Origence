# Public synthetic preflight

**English** · [简体中文](PREFLIGHT.md)

Run from the repository root. Use Python 3.11 or newer, two installed and authorized clients, a fresh test workspace, and a report file that does not yet exist. These commands call real models. Codex uses its existing OpenAI sign-in. Claude Code reads provider settings from the environment or optional `--claude-settings PATH`.

## Rerun native Skill loading

```sh
python tools/anyseek_smoke.py --anyseek-binary ANYSEEK_BINARY --run python evals/pilot/live_skill_preflight.py --binary ORIGENCE_BINARY --claude-model CLAUDE_MODEL --codex-model CODEX_MODEL --report NEW_REPORT.json
```

The clients share 100 seconds, each downloading and staging the same approved Skill in its test project and returning a random string from the package. The script checks native loading signals and Runs, then withdraws the support source and verifies that new downloads are refused and original files remain. Missing required checks produce a nonzero exit. [Test record](native-skill.windows.json)

## Rerun cross-client handoff in both scenarios

Replace the script above with `evals/pilot/public_handoff.py` and use a different report. The two directions run in parallel with independent new databases. Four sessions share 85 seconds. The first client submits a decision table and the test account reviews and publishes it. The second client uses separate credentials to load natively on a new task. Check its answer and package-random string, and test access to the first Run and private candidate. Raw transcripts are cleaned up with the temporary directory. The report retains identifiers, digests, and outcomes. [Test record](public-handoff.windows.json)

## Prerequisites for manual Windows service startup

Use a fresh `-DataDir` and workspace for this run. Initialize with the current version and then start `tools/start-dev.ps1`. The embedded AnySeek instance uses the same data root. Preserve existing databases. If an old database lacks `oc_run_verifications`, record the schema incompatibility and use a new database. Record the initialization binary hash, schema, and workspace UUID.

## Separate retrieval preflight (unscored)

The initial expense handoff task includes the policy. A separate retrieval preflight uses a task containing only claim facts and a policy identifier. An independent custodian preserves authorized knowledge assets v1/v2, originals, locators, hashes, and answers. Use an authorized fresh session to run actual search/get and record exact versions, preparation receipts, observable client receipt evidence, and output. Use an independent clean unauthorized session to check denial or invisibility without supplying old text or Skill copies. After withdrawing the v2 source, check fresh server fetch and residual local copies separately. [Existing v2 retrieval evidence](../../docs/VALIDATION.en.md)
