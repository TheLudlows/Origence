"""Rehearse both client handoffs on public synthetic cases, with no formal scoring."""
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import re
import secrets
import subprocess
import tempfile
import time
from urllib.error import HTTPError

from live_skill_preflight import (Host, claude_turn, codex_turn, export, installed, isolated_env,
                                  main, publish_from_source, require, sha, uid)


CASES = {
    "database_wait": {
        "author": "claude-code", "consumer": "codex",
        "first": {"telemetry_complete": True, "blocker_session": 312, "lock_wait_ms": 8000,
                  "pool_active": 2, "pool_limit": 10, "connect_elapsed_ms": 100, "connect_timeout_ms": 1000},
        "second": {"telemetry_complete": True, "blocker_session": None, "lock_wait_ms": 0,
                   "pool_active": 12, "pool_limit": 12, "connect_elapsed_ms": 90, "connect_timeout_ms": 900},
        "instructions": "Classify this telemetry snapshot and design a reusable diagnostic table. "
                        "Set rules.missing_telemetry, identified_lock_blocker, pool_exhausted and "
                        "connect_deadline_exceeded to the appropriate literal class from "
                        "[pool, timeout, insufficient, lock]. Then separately map lock_check, "
                        "pool_check, timeout_check and insufficient_check to the appropriate literal read-only "
                        "check from [collect_telemetry, inspect_connect_latency, inspect_blocker, inspect_pool_metrics]. "
                        "For answer.evidence_field use one input key. For answer.next_check use a check identifier.",
        "output_format": "classification must be a class value, evidence_field must be one input key, "
                         "and next_check must be the matching literal *_check value from the table.",
        "fields": ["classification", "evidence_field", "next_check"],
        "first_expected": {"classification": "lock", "evidence_field": "blocker_session", "next_check": "inspect_blocker"},
        "second_expected": {"classification": "pool", "next_check": "inspect_pool_metrics"},
        "rules": {"missing_telemetry": "insufficient", "identified_lock_blocker": "lock",
                  "pool_exhausted": "pool", "connect_deadline_exceeded": "timeout",
                  "lock_check": "inspect_blocker", "pool_check": "inspect_pool_metrics",
                  "timeout_check": "inspect_connect_latency", "insufficient_check": "collect_telemetry"},
    },
    "expense_review": {
        "author": "codex", "consumer": "claude-code",
        "first": {"claim_minor": 120, "receipt_present": True, "duplicate": True, "currency": "TEST"},
        "second": {"claim_minor": 123, "receipt_present": True, "duplicate": False, "currency": "TEST"},
        "instructions": "The superseded v1 synthetic travel policy capped claims at 160. Current v2 caps "
                        "eligible claims at 97 minor TEST units. Duplicate or missing receipt means hold "
                        "and zero payable. Otherwise recommend the smaller of claim and cap. Return only "
                        "a recommendation. Decisions are hold or recommend.",
        "output_format": "decision is the applicable decision value (hold or recommend). "
                         "payable_minor and policy_version are JSON integers. For eligible claims apply the cap "
                         "to the claimed amount. Use exactly these three keys inside answer.",
        "fields": ["decision", "payable_minor", "policy_version"],
        "first_expected": {"decision": "hold", "payable_minor": 0, "policy_version": 2},
        "second_expected": {"decision": "recommend", "payable_minor": 97, "policy_version": 2},
        "rules": {"policy_version": 2, "eligible_cap_minor": 97, "duplicate_decision": "hold",
                  "missing_receipt_decision": "hold", "held_payable_minor": 0, "eligible_decision": "recommend"},
    },
}
TURNS = {"codex": codex_turn, "claude-code": claude_turn}


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)


def parse_answer(text):
    def reject_constant(_):
        raise ValueError("non-finite JSON value")

    def unique(pairs):
        obj = dict(pairs)
        require(len(obj) == len(pairs), "duplicate answer field")
        return obj

    # Never store an unvalidated provider response in the Run or in the report.
    text = text.strip()
    fenced = re.fullmatch(r"```(?:json)?[ \t]*\r?\n(.*?)\r?\n```", text, flags=re.IGNORECASE | re.DOTALL)
    if fenced:
        text = fenced.group(1)
    try:
        answer = json.loads(text, object_pairs_hook=unique, parse_constant=reject_constant)
    except ValueError:
        raise RuntimeError("model returned no final text" if not text else "model response was not a JSON object") from None
    require(isinstance(answer, dict), "model response was not an object")
    return answer


