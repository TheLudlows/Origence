"""Trace one synthetic expense task through native Codex, Origence and the real browser UI."""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import time
from urllib.error import HTTPError
import uuid

from live_archive_smoke import child_environment
from native_fixture import workspace as seed_workspace
from mcp_smoke import Host, isolated_env, require, sanitized
from plugin_smoke import codex_session, execute, tool_value

QUERY = "ExpensePilotFixture"
POLICIES = {1: 120, 2: 80, 3: 60}
REPORTED_AMOUNT = 120
BROWSER_CHECK = Path(__file__).resolve().parents[1] / "ui/scripts/check-run-trace-e2e.cjs"


def policy_text(version):
    return f"{QUERY} 报销上限 {POLICIES[version]} 元。"


def publish_policy(host, version, asset=None):
    payload = {"title": f"合成报销制度 v{version}", "content": policy_text(version), "format": "text"}
    if asset is not None:
        payload.update(asset_id=asset, expected_version=version - 1)
    receipt = host.request("/v1/knowledge", payload)
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        job = host.request("/v1/jobs/" + receipt["job_id"])
        if job["state"] == "completed":
            require(job["outcome"] == "published" and job["result"]["version"] == version,
                    "Policy publication returned the wrong version")
            return receipt
        require(job["state"] not in {"failed", "cancelled", "superseded"}, "Policy publication failed")
        time.sleep(0.1)
    raise RuntimeError("Policy publication timed out")


def report_from_codex(host, cli, project, owner, forbidden, asset):
    settings = project / "settings"
    settings.mkdir()
    env = dict(child_environment(), HOME=str(project), USERPROFILE=str(project),
               CODEX_HOME=str(settings), OC_API_KEY=owner)
    execute([cli, "mcp", "add", "origence", "--url", host.base + "/mcp",
             "--bearer-token-env-var", "OC_API_KEY"], project, env, forbidden)
    run = str(uuid.uuid4())
    with codex_session(cli, project, env, forbidden) as (rpc, thread):
        def call(name, arguments):
            return tool_value(rpc("mcpServer/tool/call", {
                "threadId": thread, "server": "origence", "tool": name, "arguments": arguments,
            }))

        call("context_run_create", {"run_id": run, "summary": "合成报销复核 Run"})
        preparation = call("context_run_prepare", {
            "run_id": run, "event_id": str(uuid.uuid4()),
            "context": {"query": QUERY, "mode": "keyword"},
        })
        require(any(item["provenance"].get("asset_id") == asset and
                    item["provenance"].get("asset_version") == 2
                    for item in preparation["bundle"]["items"]), "Codex preparation did not cite v2")
        call("context_run_report", {"run_id": run, "report": {
            "event_id": str(uuid.uuid4()), "reporter": "client", "observed_at_ms": 1,
            "event": {"kind": "result_reported", "outcome": "success",
                      "summary": f"合成客户端自报：建议报销 {REPORTED_AMOUNT} 元"},
        }})
        call("context_run_report", {"run_id": run, "report": {
            "event_id": str(uuid.uuid4()), "reporter": "client", "observed_at_ms": 2,
            "event": {"kind": "coverage", "component": "model_use", "status": "unknown",
                      "summary": "合成用例没有可核对的模型使用记录"},
        }})
    return run


def transcribe_fixture(host, path, asset):
    prefix = host.request(path + "/prefix?through_sequence=2")
    policy = host.request(f"/v1/assets/{asset}?version=2")
    require(policy["content"] == policy_text(2), "Fixture could not read the pinned verification basis")
    expected = min(REPORTED_AMOUNT, POLICIES[policy["version"]])
    require(expected < REPORTED_AMOUNT, "Fixture must model an incorrect client result")
    report = {"verification_id": str(uuid.uuid4()), "through_sequence": 2,
              "prefix_digest": prefix["digest"], "basis_sequences": [1, 2],
              "verifier_label": "合成制度核对", "assessed_at_ms": 3,
              "conclusion": "failed",
              "basis_summary": f"v2 允许 {expected} 元，客户端报告 {REPORTED_AMOUNT} 元。",
              "reported_skill_version": None}
    route = path + "/verifications"
    receipt = host.request(route, report)
    require(receipt == host.request(route, report), "Identical verification replay changed the receipt")
    try:
        host.request(route, dict(report, conclusion="passed"))
    except HTTPError as error:
        require(error.code == 409, "Conflicting verification replay returned the wrong status")
    else:
        raise RuntimeError("Conflicting verification replay changed the stored conclusion")
    require(receipt["platform_verification"] == "not_performed" and
            receipt["verifier_authentication"] == "unverified_claim", "Fixture lost attribution")
    return prefix["digest"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--codex", default="codex")
    args = parser.parse_args()
    binary = args.binary.resolve()
    with tempfile.TemporaryDirectory(prefix="origence-trace-") as temporary:
        root = Path(temporary)
        project = root / "project"
        project.mkdir()
        env = isolated_env(temporary)
        workspace = seed_workspace(env, "Synthetic trace acceptance", single_key=True)
        other = seed_workspace(env, "Isolated trace workspace", workspace["workspace_id"], single_key=True)
        owner = workspace["token"]
        forbidden = [owner, other["token"]]
        with Host(str(binary), env, owner, forbidden, serve_args=("--local",)) as host:
            first = publish_policy(host, 1)
            asset = first["asset_id"]
            second = publish_policy(host, 2, asset)
            run = report_from_codex(host, args.codex, project, owner, forbidden, asset)
            publish_policy(host, 3, asset)
            prefix = transcribe_fixture(host, f"/v1/runs/{run}", asset)
            data = {"api": host.base, "workspace": workspace["workspace_id"], "run": run,
                    "prefix": prefix, "asset": asset, "sourceV2": second["source_event_id"],
                    "textV2": policy_text(2), "owner": owner, "other": other["token"], "otherWorkspace": other["workspace_id"]}
            result = subprocess.run(["node", str(BROWSER_CHECK)], input=json.dumps(data),
                                    capture_output=True, text=True, encoding="utf-8", timeout=90)
            sanitized(result.stdout + result.stderr, forbidden)
            require(result.returncode == 0, (result.stderr or result.stdout)[-3000:])
            print(result.stdout.strip(), flush=True)


if __name__ == "__main__":
    main()
