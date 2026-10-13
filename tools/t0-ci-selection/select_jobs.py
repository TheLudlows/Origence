#!/usr/bin/env python3
"""Conservative T0 job selection; unchanged means NOT_RUN, never PASS.

Only the runner's pull_request/synchronize event supplies the comparison heads.
See README.md for the official event references and limits of this optimization.
"""

import json
import os
from pathlib import Path
import re
import subprocess


WORKFLOW = ".github/workflows/t0-storage.yml"
# Explicit historical/specification outputs, not native probe inputs. The ANN
# source-baseline.json and policy-contract.json remain execution inputs.
DOCUMENTATION = frozenset({
    "docs/VALIDATION.md",
    "docs/T0_STORAGE_CONTRACT.md",
    "docs/t0-turso-report.md",
    "docs/t0-ann-report.md",
    "docs/t0-fts-cost-report.md",
    "docs/t0-staged-fts-report.md",
    "docs/t0-lance-040-report.md",
})
EVIDENCE_PATH = re.compile(
    r"docs/evidence/[0-9]{4}-[0-9]{2}-[0-9]{2}-t0-"
    r"(?:turso|fts|ann)-[a-z0-9][a-z0-9-]*\.json"
)
HELPER_TRIGGER = "      - 'tools/t0-ci-selection/**'\n"
CONTROL = {
    name: (
        "    needs: select-probes\n"
        "    if: ${{ !cancelled() && (needs.select-probes.result != 'success' || "
        "needs.select-probes.outputs.run_" + name + " != 'false') }}\n"
    )
    for name in ("turso", "ann")
}
SELECT_JOB = """  select-probes:
    runs-on: ubuntu-24.04
    timeout-minutes: 5
    outputs:
      run_turso: ${{ steps.select.outputs.run_turso }}
      run_ann: ${{ steps.select.outputs.run_ann }}
    steps:
      - uses: actions/checkout@v4
        with:
          ref: ${{ github.event.pull_request.head.sha }}
      - name: Test conservative job selection
        run: python3 -m unittest discover -s tools/t0-ci-selection -p 'test_*.py' -v
      - name: Select exact-head probes (unknown means both)
        id: select
        run: python3 tools/t0-ci-selection/select_jobs.py
"""
REPORT_JOB = """  probe-results:
    needs: [select-probes, turso-compatibility, ann-compatibility]
    if: always()
    runs-on: ubuntu-24.04
    timeout-minutes: 5
    steps:
      - name: Report actual job results without carrying forward passes
        env:
          SELECTION_RESULT: ${{ needs.select-probes.result }}
          TURSO_RESULT: ${{ needs.turso-compatibility.result }}
          ANN_RESULT: ${{ needs.ann-compatibility.result }}
        run: |
          {
            printf 'Selection job: %s\\n\\n' "$SELECTION_RESULT"
            printf 'Turso compatibility: %s\\n\\n' "$TURSO_RESULT"
            printf 'ANN compatibility: %s\\n\\n' "$ANN_RESULT"
            printf 'Skipped = NOT_RUN on this head, not passed. Previous failures remain historical failures; no result is reused.\\n'
          } >> "$GITHUB_STEP_SUMMARY"
"""


class UnknownContext(Exception):
    """Selection cannot safely prove that a probe is unaffected."""


def exact_sha(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value):
        raise UnknownContext("missing_or_invalid_sha")
    if value == "0" * 40:
        raise UnknownContext("zero_sha")
    return value


def event_heads(event, event_name, repository):
    if event_name != "pull_request" or event.get("action") != "synchronize":
        raise UnknownContext("not_pr_synchronize")
    if event.get("changes"):
        raise UnknownContext("event_context_changed")
    pr = event["pull_request"]
    if not repository or any(
        repo["full_name"] != repository
        for repo in (event["repository"], pr["base"]["repo"], pr["head"]["repo"])
    ):
        raise UnknownContext("repository_context_unknown_or_fork")
    if pr["base"]["ref"] != "main":
        raise UnknownContext("base_context_changed")
    before = exact_sha(event.get("before"))
    after = exact_sha(event.get("after"))
    if after != exact_sha(pr["head"]["sha"]):
        raise UnknownContext("after_does_not_match_pr_head")
    if before == after:
        raise UnknownContext("identical_heads")
    return before, after


def git(*args):
    # No shell interpolation, branch refs, payload URLs, or extra credentials.
    return subprocess.run(
        ["git", *args], check=True, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, timeout=90,
    ).stdout


def ensure_commit(sha, run=git):
    try:
        kind = run("cat-file", "-t", sha).strip()
    except subprocess.CalledProcessError:
        run("fetch", "--no-tags", "--depth=1", "origin", sha)
        kind = run("cat-file", "-t", sha).strip()
    if kind != b"commit":
        raise UnknownContext("baseline_is_not_a_commit")