def mismatched_fields(actual, expected):
    if not isinstance(actual, dict):
        return ["object_required"]
    changed = [key for key in expected if key not in actual or canonical(actual[key]) != canonical(expected[key])]
    if set(actual) - set(expected):
        changed.append("extra_fields")
    return changed


def answer_differences(case, stage, answer):
    expected = case[stage + "_expected"]
    if "evidence_field" in case["fields"] and isinstance(answer, dict):
        fields = ("pool_active", "pool_limit") if stage == "second" else ("blocker_session", "lock_wait_ms")
        selected = answer.get("evidence_field")
        expected = dict(expected, evidence_field=selected if selected in fields else fields[0])
    return mismatched_fields(answer, expected)


def report_event(host, run, event):
    row = {"event_id": uid(), "reporter": "test", "observed_at_ms": int(time.time() * 1000), "event": event}
    receipt = host.request(f"/v1/runs/{run}/events", row)
    events = host.request(f"/v1/runs/{run}")["events"]
    require(events[-1]["report"] == row and events[-1]["sequence"] == receipt["sequence"]
            and events[-1]["evidence_origin"] == "reported", "Run readback mismatch")
    return host.request(f"/v1/runs/{run}/prefix?through_sequence={receipt['sequence']}")


def denied_private(host, path):
    try:
        host.request(path)
    except HTTPError as error:
        with error:
            require(error.code == 404, "unexpected private read response")
        return error.code
    raise RuntimeError("consumer could read author's private resource")


def rehearse(args, root, host, space, keys, case_name, case, record, deadline):
    author, consumer = case["author"], case["consumer"]
    host.token = keys[author]["token"]
    run = uid()
    host.request("/v1/runs", {"run_id": run, "summary": "Public first case " + case_name})
    project = root / "first" / author
    project.mkdir(parents=True)
    shape = {"answer": dict.fromkeys(case["fields"], None), "rules": dict.fromkeys(case["rules"], None)}
    prompt = (f"Public synthetic task, Run {run}. Use only the supplied data and no tools. "
              f"{case['instructions']} Input: {canonical(case['first'])}. Return one plain JSON object "
              f"in exactly this shape: {canonical(shape)}. Replace every null with a value. "
              "Use integer amounts and versions. Begin with { and end with }. No explanation or Markdown.")
    print(case_name, "starting first case", author, flush=True)
    native, raw = TURNS[author](args, project, run, prompt, deadline)
    record["first"] = {"client": author, "native": native, "input": case["first"],
                       "prompt_sha256": sha(prompt.encode()), "output_sha256": sha(raw.encode())}
    first = parse_answer(raw)
    require(set(first) == {"answer", "rules"} and not answer_differences(case, "first", first["answer"]),
            "public first case did not match the expected result")
    differences = mismatched_fields(first["rules"], case["rules"])
    require(not differences, "public decision table review failed: " + ", ".join(differences))
    record["first"].update(first)
    prefix = report_event(host, run, {
        "kind": "result_reported", "outcome": "success",
        "summary": f"Public first case. Prompt SHA256 {record['first']['prompt_sha256']}. "
                   f"Answer and decision table SHA256 {sha(canonical(first).encode())}.",
    })
    marker = secrets.token_hex(12)
    name = case_name.replace("_", "-") + "-" + secrets.token_hex(3)
    body = ("Use this decision table for the supplied case. "
            + canonical(first["rules"]) + ". Only recommend, using read-only checks. "
            + case["output_format"] + " "
            "Return one plain JSON object with keys answer and receipt. In answer use "
            + ", ".join(case["fields"]) + ". Copy this string to receipt: " + marker)
    package, approval = publish_from_source(host, space, keys["reviewer"], name, "Review public " + case_name,
                                            body, {"kind": "run", **prefix})
    record["approval"] = approval
    host.token = keys[consumer]["token"]
    record["private_run_http"] = denied_private(host, f"/v1/runs/{run}")
    record["private_candidate_http"] = denied_private(host, "/v1/skill-candidates/" + approval["candidate_id"])
    project = root / "second" / consumer
    project.parent.mkdir(parents=True)
    require(export(args.binary, host, host.token, project, consumer, package).returncode == 0, "peer export failed")
    path = installed(project, consumer, package)
    second_run = uid()
    host.request("/v1/runs", {"run_id": second_run, "summary": "Public second case " + case_name})
    prompt = (f"Public synthetic task, Run {second_run}. Load the selected project skill {name} using "
              "your native Skill mechanism. Apply it to this new case. Use no other tools. "
              f"Input: {canonical(case['second'])}. Return only the JSON specified by the skill, no Markdown.")
    print(case_name, "starting second case", consumer, flush=True)
    native, raw = TURNS[consumer](args, project, second_run, prompt, deadline, path, body)
    record["second"] = {"client": consumer, "native": native, "input": case["second"],
                        "prompt_sha256": sha(prompt.encode()), "output_sha256": sha(raw.encode())}
    answer = parse_answer(raw)
    loaded = (native["tool_result_success"] and native["input_body_observed"]
              if consumer == "claude-code" else native["model_request_body_verified"])
    require(loaded and set(answer) == {"answer", "receipt"} and answer["receipt"] == marker,
            "missing native loading or package receipt")
    differences = answer_differences(case, "second", answer["answer"])
    require(not differences, "public second result did not match: " + ", ".join(differences))
    record["second"].update(answer=answer["answer"], receipt_sha256=sha(marker.encode()))
    record["second"]["load_prefix"] = report_event(host, second_run, {
        "kind": "skill_loaded_reported", "skill_id": approval["skill_id"],
        "version": approval["version"], "digest": approval["package_digest"],
    })
    record["second"]["result_prefix"] = report_event(host, second_run, {
        "kind": "result_reported", "outcome": "success", "summary": "Public second answer SHA256 " + sha(raw.encode()),
    })
    record["status"] = "OBSERVED"


