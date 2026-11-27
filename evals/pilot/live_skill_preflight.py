"""Unscored public Skill loading probe; run via tools/anyseek_smoke.py --run."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import secrets
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
from urllib.error import HTTPError
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools"))
from live_archive_smoke import CLAUDE_ENV, child_environment
from mcp_smoke import Host, isolated_env, require


def uid():
    return str(uuid.uuid4())


def sha(data):
    return hashlib.sha256(data).hexdigest()


def terminate(process):
    if process.poll() is not None:
        return
    if os.name == "nt":
        try:
            subprocess.run(["taskkill.exe", "/PID", str(process.pid), "/T", "/F"],
                           capture_output=True, timeout=5)
        finally:
            process.kill()
    else:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    process.wait(timeout=5)


def start(command, directory, env):
    options = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {"start_new_session": True}
    return subprocess.Popen(command, cwd=directory, env=env, stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, **options)


def publish(host, space, writer, reviewer, name, body):
    host.token = writer["token"]
    asset = host.publish("Public synthetic policy: cap an eligible claim at 100 TEST units.")
    version = host.request(f"/v1/assets/{asset}?version=1")
    source = {"kind": "asset", "asset_id": asset, "version": 1, "content_sha256": version["content_hash"]}
    package, approval = publish_from_source(
        host, space, reviewer, name, "Check a synthetic TEST-unit expense claim", body, source,
    )
    approval["source_event_id"] = version["source_event_id"]
    return package, version["source_event_id"], approval


def publish_from_source(host, space, reviewer, name, description, body, source):
    target = {key: space[key] for key in ("tenant_id", "workspace_id")}
    target.update(skill_id=uid(), version=1)
    candidate, review = uid(), uid()
    candidate_path = "/v1/skill-candidates/" + candidate
    saved = host.request(candidate_path, {"expected_revision": 0, "candidate": {
        "draft": {"name": name, "description": description, "body": body},
        "sources": [source], "private_notes": ""}}, method="PUT")
    packet = host.request(candidate_path + "/reviews/" + review, {
        "revision": saved["revision"], "reviewer_id": reviewer["key_id"],
        "basis": "Public synthetic loading probe", "target": target})
    host.token = reviewer["token"]
    review_path = "/v1/skill-reviews/" + review
    require(host.request(review_path)["packet"]["digest"] == packet["digest"], "review packet changed")
    host.request(review_path + "/decision", {"approve": True, "digest": packet["digest"]})
    package = host.request(review_path + "/publish", {"digest": packet["digest"]})
    require(all(package["manifest"][k] == v for k, v in target.items()), "published target mismatch")
    return package, {
        **target, "name": name, "candidate_id": candidate, "revision": saved["revision"], "review_id": review,
        "reviewer_key_id": reviewer["key_id"],
        "packet_digest": packet["digest"], "source": source, "package_digest": package["digest"],
        "file_sha256": package["manifest"]["artifact_sha256"]["SKILL.md"],
    }


def export(binary, host, key, project, client, package):
    manifest = package["manifest"]
    project.mkdir()
    result = subprocess.run([binary, "--server-url", host.base, "skill-export", "--client", client,
                             "--project", str(project), "--tenant-id", manifest["tenant_id"],
                             "--workspace-id", manifest["workspace_id"], manifest["skill_id"],
                             str(manifest["version"])], env=dict(child_environment(), OC_API_KEY=key),
                            capture_output=True, timeout=15)
    return result


def installed(project, client, package):
    layout = ".agents" if client == "codex" else ".claude"
    directory = project / layout / "skills" / package["manifest"]["name"]
    require(json.loads((directory / "origence-manifest.json").read_bytes()) == package["manifest"],
            "installed manifest mismatch")
    path = directory / "SKILL.md"
    require(sha(path.read_bytes()) == package["manifest"]["artifact_sha256"]["SKILL.md"],
            "installed file digest mismatch")
    return path


def claude_load(args, project, path, run, marker, body, deadline):
    prompt = (f"Use the Skill tool to load project skill {path.parent.name}. Public test, Run {run}. "
              "Claim 110 TEST units with receipt, no duplicate.")
    evidence, answer = claude_turn(args, project, run, prompt, deadline, path, body)
    return {**evidence, "answer_matches": answer.strip() == "100 " + marker}


def claude_turn(args, project, run, prompt, deadline, path=None, body=None):
    env = child_environment()
    provider = os.environ if not args.claude_settings else json.loads(
        args.claude_settings.read_text(encoding="utf-8-sig"))["env"]
    env.update({key: provider[key] for key in CLAUDE_ENV if provider.get(key)})
    require(env.get("ANTHROPIC_API_KEY") or env.get("ANTHROPIC_AUTH_TOKEN"), "Claude provider credentials required")
    env.update(CLAUDE_CONFIG_DIR=str(project.parent / "claude-settings"), DISABLE_TELEMETRY="1",
               CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1")
    session = uid()
    name = path.parent.name if path else None
    mcp = project.parent / "empty-mcp.json"
    mcp.write_text('{"mcpServers":{}}', encoding="utf-8")
    command = [args.claude, "-p", "--session-id", session, "--permission-mode", "dontAsk",
               "--setting-sources", "project", "--strict-mcp-config", "--mcp-config", str(mcp),
               "--model", args.claude_model, "--tools", "Skill" if path else "",
               "--output-format", "stream-json", "--verbose"]
    if path:
        command.extend(["--allowedTools", "Skill"])
    else:
        command.extend(["--effort", "low"])
    with start(command, project, env) as process:
        try:
            output, _ = process.communicate(prompt.encode(), timeout=max(0.1, deadline - time.monotonic()))
        finally:
            terminate(process)
    require(process.returncode == 0, "Claude Code failed")
    events = [json.loads(line) for line in output.splitlines() if line.strip()]
    relevant = [e for e in events if e.get("session_id") == session]
    transcript = []
    for file in Path(env["CLAUDE_CONFIG_DIR"]).rglob(session + ".jsonl"):
        for line in file.read_bytes().splitlines():
            event = json.loads(line)
            if event.get("sessionId") == session:
                transcript.append(event)
    tool_uses = [b for e in relevant + transcript if e.get("type") == "assistant"
                 for b in e["message"]["content"] if b.get("type") == "tool_use"]
    calls = [b for b in tool_uses if path and b.get("name") == "Skill"
             and b.get("input", {}).get("skill", "").split(":")[-1] == name]
    require(len(calls) == len(tool_uses), "Claude Code used an unrelated tool")
    call_ids = {b["id"] for b in calls}
    blocks = []
    for event in relevant + transcript:
        if event.get("type") == "user":
            content = event["message"]["content"]
            blocks.extend([{"type": "text", "text": content}] if isinstance(content, str) else content)
    completed = any(b.get("type") == "tool_result" and b.get("tool_use_id") in call_ids
                    and not b.get("is_error") for b in blocks)
    last = next((e.get("result", "") for e in reversed(relevant)
                 if e.get("type") == "result" and not e.get("is_error")), "")
    injected = bool(path) and any(body in b.get("text", "") and name in b["text"] for b in blocks)
    init = next((e for e in relevant if e.get("subtype") == "init"), {})
    return {
        "session_id": session, "run_id": run, "trigger": "model Skill tool" if path else "plain task",
        "reported_model": init.get("model"),
        "skill_in_init": name in init.get("skills", []) or name in init.get("slash_commands", []),
        "tool_call_ids": sorted(call_ids), "tool_result_success": completed,
        "input_body_observed": injected, "model_request_body_verified": None,
        "event_stream_sha256": sha(output),
    }, last


def codex_request_matches(request, name, body, run):
    """Require the Run prompt and injected skill in the same provider request."""
    if not isinstance(request, dict) or not isinstance(request.get("input"), list):
        return False
    texts = []
    for item in request["input"]:
        if item.get("type") == "message" and item.get("role") in ("developer", "user"):
            texts.extend(block["text"] for block in item.get("content", [])
                         if isinstance(block, dict) and isinstance(block.get("text"), str))
    return any(body in text and name in text for text in texts) and any(run in text for text in texts)


def codex_load(args, project, path, run, marker, body, deadline):
    prompt = (f"Public test, Run {run}. Claim 110 TEST units with receipt, no duplicate. "
              "Follow the selected skill. Use no other tools.")
    evidence, answer = codex_turn(args, project, run, prompt, deadline, path, body)
    return {**evidence, "answer_matches": answer.strip() == "100 " + marker}


def codex_turn(args, project, run, prompt, deadline, path=None, body=None):
    trace_root = project.parent / "native-trace"
    env = child_environment()
    env.update({k: os.environ[k] for k in ("OPENAI_API_KEY", "CODEX_HOME") if k in os.environ})
    env["CODEX_ROLLOUT_TRACE_ROOT"] = str(trace_root)
    command = [args.codex, "app-server", "--stdio", "-c", 'model_provider="openai"', "-c", "analytics.enabled=false",
               "-c", 'model_reasoning_effort="low"', "-c", 'approval_policy="never"']
    if args.codex_credential_store:
        command.extend(["-c", "cli_auth_credentials_store=" + json.dumps(args.codex_credential_store)])
    with start(command, project, env) as process:
        lines = queue.Queue()

        def read():
            for raw in process.stdout:
                lines.put(raw)
            lines.put(None)

        reader = threading.Thread(target=read, daemon=True)
        reader.start()

        def send(method, params, seq=None):
            msg = {"method": method, "params": params}
            if seq is not None:
                msg["id"] = seq
            process.stdin.write(json.dumps(msg).encode() + b"\n")
            process.stdin.flush()

        def receive():
            try:
                line = lines.get(timeout=max(0.01, deadline - time.monotonic()))
            except queue.Empty:
                raise RuntimeError("Codex session timed out") from None
            require(line is not None and time.monotonic() < deadline, "Codex session ended or timed out")
            return json.loads(line)

        def rpc(method, params, seq):
            send(method, params, seq)
            while True:
                msg = receive()
                if msg.get("id") == seq:
                    require("result" in msg, "Codex rejected " + method)
                    return msg["result"]

        try:
            rpc("initialize", {"clientInfo": {"name": "origence-pilot", "version": "1"},
                               "capabilities": {"experimentalApi": True}}, 1)
            send("initialized", {})
            inputs = [{"type": "text", "text": prompt}]
            if path:
                name = path.parent.name
                listing = rpc("skills/list", {"cwds": [str(project)], "forceReload": True}, 2)
                matches = [s for entry in listing["data"] for s in entry["skills"] if s["name"] == name
                           and Path(s["path"]).samefile(path) and s.get("enabled", True)]
                require(len(matches) == 1, "Codex did not discover the exported project Skill")
                inputs.append({"type": "skill", "name": name, "path": str(path)})
            thread = rpc("thread/start", {
                "cwd": str(project), "model": args.codex_model, "approvalPolicy": "never",
                "sandbox": "read-only", "ephemeral": True,
            }, 3)["thread"]["id"]
            turn = rpc("turn/start", {"threadId": thread, "input": inputs}, 4)["turn"]["id"]
            last = ""
            while True:
                msg = receive()
                params = msg.get("params", {})
                if params.get("threadId") != thread:
                    continue
                if msg.get("method") == "item/completed" and params.get("turnId") == turn:
                    item = params["item"]
                    require(item["type"] in ("userMessage", "reasoning", "agentMessage", "plan"),
                            "Codex performed an unexpected action")
                    if item["type"] == "agentMessage":
                        last = item["text"]
                if msg.get("method") == "turn/completed" and params["turn"]["id"] == turn:
                    require(params["turn"]["status"] == "completed", "Codex turn failed")
                    break
        finally:
            terminate(process)
            reader.join(timeout=3)
    digests = []
    for file in trace_root.rglob("*.json"):
        data = file.read_bytes()
        request = json.loads(data)
        if path and codex_request_matches(request, name, body, run):
            digests.append(sha(data))
    return {"session_id": thread, "turn_id": turn, "run_id": run,
            "trigger": "explicit native Skill input" if path else "plain task",
            "native_path_discovered": bool(path), "model_request_body_verified": bool(digests) if path else None,
            "request_sha256": digests}, last


def probe(args, report):
    with tempfile.TemporaryDirectory(prefix="oc-pilot-skill-") as tmp:
        root = Path(tmp)
        env = isolated_env(tmp)
        require("OC_ANYSEEK_API_KEY" in env, "run under tools/anyseek_smoke.py")
        boot = subprocess.run([args.binary, "--offline", "workspace-create", "skill-preflight"],
                              cwd=root, env=env, capture_output=True, check=True, timeout=15)
        space = json.loads(boot.stdout)
        admin = space["token"]
        with Host(args.binary, env, admin, [admin, env["OC_ANYSEEK_API_KEY"]]) as host:
            def issue(role):
                key = host.request("/admin/keys", {"workspace_id": space["workspace_id"], "role": role})
                host.forbidden.append(key["token"])
                return key

            writer, reviewer = issue("writer"), issue("writer")
            marker = secrets.token_hex(12)
            name = "expense-check-" + secrets.token_hex(3)
            body = ("For an eligible claim return the smaller of the claim and 100 TEST units. "
                    "Return only the integer, a space and this receipt code: " + marker)
            package, source, report["approval"] = publish(host, space, writer, reviewer, name, body)
            host.token = writer["token"]
            report["marker_sha256"] = sha(marker.encode())
            deadline = time.monotonic() + 100
            paths = {}
            with ThreadPoolExecutor(max_workers=2) as pool:
                jobs = {}
                for client, load in (("codex", codex_load), ("claude-code", claude_load)):
                    project = root / client
                    exported = export(args.binary, host, writer["token"], project, client, package)
                    require(exported.returncode == 0, "export failed")
                    paths[client] = installed(project, client, package)
                    run = uid()
                    host.request("/v1/runs", {"run_id": run, "summary": "Synthetic native Skill loading probe"})
                    jobs[client] = pool.submit(load, args, project, paths[client], run, marker, body, deadline)
                for client, job in jobs.items():
                    try:
                        result = job.result()
                    except (RuntimeError, OSError, subprocess.SubprocessError) as error:
                        # Do not retain provider output, local paths or credentials.
                        report["clients"][client] = {"status": "INCOMPLETE", "failure": type(error).__name__}
                        if isinstance(error, RuntimeError):
                            print(client, str(error), flush=True)
                        continue
                    if client == "claude-code":
                        native = result["tool_result_success"] and result["input_body_observed"]
                    else:
                        native = result["model_request_body_verified"]
                    result["status"] = "OBSERVED" if native and result["answer_matches"] else "INCOMPLETE"
                    if result["status"] == "OBSERVED":
                        run = result["run_id"]
                        event = {"kind": "skill_loaded_reported", "skill_id": package["manifest"]["skill_id"],
                                 "version": 1, "digest": package["digest"]}
                        result["event_id"] = uid()
                        result["observed_at_ms"] = int(time.time() * 1000)
                        report_event = {
                            "event_id": result["event_id"], "reporter": "test",
                            "observed_at_ms": result["observed_at_ms"], "event": event,
                        }
                        receipt = host.request(f"/v1/runs/{run}/events", report_event)
                        recorded = host.request(f"/v1/runs/{run}")["events"]
                        require(len(recorded) == 1 and recorded[0]["report"] == report_event
                                and recorded[0]["evidence_origin"] == "reported",
                                "Run event attribution or package mismatch")
                        result["prefix"] = host.request(f"/v1/runs/{run}/prefix?through_sequence={receipt['sequence']}")
                    report["clients"][client] = result
                    print(client, result["status"], flush=True)
            host.token = admin
            host.request("/v1/events/" + source, {}, method="DELETE")
            host.token = writer["token"]
            route = "/v1/skills/" + package["manifest"]["skill_id"] + "/versions/1/package"
            try:
                host.request(route)
            except HTTPError as error:
                with error:
                    require(error.code in (403, 404), "unexpected online withdrawal result")
                    report["withdrawn_package_http"] = error.code
            else:
                raise RuntimeError("withdrawn package was distributed")
            report["withdrawal"] = {}
            for client in paths:
                fresh = root / ("withdrawn-" + client)
                denied = export(args.binary, host, writer["token"], fresh, client, package)
                reason = b"resource not found" if report["withdrawn_package_http"] == 404 else b"permission denied"
                require(denied.returncode != 0 and reason in denied.stderr.lower(),
                        "export did not report the expected withdrawal error")
                require(not any(fresh.iterdir()), "withdrawn export wrote project files")
                installed(root / client, client, package)
                report["withdrawal"][client] = {"new_export_denied": True, "existing_copy_matches": True}


    require(len(report["clients"]) == 2 and all(c["status"] == "OBSERVED" for c in report["clients"].values()),
            "one or more native loading observations incomplete")


def main(run_probe=probe, description=__doc__):
    parser = argparse.ArgumentParser(description=description)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--claude-model", required=True)
    parser.add_argument("--claude-settings", type=Path)
    parser.add_argument("--codex-model", required=True)
    parser.add_argument("--codex-credential-store", choices=("file", "keyring", "auto"))
    args = parser.parse_args()
    args.binary = args.binary.resolve(strict=True)
    args.claude = shutil.which("claude")
    args.codex = shutil.which("codex.exe" if os.name == "nt" else "codex")
    require(args.claude and args.codex, "both native clients must be installed")
    require(all(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:/-]*", m) for m in (args.claude_model, args.codex_model)),
            "invalid model selector")
    report = {"scope": "public synthetic unscored", "formal_trial": "NOT_RUN", "clients": {},
              "started_at": datetime.now(timezone.utc).isoformat(),
              "configured_models": {"codex": args.codex_model, "claude-code": args.claude_model}}
    start_time = time.monotonic()
    with args.report.open("x", encoding="utf-8") as out:
        try:
            report["cli_versions"] = {}
            for client, binary in (("codex", args.codex), ("claude-code", args.claude)):
                proc = subprocess.run([binary, "--version"], env=child_environment(),
                                      capture_output=True, timeout=10, check=True)
                version = re.search(rb"\b\d+\.\d+\.\d+\b", proc.stdout)
                require(version, "client version unavailable")
                report["cli_versions"][client] = version.group().decode("ascii")
            with args.binary.open("rb") as binary:
                report["origence_binary_sha256"] = hashlib.file_digest(binary, "sha256").hexdigest()
            run_probe(args, report)
        finally:
            report["elapsed_seconds"] = round(time.monotonic() - start_time, 2)
            json.dump(report, out, indent=2)
            out.write("\n")


if __name__ == "__main__":
    main()
