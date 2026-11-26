"""Run the optional live Claude Code / Codex private archive acceptance test."""
import argparse
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import signal
import subprocess
import tempfile
import time
from urllib.error import HTTPError
import uuid

from mcp_smoke import Host, isolated_env, require


WRITE_TOOLS = ["context_turn_archive", "context_run_create", "context_run_prepare"]
CLAUDE_ENV = ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL",
              "ANTHROPIC_DEFAULT_HAIKU_MODEL", "ANTHROPIC_DEFAULT_SONNET_MODEL",
              "ANTHROPIC_DEFAULT_OPUS_MODEL")


def child_environment():
    # Give the client the OS paths needed for startup, then add its own credentials.
    names = {"PATH", "PATHEXT", "SYSTEMROOT", "WINDIR", "COMSPEC", "TEMP", "TMP", "TMPDIR",
             "HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "PROGRAMDATA", "PROGRAMFILES",
             "PROGRAMFILES(X86)", "HOMEDRIVE", "HOMEPATH", "LANG", "LC_ALL", "HTTP_PROXY",
             "HTTPS_PROXY", "ALL_PROXY", "SSL_CERT_FILE", "NODE_EXTRA_CA_CERTS"}
    env = {key: value for key, value in os.environ.items() if key.upper() in names}
    env["NO_PROXY"] = "127.0.0.1,localhost,::1"
    return env