def run_scenario(args, name, case, record, deadline):
    with tempfile.TemporaryDirectory(prefix="oc-public-handoff-") as directory:
        root = Path(directory)
        env = isolated_env(directory)
        require("OC_ANYSEEK_API_KEY" in env, "run under tools/anyseek_smoke.py")
        boot = subprocess.run([args.binary, "--offline", "workspace-create", "public-handoff"],
                              cwd=root, env=env, capture_output=True, check=True, timeout=15)
        space = json.loads(boot.stdout)
        admin = space["token"]
        with Host(args.binary, env, admin, [admin, env["OC_ANYSEEK_API_KEY"]]) as host:
            keys = {}
            for client, role in (("codex", "writer"), ("claude-code", "writer"), ("reviewer", "reviewer")):
                keys[client] = host.request("/admin/keys", {"workspace_id": space["workspace_id"], "role": role})
                host.forbidden.append(keys[client]["token"])
            rehearse(args, root / name, host, space, keys, name, case, record, deadline)


def handoffs(args, report):
    report.pop("clients")
    report["scenarios"] = {name: {"status": "INCOMPLETE"} for name in CASES}
    deadline = time.monotonic() + 85
    with ThreadPoolExecutor(max_workers=2) as pool:
        jobs = {name: pool.submit(run_scenario, args, name, case, report["scenarios"][name], deadline)
                for name, case in CASES.items()}
        for name, job in jobs.items():
            record = report["scenarios"][name]
            try:
                job.result()
            except (RuntimeError, subprocess.SubprocessError, OSError) as error:
                record["reason"] = str(error) if isinstance(error, RuntimeError) else type(error).__name__
            print(name, record["status"], record.get("reason", ""), flush=True)
    require(all(row["status"] == "OBSERVED" for row in report["scenarios"].values()), "incomplete public handoff")


if __name__ == "__main__":
    main(handoffs, __doc__)