def changed_paths(before, after, run=git):
    if run("rev-parse", "HEAD").decode("ascii").strip() != after:
        raise UnknownContext("checkout_does_not_match_pr_head")
    ensure_commit(after, run)
    ensure_commit(before, run)
    # Compare endpoint trees, not a merge-base/whole-PR diff. Disabling rename
    # detection exposes both deleted and added names across probe boundaries.
    raw = run("diff", "--no-ext-diff", "--no-renames", "--name-only", "-z", before, after, "--")
    if not raw or not raw.endswith(b"\0"):
        raise UnknownContext("empty_or_invalid_diff")
    paths = raw[:-1].decode("utf-8").split("\0")
    if any(not p or p.startswith("/") or any(x in ("", ".", "..") for x in p.split("/")) for p in paths):
        raise UnknownContext("invalid_changed_path")
    return sorted(set(paths))


def reject_yaml_indirection(source):
    """Reject unsupported YAML references; deliberately allow false positives.

    Do not attempt to parse block scalars. An anchor/alias-looking shell token
    also runs both jobs. Only single-line GitHub expressions, the exact shell
    && operator and standalone shell ! are exempted from token matching.
    """
    tokens = re.compile(
        r"(?:^|[\s\[\]{},:?-])"
        r"(?:&(?!(?:&)(?:\s|$))[^\s\[\]{},]+|\*[^\s\[\]{},]+|![^\s\[\]{},]+)"
    )
    for line in source.splitlines():
        plain = re.sub(r"\$\{\{[^\n]*?\}\}", "", line)
        if (tokens.search(plain)
                or re.match(r"^\s*(?:-\s+)?[A-Za-z_][A-Za-z0-9_-]*:\s*!(?:\s|$)", plain)
                or re.match(r"^\s*<<\s*:", plain)
                or plain.startswith("%")):
            raise UnknownContext("unsupported_yaml_indirection")


def workflow_blocks(source):
    """Accept only the simple, known top-level job layout used by this file."""
    reject_yaml_indirection(source)
    header, sep, body = source.partition("jobs:\n")
    if not sep or source.count("\njobs:\n") != 1:
        raise UnknownContext("unknown_workflow_layout")
    # A root-level key after jobs would leave the jobs mapping. Never let an
    # apparent probe-only block conceal a new global env/defaults/permissions.
    if any(line and not line.startswith(" ") for line in body.splitlines()):
        raise UnknownContext("unknown_workflow_root_key")
    # Do not silently absorb valid but unrecognized YAML job headings into
    # the preceding probe block (uppercase, underscores, quotes, inline maps).
    for line in body.splitlines():
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip(" "))
        if indent < 4 and (indent != 2 or not re.fullmatch(r"  [a-z][a-z0-9-]*:", line)):
            raise UnknownContext("unsupported_workflow_job_heading")
    matches = list(re.finditer(r"^  ([a-z][a-z0-9-]*):\n", body, re.MULTILINE))
    if not matches or matches[0].start() != 0:
        raise UnknownContext("unknown_workflow_jobs")
    jobs = {}
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(body)
        name = match.group(1)
        if name in jobs:
            raise UnknownContext("duplicate_workflow_job")
        jobs[name] = body[match.start():end]
    return header, jobs


def execution_block(block, name):
    heading = f"  {name}-compatibility:\n"
    if not block.startswith(heading):
        raise UnknownContext("unknown_probe_heading")
    body = block[len(heading):]
    if body.startswith(CONTROL[name]):
        body = body[len(CONTROL[name]):]
    return heading + body


def workflow_probe_changes(before_source, after_source):
    """Return (Turso changed, ANN changed), or None for unknown context.

    Each job's steps, env, runner, timeouts, actions and permissions are
    compared byte-for-byte. Only the exact declared selector controls are
    stripped; shared settings and known selector/report wiring must match.
    """
    old_header, old_jobs = workflow_blocks(before_source)
    new_header, new_jobs = workflow_blocks(after_source)
    if old_header.count(HELPER_TRIGGER) > 1 or new_header.count(HELPER_TRIGGER) > 1:
        return None
    if old_header.replace(HELPER_TRIGGER, "") != new_header.replace(HELPER_TRIGGER, ""):
        return None
    probes = {"turso-compatibility", "ann-compatibility"}
    selected = probes | {"select-probes", "probe-results"}
    if set(old_jobs) not in (probes, selected) or set(new_jobs) != selected:
        return None
    for name, template in (("select-probes", SELECT_JOB), ("probe-results", REPORT_JOB)):
        if new_jobs[name] != template or (name in old_jobs and old_jobs[name] != template):
            return None
    if not new_jobs["turso-compatibility"].startswith("  turso-compatibility:\n" + CONTROL["turso"]):
        return None
    if not new_jobs["ann-compatibility"].startswith("  ann-compatibility:\n" + CONTROL["ann"]):
        return None
    return tuple(
        execution_block(old_jobs[f"{name}-compatibility"], name)
        != execution_block(new_jobs[f"{name}-compatibility"], name)
        for name in ("turso", "ann")
    )


