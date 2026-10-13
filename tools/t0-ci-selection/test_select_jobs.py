import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import select_jobs as s


REPOSITORY = "example/probes"
BEFORE = "a" * 40
AFTER = "b" * 40
BASE_WORKFLOW = """name: T0 probes
on:
  pull_request:
    branches: [main]
    paths:
      - 'tools/t0-turso-probe/**'
      - 'tools/t0-ann-probe/**'
permissions:
  contents: read
concurrency:
  group: t0-storage-${{ github.head_ref || github.ref_name }}
  cancel-in-progress: true
jobs:
  turso-compatibility:
    runs-on: ubuntu-24.04
    timeout-minutes: 90
    steps:
      - run: python3 tools/t0-turso-probe/verify.py
  ann-compatibility:
    runs-on: ubuntu-24.04
    timeout-minutes: 180
    env:
      RUSTUP_TOOLCHAIN: 1.98.0
    steps:
      - uses: Swatinem/rust-cache@v2
        with:
          save-if: false
      - name: ANN build
        timeout-minutes: 45
        run: cargo build --locked --release
"""


def wired(source=BASE_WORKFLOW):
    source = source.replace("      - 'tools/t0-ann-probe/**'\n", "      - 'tools/t0-ann-probe/**'\n" + s.HELPER_TRIGGER)
    source = source.replace("jobs:\n", "jobs:\n" + s.SELECT_JOB, 1)
    for name in ("turso", "ann"):
        source = source.replace(f"  {name}-compatibility:\n", f"  {name}-compatibility:\n" + s.CONTROL[name], 1)
    return source + s.REPORT_JOB


def event(before=BEFORE, after=AFTER):
    repo = {"full_name": REPOSITORY}
    return {
        "action": "synchronize", "before": before, "after": after,
        "repository": repo,
        "pull_request": {
            "head": {"sha": after, "repo": repo},
            "base": {"ref": "main", "repo": repo},
        },
    }