def claude_command(args, directory, host, names, env):
    settings = os.environ
    if args.claude_settings:
        settings = json.loads(args.claude_settings.read_text(encoding="utf-8-sig")).get("env", {})
    for key in CLAUDE_ENV:
        value = settings.get(key)
        if value:
            env[key] = value
    require(env.get("ANTHROPIC_API_KEY") or env.get("ANTHROPIC_AUTH_TOKEN"),
            "Configure credentials for the Claude Code --bare run.")
    env.update(CLAUDE_CONFIG_DIR=str(directory / "settings"), DISABLE_TELEMETRY="1",
               CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1", DISABLE_ERROR_REPORTING="1")
    config = directory / "mcp.json"
    config.write_text(json.dumps({"mcpServers": {"origence": {
        "type": "http", "url": host.base + "/mcp",
        "headers": {"Authorization": "Bearer ${OC_API_KEY}"}}}}), encoding="utf-8")
    return [args.claude, "--bare", "-p", "--no-session-persistence", "--strict-mcp-config",
            "--mcp-config", str(config), "--model", args.claude_model, "--tools", "",
            "--allowedTools", ",".join("mcp__origence__" + n for n in names),
            "--output-format", "stream-json", "--verbose"]


def codex_command(args, directory, host, names, env):
    for key in ("CODEX_HOME", "OPENAI_API_KEY"):
        if key in os.environ:
            env[key] = os.environ[key]
    server = ("mcp_servers={origence={url=" + json.dumps(host.base + "/mcp") +
              ',bearer_token_env_var="OC_API_KEY",required=true,enabled_tools=' + json.dumps(names) + "}}")
    cmd = [args.codex, "exec", "--ephemeral", "--json", "--ignore-user-config",
           "--skip-git-repo-check", "--sandbox", "read-only", "-m", args.codex_model,
           "-c", 'model_provider="openai"', "-c", 'approval_policy="never"', "-c", server,
           "-c", 'model_reasoning_effort="low"', "--output-last-message", str(directory / "last-message")]
    if args.codex_credential_store:
        cmd.extend(["-c", "cli_auth_credentials_store=" + json.dumps(args.codex_credential_store)])
    for name in names:
        cmd.extend(["-c", f'mcp_servers.origence.tools.{name}.approval_mode="approve"'])
    return cmd + ["--", "-"]


def client_result(output, allowed):
    observed = []
    last = ""
    for line in output.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        item = event.get("item") or {}
        if event.get("type") == "item.completed":
            if item.get("type") == "mcp_tool_call" and item.get("server") == "origence" and item.get("status") == "completed":
                observed.append(item.get("tool"))
            elif item.get("type") == "agent_message":
                last = item.get("text", "")
        if event.get("type") == "assistant":
            for block in event["message"].get("content", []):
                if block.get("type") == "tool_use" and block["name"].startswith("mcp__origence__"):
                    observed.append(block["name"].removeprefix("mcp__origence__"))
        if event.get("type") == "result":
            require(not event.get("is_error"), "Claude Code reported a failed turn")
            last = event.get("result", "")
    return [name for name in observed if name in allowed], last


def run_client(args, client, directory, host, prompt, names, challenge, deadline):
    directory.mkdir()
    env = child_environment()
    env["OC_API_KEY"] = host.token
    if client == "Claude Code":
        cmd = claude_command(args, directory, host, names, env)
    else:
        cmd = codex_command(args, directory, host, names, env)
        prompt += " If tools are nested, discover Origence in ALL_TOOLS using functions.exec and call via tools."
    require(not challenge or challenge not in prompt, "peer answer present in the reading prompt")
    start = time.monotonic()
    remaining = deadline - start
    require(remaining > 0, "live-client time budget exhausted")
    options = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {"start_new_session": True}
    with subprocess.Popen(cmd, cwd=directory, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL, **options) as process:
        try:
            output, _ = process.communicate(prompt.encode(), timeout=min(85, remaining))
        except subprocess.TimeoutExpired:
            if os.name == "nt":
                try:
                    subprocess.run(["taskkill.exe", "/PID", str(process.pid), "/T", "/F"],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
                finally:
                    process.kill()
            else:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            raise RuntimeError(f"{client} exceeded the model time limit") from None
    require(process.returncode == 0, f"{client} exited with status {process.returncode}")
    observed, last = client_result(output, names)
    if client == "Codex" and (directory / "last-message").is_file():
        last = (directory / "last-message").read_text(encoding="utf-8-sig")
    require(set(names) <= set(observed), f"{client} did not call all required Origence tools")
    require(not challenge or challenge in last, f"{client} did not return the peer's archived answer")
    print(f"{client}: {', '.join(observed)}", flush=True)
    return {"client": client, "observed_tools": observed, "seconds": round(time.monotonic()-start, 2),
            "peer_answer_verified": True if challenge else None}


def archive_case():
    return {"turn_id": str(uuid.uuid4()), "run_id": str(uuid.uuid4()), "event_id": str(uuid.uuid4()),
            "answer": secrets.token_hex(12)}


def round_trip(args, root, host, session, cases, deadline, report):
    for index, client in enumerate(("Claude Code", "Codex", "Claude Code")):
        own = cases[index] if index < len(cases) else None
        peer = cases[index-1] if index else None
        prompt = "Public synthetic acceptance task. Use only the Origence MCP server. "
        names = []
        if peer:
            prompt += f"Read turn {peer['turn_id']} with context_turn_get. "
            names.append("context_turn_get")
        if own:
            prompt += (f"Archive turn {own['turn_id']} with question 'Synthetic archive check', "
                       f"exact answer {own['answer']} and empty used_evidence. Create Run {own['run_id']} "
                       f"with summary 'Synthetic archive check'. Prepare it with event_id {own['event_id']} "
                       "and context containing only the query 'synthetic'. ")
            names.extend(WRITE_TOOLS)
        if peer:
            prompt += (f"Finally return just the verbatim answer field from context_turn_get for {peer['turn_id']}. "
                       "Read the JSON in the tool result's text content. Preserve all characters.")
        else:
            prompt += "Return DONE."
        report["sessions"].append(run_client(args, client, root / f"client-{index}", host, prompt, names,
                                             peer["answer"] if peer else None, deadline))
        if own:
            turn = host.request(f"/v1/sessions/{session}/turns/{own['turn_id']}")
            require(turn["answer"] == own["answer"] and turn["question"] == "Synthetic archive check",
                    "server archive did not match the explicitly submitted turn")
            events = host.request("/v1/runs/" + own["run_id"])["events"]
            require(len(events) == 1 and events[0]["report"]["kind"] == "context_prepared"
                    and events[0]["report"]["event_id"] == own["event_id"], "server preparation missing")
    require(cases[0]["run_id"] != cases[1]["run_id"], "Run IDs collided")


def denied(host, path, expected, body=None):
    try:
        host.request(path, body)
    except HTTPError as error:
        with error:
            require(error.code in expected, f"expected {expected}, received HTTP {error.code}")
            return error.code
    raise RuntimeError("expected the host to reject the operation")


def verify_controls(host, admin, writer, outsider, cases, session, checks):
    route = "/v1/runs/" + cases[1]["run_id"]
    prepare = route + "/context"
    archived = f"/v1/sessions/{session}/turns/{cases[0]['turn_id']}"
    payload = {"event_id": cases[1]["event_id"], "context": {"query": "synthetic"}}
    before = host.request(route)["events"]
    receipt = host.request(prepare, payload)
    require(receipt == host.request(prepare, payload) and before == host.request(route)["events"],
            "identical replay changed the receipt or the trace")
    checks["replay_preserved"] = True
    checks["conflict"] = denied(host, prepare, (409,), dict(payload, context={"query": "other"}))
    event_id = str(uuid.uuid4())
    checks["vector_unavailable"] = denied(host, prepare, (503,), {"event_id": event_id, "context": {
        "query": "synthetic", "mode": "vector", "allow_partial": False}})
    after = host.request(route)["events"]
    require(after[:-1] == before and after[-1]["report"]["kind"] == "context_preparation_failed"
            and after[-1]["report"]["event_id"] == event_id, "missing immutable preparation failure")
    host.token = outsider
    checks["other_admin_denied"] = [denied(host, path, (403, 404)) for path in (archived, route)]
    host.token = admin
    rotated = host.request("/admin/keys/" + writer["key_id"] + "/rotate", {})
    host.forbidden.append(rotated["token"])
    host.token = writer["token"]
    checks["old_key_denied"] = denied(host, archived, (401,))
    host.token = rotated["token"]
    require(host.request("/v1/session")["session_id"] == session
            and host.request(archived)["answer"] == cases[0]["answer"] and host.request(prepare, payload) == receipt,
            "rotation lost the original archive or receipt")
    checks["rotation_preserved_ownership"] = True
    host.token = admin
    host.request("/admin/keys/" + rotated["key_id"], method="DELETE")
    host.token = rotated["token"]
    checks["revoked_read_and_replay"] = [denied(host, archived, (401,)), denied(host, prepare, (401,), payload)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path, help="Origence executable")
    parser.add_argument("--claude-model", required=True)
    parser.add_argument("--codex-model", required=True)
    parser.add_argument("--claude-settings", type=Path, help="Optional provider env from a Claude settings JSON")
    parser.add_argument("--codex-credential-store", choices=["file", "keyring", "auto"])
    parser.add_argument("--report", required=True, type=Path, help="New JSON results file")
    args = parser.parse_args()
    for model in (args.claude_model, args.codex_model):
        require(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:/-]*", model), "invalid model selector")
    args.binary = args.binary.resolve(strict=True)
    args.claude = shutil.which("claude")
    args.codex = shutil.which("codex.exe" if os.name == "nt" else "codex")
    require(args.claude and args.codex, "install Claude Code and Codex CLI on PATH")
    start = time.monotonic()
    report = {"status": "INCOMPLETE", "configured_models": [args.claude_model, args.codex_model],
              "sessions": [], "checks": {}}
    with args.report.open("x", encoding="utf-8") as output:
        try:
            with tempfile.TemporaryDirectory(prefix="origence-client-archive-") as folder:
                root = Path(folder)
                env = isolated_env(folder)
                require("OC_ANYSEEK_API_KEY" in env, "run under tools/anyseek_smoke.py --run")
                boot = subprocess.run([args.binary, "--offline", "workspace-create", "synthetic-archive"],
                                      cwd=root, env=env, capture_output=True, check=True, timeout=15)
                workspace = json.loads(boot.stdout)
                admin = workspace["token"]
                with Host(args.binary, env, admin, [admin, env["OC_ANYSEEK_API_KEY"]]) as host:
                    def issue(role):
                        key = host.request("/admin/keys", {"workspace_id": workspace["workspace_id"], "role": role})
                        host.forbidden.append(key["token"])
                        return key
                    writer = issue("writer")
                    outsider = issue("admin")
                    host.publish("Synthetic public fixture for shared archive verification.")
                    host.token = writer["token"]
                    session = host.request("/v1/session")["session_id"]
                    cases = [archive_case(), archive_case()]
                    round_trip(args, root, host, session, cases, start + 240, report)
                    verify_controls(host, admin, writer, outsider["token"], cases, session, report["checks"])
            report["status"] = "PASS"
        finally:
            report["elapsed_seconds"] = round(time.monotonic()-start, 2)
            json.dump(report, output, indent=2)
            output.write("\n")


if __name__ == "__main__":
    main()