def workflow_keeps_ann_inputs(before_source, after_source):
    """Retain the original narrow comparison for callers and regressions."""
    changes = workflow_probe_changes(before_source, after_source)
    return changes is not None and not changes[1]


def classify(paths, workflow_changes=None):
    turso = ann = False
    selection_changed = False
    for path in paths:
        if path.startswith("tools/t0-turso-probe/"):
            turso = True
        elif path.startswith("tools/t0-ann-probe/"):
            ann = True
        elif path == WORKFLOW or path.startswith("tools/t0-ci-selection/"):
            selection_changed = True
        elif path in DOCUMENTATION or EVIDENCE_PATH.fullmatch(path):
            continue
        else:
            return True, True, "shared_or_unknown_input_changed"
    if selection_changed:
        if workflow_changes is None:
            return True, True, "workflow_or_selection_context_changed"
        turso = turso or workflow_changes[0]
        ann = ann or workflow_changes[1]
    if not paths:
        return True, True, "empty_diff"
    reason = "exact_head_inputs_selected" if turso or ann else (
        "selection_only" if selection_changed else "documentation_only"
    )
    return turso, ann, reason


def select(event, event_name, repository, run=git):
    result = {"run_turso": True, "run_ann": True, "reason": "unknown_context", "before": None, "after": None, "paths": []}
    try:
        before, after = event_heads(event, event_name, repository)
        result.update(before=before, after=after)
        paths = changed_paths(before, after, run)
        result["paths"] = paths
        workflow_changes = None
        if any(p == WORKFLOW or p.startswith("tools/t0-ci-selection/") for p in paths):
            old = run("show", f"{before}:{WORKFLOW}").decode("utf-8")
            new = run("show", f"{after}:{WORKFLOW}").decode("utf-8")
            workflow_changes = workflow_probe_changes(old, new)
        turso, ann, reason = classify(paths, workflow_changes)
        result.update(run_turso=turso, run_ann=ann, reason=reason)
    except UnknownContext as exc:
        result["reason"] = str(exc)
    except (KeyError, TypeError, AttributeError, ValueError, OSError, subprocess.SubprocessError):
        result["reason"] = "event_git_or_baseline_unavailable"
    return result


def check_execution_context(result, workflow_sha, run=git):
    """The compiled workflow may come from the PR merge ref, not head."""
    result = dict(result, workflow_sha=None)
    if result["run_turso"] and result["run_ann"]:
        return result
    try:
        actual = exact_sha(workflow_sha)
        result["workflow_sha"] = actual
        ensure_commit(actual, run)
        compiled = run("show", f"{actual}:{WORKFLOW}")
        head = run("show", f"{result['after']}:{WORKFLOW}")
        if compiled != head:
            raise UnknownContext("executed_workflow_differs_from_head")
    except (UnknownContext, OSError, subprocess.SubprocessError):
        result.update(run_turso=True, run_ann=True, reason="executed_workflow_context_unverified")
    return result


def summary(result):
    lines = ["T0 exact-head probe selection", "", f"Reason: {result['reason']}"]
    for key in ("before", "after"):
        lines.append(f"{key}: {result[key] or 'unavailable'}")
    lines.append(f"Executed workflow SHA: {result.get('workflow_sha') or 'unavailable / no skip requested'}")
    for key, label in (("run_turso", "Turso"), ("run_ann", "ANN")):
        lines.append(f"{label}: {'selected to run' if result[key] else 'SKIPPED / NOT_RUN (inputs unchanged)'}")
    lines += ["", "Selection is not a test result. Skipped does not mean passed. No earlier pass or failure is reused.", "Documentation edits are not native execution evidence.", ""]
    return "\n".join(lines)


def main():
    try:
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    except (KeyError, OSError, ValueError):
        event = {}
    result = select(event, os.environ.get("GITHUB_EVENT_NAME"), os.environ.get("GITHUB_REPOSITORY"))
    result = check_execution_context(result, os.environ.get("GITHUB_WORKFLOW_SHA"))
    # Missing output / write failure fails the selector job. Downstream guards
    # run BOTH whenever this job fails or its output is anything but 'false'.
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"run_turso={str(result['run_turso']).lower()}\nrun_ann={str(result['run_ann']).lower()}\n")
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as output:
        output.write(summary(result))
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