class SelectorTests(unittest.TestCase):
    def assert_both(self, result):
        self.assertTrue(result["run_turso"], result)
        self.assertTrue(result["run_ann"], result)

    def fake_git(self, paths, old=BASE_WORKFLOW, new=None):
        def run(*args):
            if args[:1] == ("rev-parse",):
                return AFTER.encode() + b"\n"
            if args[:1] == ("cat-file",):
                return b"commit\n"
            if args[:1] == ("diff",):
                self.assertIn("--no-renames", args)
                self.assertEqual(args[-3:], (BEFORE, AFTER, "--"))
                return b"".join(p.encode() + b"\0" for p in paths)
            if args == ("show", f"{BEFORE}:{s.WORKFLOW}"):
                return old.encode()
            if args == ("show", f"{AFTER}:{s.WORKFLOW}"):
                return (wired() if new is None else new).encode()
            raise AssertionError(args)
        return run

    def test_checked_in_workflow_uses_exact_templates_and_fail_safe_guards(self):
        workflow = Path(__file__).resolve().parents[2] / s.WORKFLOW
        _, jobs = s.workflow_blocks(workflow.read_text())
        self.assertEqual(jobs["select-probes"], s.SELECT_JOB)
        self.assertEqual(jobs["probe-results"], s.REPORT_JOB)
        for name in ("turso", "ann"):
            self.assertTrue(jobs[name + "-compatibility"].startswith(f"  {name}-compatibility:\n" + s.CONTROL[name]))
            self.assertIn("needs.select-probes.result != 'success'", s.CONTROL[name])
            self.assertIn(f"outputs.run_{name} != 'false'", s.CONTROL[name])

    def test_probe_specific_changes(self):
        for paths, expected in (
            (["tools/t0-turso-probe/staged.py"], (True, False)),
            (["tools/t0-turso-probe/Cargo.lock"], (True, False)),
            (["tools/t0-ann-probe/Cargo.lock"], (False, True)),
            (["tools/t0-ann-probe/src/main.rs", "tools/t0-turso-probe/verify.py"], (True, True)),
        ):
            with self.subTest(paths=paths):
                result = s.select(event(), "pull_request", REPOSITORY, self.fake_git(paths))
                self.assertEqual((result["run_turso"], result["run_ann"]), expected)
                self.assertEqual((result["before"], result["after"]), (BEFORE, AFTER))

    def test_unknown_shared_and_dependency_context_runs_both(self):
        for path in ("Cargo.lock", "Cargo.toml", "rust-toolchain.toml", ".cargo/config.toml", "docs/unknown-contract.md", "docs/evidence/other.json", "tools/t0-ann-probe-extra/a", "unknown"):
            with self.subTest(path=path):
                self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git([path])))

    def test_fts_with_explicit_docs_and_historical_evidence_skips_ann(self):
        docs = sorted(s.DOCUMENTATION) + ["docs/evidence/2026-10-13-t0-fts-staged-abc123.json"]
        paths = docs + [s.WORKFLOW, "tools/t0-turso-probe/staged_fts.py"]
        result = s.select(event(), "pull_request", REPOSITORY, self.fake_git(paths))
        self.assertTrue(result["run_turso"], result)
        self.assertFalse(result["run_ann"], result)
        # An executable ANN contract is never mistaken for documentation.
        paths.append("tools/t0-ann-probe/policy-contract.json")
        self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git(paths)))

    def test_doc_only_change_reports_neither_probe_run(self):
        result = s.select(event(), "pull_request", REPOSITORY, self.fake_git(sorted(s.DOCUMENTATION)))
        self.assertFalse(result["run_turso"], result)
        self.assertFalse(result["run_ann"], result)
        self.assertEqual(result["reason"], "documentation_only")
        self.assertIn("Documentation edits are not native execution evidence", s.summary(result))

    def test_initial_selector_and_fts_workflow_change_keeps_ann_unselected(self):
        paths = [s.WORKFLOW, "tools/t0-ci-selection/select_jobs.py", "tools/t0-ci-selection/test_select_jobs.py", "tools/t0-turso-probe/staged.py"]
        new = wired().replace("python3 tools/t0-turso-probe/verify.py", "timeout --signal=KILL 900s python3 tools/t0-turso-probe/staged.py")
        result = s.select(event(), "pull_request", REPOSITORY, self.fake_git(paths, new=new))
        self.assertTrue(result["run_turso"])
        self.assertFalse(result["run_ann"])

    def test_subsequent_fts_workflow_change_keeps_ann_unselected(self):
        old = wired()
        new = old.replace("python3 tools/t0-turso-probe/verify.py", "python3 tools/t0-turso-probe/staged.py")
        result = s.select(event(), "pull_request", REPOSITORY, self.fake_git([s.WORKFLOW], old, new))
        self.assertFalse(result["run_ann"])

    def test_every_ann_execution_and_global_change_selects_safely(self):
        replacements = (
            ("cargo build --locked --release", "cargo build --release"),
            ("RUSTUP_TOOLCHAIN: 1.98.0", "RUSTUP_TOOLCHAIN: 1.99.0"),
            ("timeout-minutes: 45", "timeout-minutes: 46"),
            ("timeout-minutes: 180", "timeout-minutes: 181"),
            ("save-if: false", "save-if: true"),
            ("Swatinem/rust-cache@v2", "Swatinem/rust-cache@v3"),
            ("contents: read", "contents: write"),
            ("cancel-in-progress: true", "cancel-in-progress: false"),
            ("name: T0 probes", "name: Changed T0 probes"),
            ("jobs:\n", "env:\n  CARGO_HOME: other\njobs:\n"),
            ("  ann-compatibility:\n", "env:\n  CARGO_HOME: hidden-root-setting\n  ann-compatibility:\n"),
            ("      - name: ANN build", "      - run: echo new step\n      - name: ANN build"),
            (s.CONTROL["ann"], "    if: true\n"),
            ("  ann-compatibility:\n" + s.CONTROL["ann"] + "    runs-on: ubuntu-24.04", "  ann-compatibility:\n" + s.CONTROL["ann"] + "    runs-on: ubuntu-latest"),
            ("run: python3 tools/t0-ci-selection/select_jobs.py", "run: echo false"),
        )
        for old_text, new_text in replacements:
            with self.subTest(change=old_text):
                candidate = wired().replace(old_text, new_text)
                self.assertNotEqual(candidate, wired())
                result = s.select(event(), "pull_request", REPOSITORY, self.fake_git([s.WORKFLOW], new=candidate))
                global_or_wiring = old_text in {
                    "contents: read", "cancel-in-progress: true", "name: T0 probes", "jobs:\n",
                    "  ann-compatibility:\n", s.CONTROL["ann"],
                    "run: python3 tools/t0-ci-selection/select_jobs.py",
                }
                self.assertTrue(result["run_ann"], result)
                self.assertEqual(result["run_turso"], global_or_wiring, result)

    def test_ann_only_workflow_and_source_upgrade_skips_unchanged_turso(self):
        old = wired()
        new = old.replace("timeout-minutes: 180", "timeout-minutes: 160").replace(
            "save-if: false", "save-if: true"
        ).replace("name: ANN build", "name: Lance 0.40 ANN and FTS build").replace(
            "cargo build --locked --release", "cargo build --locked --release -j 2\n      - run: python3 tools/t0-ann-probe/fts.py"
        )
        paths = [s.WORKFLOW, "tools/t0-ann-probe/Cargo.toml", "tools/t0-ann-probe/Cargo.lock",
                 "tools/t0-ann-probe/fts.py", "tools/t0-ci-selection/select_jobs.py",
                 "docs/VALIDATION.md", "docs/T0_STORAGE_CONTRACT.md", "docs/t0-lance-040-report.md"]
        result = s.select(event(), "pull_request", REPOSITORY, self.fake_git(paths, old, new))
        self.assertFalse(result["run_turso"], result)
        self.assertTrue(result["run_ann"], result)
        self.assertEqual(s.workflow_probe_changes(old, new), (False, True))
        self.assertIn("Turso: SKIPPED / NOT_RUN", s.summary(result))
        # Changing Turso's actual source must still run Turso even with its
        # workflow block preserved byte-for-byte.
        paths.append("tools/t0-turso-probe/verify.py")
        self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git(paths, old, new)))

    def test_symmetric_workflow_only_changes_and_selection_only_edits(self):
        old = wired()
        for new, expected in (
            (old.replace("tools/t0-turso-probe/verify.py", "tools/t0-turso-probe/staged.py"), (True, False)),
            (old.replace("name: ANN build", "name: ANN and FTS build"), (False, True)),
            (old.replace("tools/t0-turso-probe/verify.py", "tools/t0-turso-probe/staged.py").replace("name: ANN build", "name: ANN and FTS build"), (True, True)),
            (old, (False, False)),
        ):
            with self.subTest(expected=expected):
                result = s.select(event(), "pull_request", REPOSITORY, self.fake_git([s.WORKFLOW, "tools/t0-ci-selection/select_jobs.py"], old, new))
                self.assertEqual((result["run_turso"], result["run_ann"]), expected)
        result = s.select(event(), "pull_request", REPOSITORY, self.fake_git(["tools/t0-ci-selection/test_select_jobs.py"], old, old))
        self.assertFalse(result["run_turso"])
        self.assertFalse(result["run_ann"])
        self.assertEqual(result["reason"], "selection_only")

    def test_ann_only_upgrade_falls_back_for_global_alias_and_fetch_failures(self):
        old = wired()
        new = old.replace("name: ANN build", "name: Lance 0.40 build")
        paths = [s.WORKFLOW, "tools/t0-ann-probe/Cargo.toml"]
        for changed in (
            new.replace("contents: read", "contents: write"),
            new.replace("    timeout-minutes: 180", "    env: &shared {INPUT: changed}\n    timeout-minutes: 180"),
            new.replace("run: python3 tools/t0-ci-selection/select_jobs.py", "run: echo modified selector"),
        ):
            self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git(paths, old, changed)))
        def unavailable(*args):
            if args[0] in ("cat-file", "fetch"):
                raise subprocess.CalledProcessError(1, ["git", *args])
            return self.fake_git(paths, old, new)(*args)
        self.assert_both(s.select(event(), "pull_request", REPOSITORY, unavailable))

    def test_new_lance_report_is_exact_documentation_allowlist_entry(self):
        result = s.select(event(), "pull_request", REPOSITORY, self.fake_git(["docs/t0-lance-040-report.md"]))
        self.assertFalse(result["run_turso"])
        self.assertFalse(result["run_ann"])
        self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git(["docs/t0-lance-040-other-report.md"])))

    def test_cross_job_yaml_anchor_cannot_hide_ann_environment_change(self):
        old = wired().replace(
            "    timeout-minutes: 90\n",
            "    timeout-minutes: 90\n    env: &shared_env\n      SHARED_INPUT: before\n", 1,
        ).replace("    env:\n      RUSTUP_TOOLCHAIN: 1.98.0\n", "    env: *shared_env\n")
        new = old.replace("SHARED_INPUT: before", "SHARED_INPUT: after")
        self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git([s.WORKFLOW], old, new)))
        with self.assertRaises(s.UnknownContext):
            s.workflow_keeps_ann_inputs(old, new)

    def test_yaml_alias_tag_merge_and_directive_layouts_are_rejected(self):
        for value in ("&anchor value", "*anchor", "!!str value", "!custom value", "!<tag:example.test,2026:custom> value", "! value", "{key: &shared value}"):
            with self.subTest(value=value):
                old = wired().replace("    timeout-minutes: 90\n", "    timeout-minutes: 90\n    env: " + value + "\n", 1)
                new = old.replace("tools/t0-turso-probe/verify.py", "tools/t0-turso-probe/staged.py")
                self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git([s.WORKFLOW], old, new)))
        for insertion in ("    <<: {env: {X: value}}\n", "%TAG !e! tag:example.test,2026:\n"):
            old = wired().replace("    timeout-minutes: 90\n", "    timeout-minutes: 90\n" + insertion, 1)
            self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git([s.WORKFLOW], old, old)))

    def test_flow_mapping_alias_without_colon_space_is_rejected(self):
        old = wired().replace(
            "    timeout-minutes: 90\n", '    timeout-minutes: 90\n    env: {"SHARED_INPUT":&shared before}\n', 1,
        ).replace('    env:\n      RUSTUP_TOOLCHAIN: 1.98.0\n', '    env: {"SHARED_INPUT":*shared}\n')
        new = old.replace("&shared before", "&shared after")
        self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git([s.WORKFLOW], old, new)))
        with self.assertRaises(s.UnknownContext):
            s.workflow_keeps_ann_inputs(old, new)

    def test_step_environment_alias_after_run_block_is_not_shell_text(self):
        old = wired().replace(
            "      - run: python3 tools/t0-turso-probe/verify.py\n",
            "      - run: |\n          python3 tools/t0-turso-probe/verify.py\n        env: &shared_env\n          SHARED_INPUT: before\n", 1,
        ).replace(
            "      - uses: Swatinem/rust-cache@v2\n",
            "      - run: |\n          echo ANN setup\n        env: *shared_env\n      - uses: Swatinem/rust-cache@v2\n", 1,
        )
        new = old.replace("SHARED_INPUT: before", "SHARED_INPUT: after")
        self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git([s.WORKFLOW], old, new)))
        with self.assertRaises(s.UnknownContext):
            s.workflow_keeps_ann_inputs(old, new)

    def test_comments_cannot_disguise_aliases_as_scalar_contents(self):
        for comment in ("    # fake: |\n", "    name: value # fake: |\n"):
            source = "jobs:\n  turso-compatibility:\n    steps:\n" + comment + "      - &shared {run: echo before}\n  ann-compatibility:\n    steps:\n" + comment + "      - *shared\n"
            with self.subTest(comment=comment), self.assertRaises(s.UnknownContext):
                s.workflow_blocks(source)

    def test_shell_anchor_looking_tokens_are_conservatively_rejected(self):
        source = wired().replace("      - run: python3 tools/t0-turso-probe/verify.py\n", "      - run: |\n          echo &possibly_yaml_anchor\n")
        with self.assertRaises(s.UnknownContext):
            s.workflow_blocks(source)

    def test_shell_block_and_github_expression_operators_remain_supported(self):
        old = wired().replace("      - run: python3 tools/t0-turso-probe/verify.py\n", "      - run: |\n          if [ ! -f result ]; then\n            echo first && echo second\n          fi\n")
        new = old.replace("echo first", "echo changed")
        self.assertTrue(s.workflow_keeps_ann_inputs(old, new))

    def test_other_valid_yaml_job_heading_forms_cannot_hide_in_turso(self):
        for heading in ("other_job:", "Other:", '\"other\":', "'other':", "other: # comment", "other: ", "other: {runs-on: ubuntu-24.04, steps: []}"):
            with self.subTest(heading=heading):
                inserted = "  " + heading + "\n"
                if "{" not in heading:
                    inserted += "    runs-on: ubuntu-24.04\n    steps:\n      - run: echo unknown job\n"
                new = wired().replace("  ann-compatibility:\n", inserted + "  ann-compatibility:\n")
                self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git([s.WORKFLOW], wired(), new)))
                with self.assertRaises(s.UnknownContext):
                    s.workflow_keeps_ann_inputs(wired(), new)

    def test_unknown_extra_jobs_and_duplicate_trigger_are_conservative(self):
        for new in (wired() + "  mystery:\n    runs-on: ubuntu-24.04\n", wired().replace(s.HELPER_TRIGGER, s.HELPER_TRIGGER * 2)):
            self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git([s.WORKFLOW], new=new)))

    def test_missing_or_invalid_event_never_invokes_git(self):
        variants = []
        for field in ("before", "after", "pull_request", "repository"):
            e = event()
            del e[field]
            variants.append(e)
        for sha in ("0" * 40, "a" * 39, "A" * 40, "--upload-pack=evil", "a" * 40 + "\n", None):
            e = event()
            e["before"] = sha
            variants.append(e)
        for action in ("opened", "reopened", "edited"):
            e = event()
            e["action"] = action
            variants.append(e)
        e = event()
        e["pull_request"]["head"]["sha"] = "c" * 40
        variants.append(e)
        e = event()
        e["changes"] = {"base": {"ref": {"from": "other"}}}
        variants.append(e)
        e = copy.deepcopy(event())
        e["pull_request"]["head"]["repo"] = {"full_name": "fork/probes"}
        variants.append(e)
        variants += [event(AFTER, AFTER), None, [], {}]
        for e in variants:
            with self.subTest(event=e):
                def no_git(*args):
                    self.fail(f"Git should not run: {args}")
                self.assert_both(s.select(e, "pull_request", REPOSITORY, no_git))

    def test_wrong_event_or_base_context_runs_both(self):
        self.assert_both(s.select(event(), "pull_request_target", REPOSITORY))
        e = event()
        e["pull_request"]["base"]["ref"] = "other"
        self.assert_both(s.select(e, "pull_request", REPOSITORY))

    def test_empty_diff_bad_checkout_fetch_and_diff_failure_run_both(self):
        self.assert_both(s.select(event(), "pull_request", REPOSITORY, self.fake_git([])))
        for failing in ("rev-parse", "cat-file", "fetch", "diff", "show"):
            with self.subTest(command=failing):
                def run(*args):
                    if args[0] == failing or (failing == "fetch" and args[0] == "cat-file"):
                        raise subprocess.CalledProcessError(1, ["git", *args])
                    return self.fake_git([s.WORKFLOW])(*args)
                # A failed cat-file permits one fetch, which also fails safely
                # in this mock because no successful fetch is modeled.
                if failing == "cat-file":
                    def run(*args):
                        if args[0] in ("cat-file", "fetch"):
                            raise subprocess.CalledProcessError(1, ["git", *args])
                        return self.fake_git([s.WORKFLOW])(*args)
                self.assert_both(s.select(event(), "pull_request", REPOSITORY, run))
        def wrong_checkout(*args):
            return b"c" * 40 + b"\n"
        self.assert_both(s.select(event(), "pull_request", REPOSITORY, wrong_checkout))

    def test_timeout_corrupt_tree_and_non_commit_are_conservative(self):
        for error in (subprocess.TimeoutExpired("git", 90), OSError("unavailable"), UnicodeDecodeError("utf8", b"\xff", 0, 1, "bad")):
            def fail(*args):
                raise error
            self.assert_both(s.select(event(), "pull_request", REPOSITORY, fail))
        def non_commit(*args):
            return b"tag\n" if args[0] == "cat-file" else self.fake_git([s.WORKFLOW])(*args)
        self.assert_both(s.select(event(), "pull_request", REPOSITORY, non_commit))

    def test_executed_merge_ref_workflow_must_equal_head_before_skip(self):
        selected = s.select(event(), "pull_request", REPOSITORY, self.fake_git(["tools/t0-turso-probe/staged.py"]))
        merge = "d" * 40
        def context_git(*args):
            if args[0] == "cat-file":
                return b"commit\n"
            if args[0] == "show":
                return wired().encode()
            raise AssertionError(args)
        checked = s.check_execution_context(selected, merge, context_git)
        self.assertFalse(checked["run_ann"])
        self.assertEqual(checked["workflow_sha"], merge)
        for missing in (None, "", "bad", "0" * 40):
            self.assert_both(s.check_execution_context(selected, missing, context_git))
        def mismatched(*args):
            if args == ("show", f"{merge}:{s.WORKFLOW}"):
                return wired().replace("contents: read", "contents: write").encode()
            return context_git(*args)
        self.assert_both(s.check_execution_context(selected, merge, mismatched))
        def unavailable(*args):
            raise subprocess.CalledProcessError(1, ["git", *args])
        self.assert_both(s.check_execution_context(selected, merge, unavailable))

    def test_summary_does_not_turn_missing_or_failed_prior_evidence_into_pass(self):
        result = s.select(event(), "pull_request", REPOSITORY, self.fake_git(["tools/t0-turso-probe/staged.py"]))
        text = s.summary(result)
        self.assertIn("ANN: SKIPPED / NOT_RUN", text)
        self.assertIn("Skipped does not mean passed", text)
        self.assertIn("No earlier pass or failure is reused", text)
        self.assertIn(BEFORE, text)
        self.assertIn(AFTER, text)
        self.assertNotIn("pass", result)

    def test_cli_missing_event_outputs_both(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "output"
            summary = Path(tmp) / "summary"
            with patch.dict(os.environ, {"GITHUB_EVENT_PATH": str(Path(tmp) / "missing"), "GITHUB_EVENT_NAME": "pull_request", "GITHUB_REPOSITORY": REPOSITORY, "GITHUB_OUTPUT": str(output), "GITHUB_STEP_SUMMARY": str(summary)}):
                s.main()
            self.assertEqual(output.read_text(), "run_turso=true\nrun_ann=true\n")
            self.assertIn("selected to run", summary.read_text())


class GitFixtureTests(unittest.TestCase):
    def test_previous_pr_head_not_whole_pr_diff_and_cross_probe_rename(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            def git(*args):
                return subprocess.run(["git", "-C", tmp, *args], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout
            def commit(message):
                git("add", ".")
                git("-c", "user.name=CI fixture", "-c", "user.email=ci@example.invalid", "commit", "-qm", message)
                return git("rev-parse", "HEAD").decode().strip()
            git("init", "-q")
            workflow = root / s.WORKFLOW
            workflow.parent.mkdir(parents=True)
            workflow.write_text(wired())
            ann = root / "tools/t0-ann-probe/input"
            ann.parent.mkdir(parents=True)
            ann.write_text("base\n")
            base = commit("base")
            ann.write_text("earlier ANN change already in the PR\n")
            previous = commit("prior ANN work")
            fts = root / "tools/t0-turso-probe/input"
            fts.parent.mkdir(parents=True)
            fts.write_text("new FTS work\n")
            current = commit("pure FTS update")
            result = s.select(event(previous, current), "pull_request", REPOSITORY, git)
            self.assertFalse(result["run_ann"], result)
            self.assertEqual(result["paths"], ["tools/t0-turso-probe/input"])
            self.assertIn(b"tools/t0-ann-probe/input", git("diff", "--name-only", base, current))
            # Exercise the real CLI, including executed-workflow verification.
            event_file = root / "event.json"
            event_file.write_text(json.dumps(event(previous, current)))
            output = root / "job-output"
            summary = root / "job-summary"
            env = {**os.environ, "GITHUB_EVENT_PATH": str(event_file),
                   "GITHUB_EVENT_NAME": "pull_request", "GITHUB_REPOSITORY": REPOSITORY,
                   "GITHUB_WORKFLOW_SHA": current, "GITHUB_OUTPUT": str(output),
                   "GITHUB_STEP_SUMMARY": str(summary)}
            subprocess.run([sys.executable, str(Path(s.__file__).resolve())], cwd=root,
                           env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
            self.assertEqual(output.read_text(), "run_turso=true\nrun_ann=false\n")
            self.assertIn(current, summary.read_text())
            for temporary in (event_file, output, summary):
                temporary.unlink()
            # Inverse update: earlier Turso work remains in the PR, but only
            # ANN inputs/workflow plus the explicit report change now.
            ann.write_text("upgraded ANN and FTS inputs\n")
            workflow.write_text(wired().replace("name: ANN build", "name: Lance 0.40 ANN and FTS build"))
            report = root / "docs/t0-lance-040-report.md"
            report.parent.mkdir(parents=True)
            report.write_text("Native execution not run in this fixture.\n")
            ann_current = commit("ANN-only workflow and source update")
            result = s.select(event(current, ann_current), "pull_request", REPOSITORY, git)
            self.assertFalse(result["run_turso"], result)
            self.assertTrue(result["run_ann"], result)
            self.assertEqual(s.check_execution_context(result, ann_current, git)["run_turso"], False)
            self.assertIn(b"tools/t0-turso-probe/input", git("diff", "--name-only", base, ann_current))
            event_file.write_text(json.dumps(event(current, ann_current)))
            env["GITHUB_WORKFLOW_SHA"] = ann_current
            subprocess.run([sys.executable, str(Path(s.__file__).resolve())], cwd=root,
                           env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
            self.assertEqual(output.read_text(), "run_turso=false\nrun_ann=true\n")
            self.assertIn("Turso: SKIPPED / NOT_RUN", summary.read_text())
            for temporary in (event_file, output, summary):
                temporary.unlink()
            current = ann_current
            # Renames must expose deletion from ANN as well as addition to FTS.
            ann.rename(fts.parent / "moved-ann-input")
            renamed = commit("cross-probe move")
            result = s.select(event(current, renamed), "pull_request", REPOSITORY, git)
            self.assertTrue(result["run_ann"], result)
            self.assertTrue(result["run_turso"], result)
            # A nonexistent baseline and unavailable origin must run both.
            result = s.select(event("f" * 40, renamed), "pull_request", REPOSITORY, git)
            self.assertTrue(result["run_ann"], result)
            self.assertTrue(result["run_turso"], result)


if __name__ == "__main__":
    unittest.main()
