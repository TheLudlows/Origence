"""Exercise installed CLI plugins with a private, synthetic Origence workspace."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import threading
import time

from live_archive_smoke import child_environment
from mcp_smoke import Child, Host, isolated_env, require, sanitized

PLUGIN = "origence@origence-tools"
TOOLS = ("context_session", "context_search", "context_get")
QUERY = "PluginFixtureKeyword"
CONTENT = QUERY + " uses a published workspace policy."
REPO = Path(__file__).resolve().parent.parent


def execute(command, cwd, env, forbidden, expected=None, input_text=None):
    result = subprocess.run(command, cwd=cwd, env=env, input=input_text,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            encoding="utf-8", errors="replace", timeout=90)
    output = result.stdout + result.stderr
    sanitized(output, forbidden)
    if expected is None:
        require(result.returncode == 0, f"Client command failed ({result.returncode}): {command[1]}")
    else:
        require(result.returncode != 0 and expected in output, f"Expected diagnostic: {expected}")
    return result.stdout


def tool_value(result):
    require(not result.get("isError"), "Client tool call returned a protocol error")
    if result.get("structuredContent") is not None:
        return result["structuredContent"]
    return json.loads(next(block["text"] for block in result["content"] if block["type"] == "text"))


def published_hit(hits, asset):
    hit = next((item for item in hits if item["asset_id"] == asset), None)
    require(hit is not None and hit["version"] == 1, "Search missed the published version")
    return {"asset_id": hit["asset_id"], "version": hit["version"]}


def calls(lookup, expected_session, asset):
    require(lookup(TOOLS[0], {}) == expected_session, "Unexpected logical key or session")
    hits = lookup(TOOLS[1], {"query": QUERY, "mode": "keyword"})["hits"]
    item = lookup(TOOLS[2], published_hit(hits, asset))
    require(item["version"] == 1 and item["content"] == CONTENT, "Pinned get returned unexpected data")


def codex_client(cli, project, env, forbidden, expected_session, asset, writer_env):
    execute([cli, "plugin", "marketplace", "add", str(REPO)], project, env, forbidden)
    execute([cli, "plugin", "add", PLUGIN], project, env, forbidden)
    config = Path(env["CODEX_HOME"]) / "config.toml"
    require("mcp_servers" not in config.read_text(encoding="utf-8"), "Unexpected standalone MCP config")
    with Child([cli, "app-server", "--listen", "stdio://"], dict(env, OC_DATA_DIR=str(project)), forbidden) as child:
        request_id = 0

        def rpc(method, params):
            nonlocal request_id
            request_id += 1
            message = {"id": request_id, "method": method, "params": params}
            child.process.stdin.write((json.dumps(message) + "\n").encode())
            child.process.stdin.flush()
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                response = child.receive(max(0, deadline - time.monotonic()))
                sanitized(response, forbidden)
                if response.get("id") == request_id:
                    require("result" in response, f"Codex app-server method failed: {method}")
                    return response["result"]
            raise RuntimeError(f"Codex app-server deadline: {method}")

        rpc("initialize", {"clientInfo": {"name": "origence-plugin-smoke", "version": "1"},
                           "capabilities": {"experimentalApi": True}})
        child.process.stdin.write(b'{"method":"initialized"}\n')
        child.process.stdin.flush()
        thread = rpc("thread/start", {"cwd": str(project), "ephemeral": True})["thread"]["id"]
        hook_entries = rpc("hooks/list", {"threadId": thread})["data"]
        hooks = [hook for entry in hook_entries for hook in entry["hooks"] if hook.get("pluginId") == PLUGIN]
        require(len(hooks) == 4 and all(hook["enabled"] and hook["source"] == "plugin" for hook in hooks),
                "Codex did not discover the four enabled plugin hooks")
        require(all(hook["trustStatus"] == "untrusted" for hook in hooks), "Fresh Codex hooks must still require review")
        status = rpc("mcpServerStatus/list", {"threadId": thread})
        servers = [entry for entry in status["data"] if set(entry["tools"]) == set(TOOLS)]
        require(len(servers) == 1, "Codex did not discover the plugin's reader tools")
        server = servers[0]["name"]
        calls(lambda name, arguments: tool_value(rpc("mcpServer/tool/call", {
            "server": server, "tool": name, "threadId": thread, "arguments": arguments,
        })), expected_session, asset)
        child.check_stderr()
    codex_hook_turn(cli, project, writer_env, forbidden)
    execute([cli, "plugin", "remove", PLUGIN], project, env, forbidden)
    execute([cli, "plugin", "marketplace", "remove", "origence-tools"], project, env, forbidden)


class ScriptedCodex(BaseHTTPRequestHandler):
    """Loopback Responses stream with a fixed text completion and no tool calls."""
    def log_message(self, *_args):
        pass

    def do_POST(self):
        if self.path != "/v1/responses":
            self.send_error(404)
            return
        self.rfile.read(int(self.headers["Content-Length"]))
        message = {"id": "msg_fixture", "type": "message", "role": "assistant", "status": "completed",
                   "content": [{"type": "output_text", "text": "Origence hook fixture complete", "annotations": []}]}
        response = {"id": "resp_fixture", "object": "response", "status": "completed", "output": [message],
                    "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}}
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for name, data in (("response.output_item.done", {"output_index": 0, "item": message}),
                           ("response.completed", {"response": response})):
            self.wfile.write(f"event: {name}\ndata: {json.dumps({'type': name, **data})}\n\n".encode())


def codex_hook_turn(cli, project, env, forbidden):
    with ThreadingHTTPServer(("127.0.0.1", 0), ScriptedCodex) as model:
        thread = threading.Thread(target=model.serve_forever, daemon=True)
        thread.start()
        try:
            # The isolated fixture installs only the reviewed Origence package. Retain Codex's
            # default untrusted state above, then allow these hooks for this synthetic turn only.
            output = execute([cli, "exec", "--ephemeral", "--json", "--sandbox", "read-only",
                              "--dangerously-bypass-hook-trust", "-m", "gpt-5.4-mini",
                              "-c", 'model_provider="fixture"', "-c", 'model_providers.fixture.name="Fixture"',
                              "-c", f'model_providers.fixture.base_url="http://127.0.0.1:{model.server_port}/v1"',
                              "-c", 'model_providers.fixture.env_key="ORIGENCE_CODEX_FIXTURE_KEY"',
                              "-c", 'approval_policy="never"', "ORIGENCE_FIXTURE_PRIVATE_PROMPT"],
                             project, dict(env, ORIGENCE_CODEX_FIXTURE_KEY="fixture-local-only"), forbidden)
            require(any(json.loads(line).get("type") == "turn.completed" for line in output.splitlines()),
                    "Codex did not complete the synthetic turn")
        finally:
            model.shutdown()
            thread.join()


class ScriptedClaude(BaseHTTPRequestHandler):
    """Minimal loopback Messages event stream. Requests are never forwarded."""
    def log_message(self, *_args):
        pass

    def do_POST(self):
        if self.path.split("?")[0] != "/v1/messages":
            self.send_error(404)
            return
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        results = [block for message in request["messages"] if isinstance(message["content"], list)
                   for block in message["content"] if block["type"] == "tool_result"]
        offset = len(results)
        if offset:
            self.server.results[offset - 1] = results[-1]
        if offset < len(TOOLS):
            matching = [tool["name"] for tool in request.get("tools", [])
                        if tool["name"].endswith("__" + TOOLS[offset])]
            if len(matching) != 1:
                self.send_error(422, "Installed Origence tool not available")
                return
            if offset == 2:
                hits = tool_value({"content": results[-1]["content"]})["hits"]
                arguments = published_hit(hits, self.server.asset)
            else:
                arguments = ({}, {"query": QUERY, "mode": "keyword"})[offset]
            block = {"type": "tool_use", "id": f"toolu_fixture_{offset}", "name": matching[0], "input": {}}
            delta = {"type": "input_json_delta", "partial_json": json.dumps(arguments)}
            stop = "tool_use"
        else:
            block = {"type": "text", "text": ""}
            delta = {"type": "text_delta", "text": "Origence plugin fixture complete"}
            stop = "end_turn"
        usage = {"input_tokens": 10, "output_tokens": 5}
        events = [
            {"type": "message_start", "message": {"id": f"msg_fixture_{offset}", "type": "message",
             "role": "assistant", "model": request["model"], "content": [], "stop_reason": None, "usage": usage}},
            {"type": "content_block_start", "index": 0, "content_block": block},
            {"type": "content_block_delta", "index": 0, "delta": delta},
            {"type": "content_block_stop", "index": 0},
            {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": usage},
            {"type": "message_stop"},
        ]
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for event in events:
            self.wfile.write(f"event: {event['type']}\ndata: {json.dumps(event)}\n\n".encode())


def claude_client(cli, project, env, forbidden, expected_session, asset):
    execute([cli, "plugin", "install", "origence", "--marketplace", str(REPO)], project, env, forbidden)
    require(not (project / ".mcp.json").exists(), "Unexpected project MCP file")
    with ThreadingHTTPServer(("127.0.0.1", 0), ScriptedClaude) as model:
        model.results = {}
        model.asset = asset
        thread = threading.Thread(target=model.serve_forever, daemon=True)
        thread.start()
        try:
            env = dict(env, ANTHROPIC_BASE_URL=f"http://127.0.0.1:{model.server_port}",
                       ANTHROPIC_API_KEY="fixture-local-only", ENABLE_TOOL_SEARCH="false",
                       CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1", DISABLE_TELEMETRY="1")
            names = ",".join("mcp__plugin_origence_origence__" + tool for tool in TOOLS)
            execute([cli, "-p", "--no-session-persistence", "--output-format", "stream-json", "--verbose",
                     "--tools", "", "--allowedTools", names, "--model", "claude-sonnet-4-6",
                     "Use the installed Origence tools to check the session, search the fixture, and read its version."],
                    project, env, forbidden)
            require(set(model.results) == {0, 1, 2}, "Claude Code did not complete all three installed tool calls")
            for result in model.results.values():
                sanitized(result, forbidden)
                require(not result.get("is_error"), "Claude Code reported a tool failure")
            def lookup(name, arguments):
                result = model.results[TOOLS.index(name)]
                return tool_value({"content": result["content"]})
            calls(lookup, expected_session, asset)
        finally:
            model.shutdown()
            thread.join()
    execute([cli, "plugin", "uninstall", PLUGIN], project, env, forbidden)
    execute([cli, "plugin", "marketplace", "remove", "origence-tools"], project, env, forbidden)


def diagnostics(binary, project, env, forbidden):
    missing = dict(env, OC_API_KEY="")
    execute([binary, "mcp"], project, missing, forbidden, "OC_API_KEY is required")
    invalid_token = "invalid-plugin-fixture"
    execute([binary, "mcp"], project, dict(env, OC_API_KEY=invalid_token),
            [*forbidden, invalid_token], "authentication required")
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))  # Reserve the port without listening.
        offline = dict(env, OC_SERVER_URL=f"http://127.0.0.1:{sock.getsockname()[1]}")
        execute([binary, "mcp"], project, offline, forbidden, "local host request failed")
    require(shutil.which(binary.name, path=str(project / "absent")) is None,
            "Unexpected binary in missing-path fixture")


def check_packages():
    for client, manifest_dir in (("claude-code", ".claude-plugin"), ("codex", ".codex-plugin")):
        package = REPO / "plugins" / client / "origence"
        require({path.name for path in package.iterdir()} == {manifest_dir, ".mcp.json", "hooks"},
                "Plugin must ship MCP and Hook configurations")
        manifest = json.loads((package / manifest_dir / "plugin.json").read_text(encoding="utf-8"))
        require(manifest.get("mcpServers") == "./.mcp.json", "Plugin is missing its MCP declaration")
        if client == "codex":
            require(manifest.get("hooks") == "./hooks/hooks.json", "Plugin is missing its Hook declaration")
        hooks = json.loads((package / "hooks" / "hooks.json").read_text(encoding="utf-8"))["hooks"]
        require({"UserPromptSubmit", "PostToolUse", "Stop"} <= hooks.keys(), "Plugin is missing lifecycle events")


def check_hook_run(host, summary, component, forbidden):
    matches = [run for run in host.request("/v1/runs?limit=10")["items"] if run["summary"] == summary]
    require(len(matches) == 1, "Plugin did not create one private Run for the synthetic prompt")
    trace = host.request(f"/v1/runs/{matches[0]['run_id']}")
    sanitized(trace, [*forbidden, "ORIGENCE_FIXTURE_PRIVATE_PROMPT", "Use the installed Origence tools"])
    events = [item["report"]["event"] for item in trace["events"]]
    require(any(event["kind"] == "coverage" and event["component"] == component for event in events),
            "Plugin Run is missing the prompt coverage event")
    require(any(event["kind"] == "result_reported" and event["outcome"] == "unknown" for event in events),
            "Plugin Run is missing the terminal observation")
    if component == "claude_code_hook":
        tools = {event["tool"] for event in events if event["kind"] == "tool_reported"}
        require({"mcp__origence__context_search", "mcp__origence__context_get"} <= tools,
                "Plugin did not observe its own allowlisted MCP calls")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--codex", default="codex")
    parser.add_argument("--claude", default="claude")
    args = parser.parse_args()
    check_packages()
    with tempfile.TemporaryDirectory(prefix="origence plugin ") as temporary:
        root = Path(temporary)
        binary_dir = root / "Origence binary"
        data = root / "database"
        binary_dir.mkdir()
        data.mkdir()
        binary = binary_dir / ("origence.exe" if os.name == "nt" else "origence")
        shutil.copy2(args.binary, binary)
        host_env = isolated_env(str(data))
        bootstrap = subprocess.run([binary, "--offline", "workspace-create", "Plugin fixture"],
                                   cwd=data, env=host_env, capture_output=True, check=True, timeout=60)
        workspace = json.loads(bootstrap.stdout)
        admin = workspace["token"]
        with Host(str(binary), host_env, admin, [admin], serve_args=("--local",)) as host:
            asset = host.publish(CONTENT)
            reader = host.request("/admin/keys", {"workspace_id": workspace["workspace_id"], "role": "reader"})["token"]
            writer = host.request("/admin/keys", {"workspace_id": workspace["workspace_id"], "role": "writer"})["token"]
            host.token = reader
            require(host.request("/v1/whoami")["workspace_id"] == workspace["workspace_id"],
                    "Reader fixture is in the wrong workspace")
            session = host.request("/v1/session")
            env = child_environment()
            path_name = next(name for name in env if name.upper() == "PATH")
            env["PATH"] = str(binary_dir) + os.pathsep + env.pop(path_name)
            env.update(HOME=temporary, USERPROFILE=temporary, OC_SERVER_URL=host.base, OC_API_KEY=reader)
            forbidden = [admin, reader, writer]
            diagnostics(binary, root, env, forbidden)
            for label, cli, run in (("Codex", args.codex, codex_client), ("Claude Code", args.claude, claude_client)):
                project = root / f"{label} project"
                config = root / f"{label} settings"
                project.mkdir()
                config.mkdir()
                execute(["git", "init", "-q"], project, env, forbidden)
                client_env = dict(env, CODEX_HOME=str(config), CLAUDE_CONFIG_DIR=str(config))
                print(execute([cli, "--version"], project, client_env, forbidden).strip(), flush=True)
                writer_env = dict(client_env, OC_API_KEY=writer, OC_WORKSPACE_ID=workspace["workspace_id"])
                if label == "Codex":
                    run(cli, project, client_env, forbidden, session, asset, writer_env)
                else:
                    run(cli, project, client_env, forbidden, session, asset)
                    host.token = writer
                    run(cli, project, writer_env, forbidden, host.request("/v1/session"), asset)
                    host.token = reader
                require(not (project / ".mcp.json").exists() and not (project / ".codex/config.toml").exists(),
                        "Client project contains standalone MCP registration")
                require(not (project / ".claude/settings.local.json").exists() and not (project / ".codex/hooks.json").exists(),
                        "Client project contains standalone Hook registration")
                require(host.request("/v1/runs?limit=1")["items"] == [], "Unexpected reader Run activity")
                host.token = writer
                summary = "Codex turn (hook metadata only)" if label == "Codex" else "Claude Code prompt (hook metadata only)"
                component = "codex_hook" if label == "Codex" else "claude_code_hook"
                check_hook_run(host, summary, component, forbidden)
                host.token = reader
                print(f"PASS {label}: install, reader session, search, exact version, writer Run hook, uninstall", flush=True)
            print("PASS startup diagnostics, packaged hooks, and reader isolation", flush=True)


if __name__ == "__main__":
    main()
