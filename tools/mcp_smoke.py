"""Black-box, no-model MCP stdio regression against a built Origence binary.

Run: python3 tools/mcp_smoke.py --binary target/debug/origence

Only synthetic data and newly issued keys in a temporary database are used.
Subprocesses are intentional here: library calls cannot test the actual CLI,
stdio framing, startup authentication, or EOF shutdown. Each child has bounded
response/startup/shutdown waits and is reaped, including on failure. Production
code does not spawn a child. No MCP client package or provider is required.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
import uuid


TIMEOUT = 30
STARTUP_TIMEOUT = 120
MAX_BYTES = 8 * 1024 * 1024
QUERY = "MCPWIREPOLICY"
OLD_CONTENT = QUERY + " requires one approval. Café 审批."
CONTENT = QUERY + " requires two approvals. Café 审批."
BAD_INPUT = "MCP_UNTRUSTED_INPUT_MUST_NOT_LEAK"
# rmcp 1.8.0 recognizes these clients; unknown versions use Origence's retained
# 0.8.x advertised version. This is deliberately a wire-level assertion.
PROTOCOLS = (
    ("2024-11-05", "2024-11-05"),
    ("2025-03-26", "2025-03-26"),
    ("2025-06-18", "2025-06-18"),
    ("2025-11-25", "2025-11-25"),
    ("2026-07-28", "2026-07-28"),
    ("2099-01-01", "2025-03-26"),
)
TOOL_FIELDS = {
    "context_search": {
        "query": "string", "mode": "string", "limit": "integer",
        "allow_partial": "boolean", "components": "object",
        "memory_identity": "object",
    },
    "context_resolve": {
        "query": "string", "mode": "string", "budget_tokens": "integer",
        "allow_partial": "boolean", "components": "object",
        "memory_identity": "object",
    },
    "context_bundle": {
        "query": "string", "mode": "string", "allow_partial": "boolean",
        "budget": "integer", "routes": "array", "memory_identity": "object",
        "applicability_context": "array", "session_id": "string",
        "exclude_session_id": "string",
    },
    "context_get": {"asset_id": "string", "version": "integer"},
}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def sanitized(value, forbidden):
    text = value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)
    for secret in forbidden:
        require(not secret or secret not in text, "response/log leaked a token, path, or source/input text")
    for marker in ("SELECT ", "INSERT INTO ", "UPDATE oc_", "panicked at", "stack backtrace:"):
        require(marker not in text, "response/log exposed an internal diagnostic")


def isolated_env(directory):
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith("OC_")
           and key.upper() not in {"HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"}}
    env.update(OC_ENABLE_MODELS="false", OC_DATA_DIR=directory, RUST_LOG="warn",
               NO_PROXY="127.0.0.1,localhost,::1", no_proxy="127.0.0.1,localhost,::1")
    return env


class Child:
    """Drain bounded stdout lines on a thread (also works with Windows pipes)."""

    def __init__(self, command, env, forbidden):
        self.forbidden = forbidden
        self.stderr = tempfile.TemporaryFile()
        self.lines = queue.Queue(maxsize=16)
        self.reader_error = None
        try:
            self.process = subprocess.Popen(command, env=env, cwd=env["OC_DATA_DIR"],
                                            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                            stderr=self.stderr)
        except BaseException:
            self.stderr.close()
            raise
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()

    def _read(self):
        try:
            while True:
                line = self.process.stdout.readline(MAX_BYTES + 1)
                if len(line) > MAX_BYTES:
                    raise RuntimeError("child stdout line exceeded size limit")
                self.lines.put_nowait(line)
                if not line:
                    return
        except BaseException:
            self.reader_error = "child stdout exceeded its limits or could not be read"

    def receive(self, timeout=TIMEOUT):
        require(self.reader_error is None, self.reader_error)
        try:
            line = self.lines.get(timeout=timeout)
        except queue.Empty:
            raise RuntimeError("child stdout response timed out") from None
        require(self.reader_error is None, self.reader_error)
        require(bool(line), "child exited before the expected response")
        require(line.endswith(b"\n"), "stdio message was not newline terminated")
        try:
            value = json.loads(line)
        except (ValueError, UnicodeError):
            raise RuntimeError("child wrote non-JSON output to stdout") from None
        require(isinstance(value, dict), "child stdout was not a JSON object")
        return value

    def check_stderr(self):
        self.stderr.seek(0)
        log = self.stderr.read(MAX_BYTES + 1)
        require(len(log) <= MAX_BYTES, "child stderr exceeded size limit")
        text = log.decode("utf-8", errors="replace")
        sanitized(text, self.forbidden)
        return text

    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=TIMEOUT)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=TIMEOUT)
        self.reader.join(timeout=TIMEOUT)
        self.process.stdin.close()
        self.process.stdout.close()
        self.stderr.close()

    def __enter__(self):
        return self

    def __exit__(self, kind, value, traceback):
        self.close()


class Mcp(Child):
    def __init__(self, command, env, forbidden):
        super().__init__(command, env, forbidden)
        self.next_id = 0

    def send(self, message):
        data = json.dumps(message, ensure_ascii=False).encode("utf-8") + b"\n"
        try:
            self.process.stdin.write(data)
            self.process.stdin.flush()
        except (BrokenPipeError, OSError):
            raise RuntimeError("MCP closed stdin unexpectedly") from None

    def request(self, method, params=None):
        self.next_id += 1
        message = {"jsonrpc": "2.0", "id": self.next_id, "method": method}
        if params is not None:
            message["params"] = params
        self.send(message)
        response = self.receive()
        require(response.get("jsonrpc") == "2.0", "invalid JSON-RPC version")
        require(response.get("id") == self.next_id, "JSON-RPC response ID mismatch")
        require(("result" in response) != ("error" in response), "invalid JSON-RPC response envelope")
        return response

    def success(self, method, params=None):
        response = self.request(method, params)
        require("result" in response, method + " unexpectedly returned a JSON-RPC error")
        return response["result"]

    def initialize(self, requested, expected):
        info = self.success("initialize", {
            "protocolVersion": requested, "capabilities": {},
            "clientInfo": {"name": "origence-mcp-smoke", "version": "1"},
        })
        require(info.get("protocolVersion") == expected, "MCP protocol negotiation changed")
        require(set(info.get("capabilities", {})) == {"tools"}, "MCP advertised unexpected capabilities")
        require(bool(info.get("serverInfo", {}).get("name")), "missing MCP server identity")
        require("untrusted evidence" in info.get("instructions", ""), "missing untrusted-evidence instruction")
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        require(self.success("ping") == {}, "MCP ping did not return an empty result")

    def tool(self, name, arguments):
        result = self.success("tools/call", {"name": name, "arguments": arguments})
        require(result.get("isError", False) is False, name + " returned an error result")
        require(result.get("structuredContent") is None, name + " unexpectedly added structured output")
        content = result.get("content", [])
        require(len(content) == 1 and content[0].get("type") == "text",
                name + " must return one JSON text content block")
        try:
            value = json.loads(content[0]["text"])
        except (KeyError, ValueError, TypeError):
            raise RuntimeError(name + " returned invalid JSON text") from None
        require(isinstance(value, dict), name + " result must be a JSON object")
        return value

    def error(self, name, arguments, code, message):
        response = self.request("tools/call", {"name": name, "arguments": arguments})
        sanitized(response, self.forbidden)
        require("error" in response, name + " unexpectedly succeeded")
        error = response["error"]
        require(error.get("code") == code and error.get("message") == message,
                name + " changed its sanitized JSON-RPC error contract")
        require(error.get("data") is None, "MCP error exposed unexpected diagnostic data")

    def invalid_arguments(self, name, arguments, message=None):
        response = self.request("tools/call", {"name": name, "arguments": arguments})
        sanitized(response, self.forbidden)
        error = response.get("error", {})
        require(error.get("code") == -32602 and bool(error.get("message")),
                name + " failed to reject malformed arguments as invalid params")
        require(error.get("message", "").startswith("failed to deserialize parameters: "),
                "argument error changed its parameter-decoding prefix")
        if message is not None:
            require(error["message"] == message, "argument error changed its exact message")
        require(error.get("data") is None, "argument error exposed diagnostic data")

    def shutdown(self):
        # EOF is the stdio transport shutdown mechanism, not a fabricated MCP
        # shutdown method. A timeout/kill is a failure, never a successful exit.
        self.process.stdin.close()
        try:
            code = self.process.wait(timeout=TIMEOUT)
        except subprocess.TimeoutExpired:
            raise RuntimeError("MCP did not exit after stdin EOF") from None
        self.reader.join(timeout=TIMEOUT)
        require(not self.reader.is_alive(), "MCP stdout reader did not finish")
        require(self.reader_error is None, self.reader_error)
        require(code == 0, "MCP returned nonzero after stdin EOF")
        require(self.lines.get(timeout=TIMEOUT) == b"" and self.lines.empty(),
                "MCP emitted unexpected stdout during shutdown")
        self.check_stderr()


def dereference(root, node):
    if "$ref" in node:
        path = node["$ref"]
        require(path.startswith("#/"), "schema has a nonlocal reference")
        target = root
        for part in path[2:].split("/"):
            target = target[part.replace("~1", "/").replace("~0", "~")]
        return dereference(root, target)
    choices = node.get("anyOf", node.get("oneOf"))
    if choices:
        values = [choice for choice in choices if choice.get("type") != "null"]
        require(len(values) == 1, "schema changed nullable input shape")
        return dereference(root, values[0])
    return node


def nullable(root, node):
    if "$ref" in node:
        return nullable(root, dereference(root, node))
    kind = node.get("type")
    if kind == "null" or isinstance(kind, list) and "null" in kind:
        return True
    return any(nullable(root, part) for part in node.get("anyOf", node.get("oneOf", [])))


def schema_type(root, node, expected):
    node = dereference(root, node)
    actual = node.get("type")
    require(actual == expected or actual == [expected, "null"] or actual == ["null", expected],
            "MCP input schema type changed")
    return node


def schema_object(root, node, fields, required, closed=True):
    node = schema_type(root, node, "object")
    require(set(node.get("properties", {})) == set(fields), "MCP input schema fields changed")
    require(set(node.get("required", [])) == set(required), "MCP input required fields changed")
    if closed:
        require(node.get("additionalProperties") is False, "MCP schema no longer rejects unknown fields")
    for name, expected in fields.items():
        schema_type(root, node["properties"][name], expected)
    return node["properties"]


def check_tools(mcp):
    result = mcp.success("tools/list", {})
    require(result.get("nextCursor") is None, "unexpected MCP tool pagination")
    tools = result.get("tools", [])
    require(len(tools) == 4 and {tool.get("name") for tool in tools} == set(TOOL_FIELDS),
            "MCP must expose exactly the four read-only context tools")
    schemas = {}
    for tool in tools:
        name = tool["name"]
        require(bool(tool.get("description")), "MCP tool description missing")
        require(tool.get("outputSchema") is None, "MCP unexpectedly advertised structured output")
        root = tool["inputSchema"]
        schemas[name] = root
        properties = schema_object(root, root, TOOL_FIELDS[name],
                                   ["asset_id" if name == "context_get" else "query"],
                                   closed=name != "context_get")
        if name == "context_get":
            require(nullable(root, properties["version"]), "get version no longer accepts null")
            continue
        require(nullable(root, properties["memory_identity"]), "memory identity no longer accepts null")
        require(properties["mode"].get("default") == "keyword", "retrieval default mode changed")
        require(properties["allow_partial"].get("default") is False, "partial retrieval default changed")
        identity = schema_object(root, properties["memory_identity"],
                                 {"subject": "object", "predicate": "string", "context": "object"},
                                 ["subject", "predicate"])
        subject = schema_object(root, identity["subject"],
                                {"kind": "string", "stable_id": "string"}, ["kind", "stable_id"])
        require(set(dereference(root, subject["kind"])["enum"]) ==
                {"user", "agent", "project", "service", "team"}, "identity subject kinds changed")
        schema_type(root, identity["context"]["additionalProperties"], "string")
        if name in ("context_search", "context_resolve"):
            components = schema_object(root, properties["components"],
                                       {"summaries": "boolean", "graph": "boolean"}, [])
            require(all(value.get("default") is True for value in components.values()),
                    "retrieval component defaults changed")
            field, default = ("limit", 10) if name == "context_search" else ("budget_tokens", 2000)
            require(properties[field].get("default") == default, "retrieval budget default changed")
        else:
            require(all(nullable(root, properties[field]) for field in ("session_id", "exclude_session_id")),
                    "bundle session filters no longer accept null")
            require(properties["budget"].get("default") == 4000, "bundle byte budget default changed")
            routes = schema_object(root, properties["routes"]["items"],
                                   {"route": "string", "limit": "integer", "max_bytes": "integer"},
                                   ["route"])
            require(set(dereference(root, routes["route"])["enum"]) ==
                    {"memory", "knowledge", "learning", "session"}, "bundle route enum changed")
            require(routes["limit"].get("default") == 20 and routes["max_bytes"].get("default") == 4000,
                    "bundle per-route defaults changed")
            require(properties["routes"].get("default") == [
                {"route": route, "limit": 20, "max_bytes": 4000}
                for route in ("memory", "knowledge", "learning")], "default bundle routes changed")
            schema_type(root, properties["applicability_context"]["items"], "string")
    return schemas


def exercise(mcp, asset_id, workspace_id):
    hits = mcp.tool("context_search", {"query": QUERY})
    require(hits["effective_mode"] == "keyword" and len(hits["hits"]) == 1,
            "keyword MCP search did not find exactly the seeded memory")
    hit = hits["hits"][0]
    require(hit["asset_id"] == asset_id and hit["version"] == 2 and hit["content"] == CONTENT,
            "MCP search returned wrong asset/version/content")
    for field in ("chunk_id", "source_event_id", "locator"):
        require(bool(hit.get(field)), "MCP search lost citation provenance")
    resolved = mcp.tool("context_resolve", {"query": QUERY, "budget_tokens": 4000})
    require(CONTENT in resolved["rendered_context"] and len(resolved["sources"]) == 1,
            "MCP resolve lost content or citation")
    require(resolved["sources"][0]["asset_id"] == asset_id, "MCP resolve cited the wrong asset")
    require(resolved["count"] == len(resolved["rendered_context"].encode("utf-8")) <= 4000,
            "MCP resolve byte budget changed")
    bundle = mcp.tool("context_bundle", {"query": QUERY, "budget": 4000})
    require(bundle["workspace_id"] == workspace_id and CONTENT in bundle["rendered_context"],
            "MCP bundle lost workspace scope or content")
    require(len(bundle["items"]) == 1 and bundle["items"][0]["route"] == "memory",
            "MCP bundle returned wrong evidence routes")
    require(bundle["items"][0]["provenance"]["asset_id"] == asset_id,
            "MCP bundle cited the wrong asset")
    require(bundle["used_bytes"] == len(bundle["rendered_context"].encode("utf-8")) <= 4000,
            "MCP bundle byte budget changed")
    current = mcp.tool("context_get", {"asset_id": asset_id})
    history = mcp.tool("context_get", {"asset_id": asset_id, "version": 1})
    require(current["version"] == 2 and current["content"] == CONTENT,
            "MCP get did not return the current version")
    require(history["version"] == 1 and history["content"] == OLD_CONTENT,
            "MCP get did not preserve historical version")
    empty = mcp.tool("context_search", {"query": "MCPDEFINITELYABSENT"})
    require(empty["hits"] == [], "missing query unexpectedly produced search hits")
    mcp.error("context_get", {"asset_id": BAD_INPUT}, -32602, "asset_id must be a UUID")
    mcp.error("context_get", {"asset_id": str(uuid.uuid4())}, -32600,
              "NOT_FOUND: resource not found")
    mcp.error("context_search", {"query": QUERY, "mode": BAD_INPUT}, -32600,
              "INVALID_ARGUMENT: query, mode or limit invalid")
    mcp.error("context_resolve", {"query": QUERY, "budget_tokens": 32001}, -32600,
              "INVALID_ARGUMENT: budget_tokens must be <=32000")
    mcp.error("context_bundle", {"query": QUERY, "mode": BAD_INPUT}, -32600,
              "INVALID_ARGUMENT: mode or budget invalid")
    # Exercise SDK parameter decoding as well as application-domain errors.
    mcp.invalid_arguments("context_search", {}, "failed to deserialize parameters: missing field `query`")
    mcp.error("context_missing", {}, -32602, "tool not found")
    mcp.invalid_arguments("context_search", {"query": QUERY, "unknown_flag": False})
    mcp.invalid_arguments("context_resolve", {"query": 42})
    mcp.invalid_arguments("context_bundle", {"query": QUERY, "budget": "wrong-type"})
    mcp.invalid_arguments("context_get", {"asset_id": asset_id, "version": "wrong-type"})
    # The same live connection must remain usable after errors.
    require(mcp.tool("context_get", {"asset_id": asset_id})["content"] == CONTENT,
            "MCP session failed to recover after invalid tool calls")


class Host(Child):
    def __init__(self, binary, env, token, forbidden):
        super().__init__([binary, "serve", "--bind", "127.0.0.1:0"], env, forbidden)
        self.token = token
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        try:
            listening = self.receive(STARTUP_TIMEOUT).get("listening", "")
            require(listening.startswith("127.0.0.1:"), "host did not bind an ephemeral loopback port")
            self.base = "http://" + listening
            deadline = time.monotonic() + STARTUP_TIMEOUT
            while True:
                require(self.process.poll() is None, "host exited before readiness")
                try:
                    self.request("/health/ready", authenticated=False)
                    break
                except (urllib.error.URLError, TimeoutError, ConnectionError):
                    require(time.monotonic() < deadline, "host readiness timed out")
                    time.sleep(0.2)
        except BaseException:
            self.close()
            raise

    def request(self, path, body=None, method=None, authenticated=True):
        headers = {}
        if authenticated:
            headers["Authorization"] = "Bearer " + self.token
        if body is not None:
            headers.update({"Content-Type": "application/json", "Idempotency-Key": uuid.uuid4().hex})
        request = urllib.request.Request(self.base + path, method=method, headers=headers,
                                         data=None if body is None else json.dumps(body).encode())
        with self.opener.open(request, timeout=TIMEOUT) as response:
            value = response.read(MAX_BYTES + 1)
        require(len(value) <= MAX_BYTES, "host response exceeded size limit")
        return json.loads(value)

    def publish(self, content):
        accepted = self.request("/v1/memories", {
            "fact_key": "mcp.smoke.policy", "content": content, "publish_if_authorized": True,
        })
        deadline = time.monotonic() + STARTUP_TIMEOUT
        while True:
            job = self.request("/v1/jobs/" + accepted["job_id"])
            if job["state"] == "completed":
                require(job["outcome"] == "published", "MCP fixture was not published")
                return accepted["asset_id"]
            require(job["state"] not in ("failed", "cancelled", "superseded"),
                    "MCP fixture publication failed")
            require(time.monotonic() < deadline, "MCP fixture publication timed out")
            time.sleep(0.2)


def invalid_startup(command, env, forbidden):
    # Keep stdin open: an unauthenticated server must fail before handshake, not
    # merely exit because the test disconnected it.
    with Mcp(command, env, forbidden) as mcp:
        try:
            code = mcp.process.wait(timeout=TIMEOUT)
        except subprocess.TimeoutExpired:
            raise RuntimeError("invalid/revoked token did not fail before initialization") from None
        require(code != 0, "invalid/revoked token was accepted at MCP startup")
        mcp.reader.join(timeout=TIMEOUT)
        require(mcp.lines.get(timeout=TIMEOUT) == b"" and mcp.lines.empty(),
                "unauthenticated MCP wrote protocol output")
        require("authentication required" in mcp.check_stderr(),
                "MCP startup failed for an unexpected reason")


def protocols(command, env, forbidden, expected_schemas=None):
    schemas = expected_schemas
    for requested, expected in PROTOCOLS:
        with Mcp(command, env, forbidden) as mcp:
            mcp.initialize(requested, expected)
            found = check_tools(mcp)
            if schemas is None:
                schemas = found
            require(schemas == found, "tool schemas changed across transport modes or protocol versions")
            mcp.shutdown()
    return schemas


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="built Origence executable (with local-storage)")
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    require(Path(binary).is_file(), "Origence binary does not exist; build it first")
    with tempfile.TemporaryDirectory(prefix="oc-mcp-smoke-") as directory:
        env = isolated_env(directory)
        forbidden = [directory, OLD_CONTENT, CONTENT, BAD_INPUT]
        bootstrap = subprocess.run([binary, "--offline", "workspace-create", "mcp-smoke"],
                                   env=env, cwd=directory, capture_output=True, timeout=STARTUP_TIMEOUT)
        require(bootstrap.returncode == 0, "MCP fixture workspace bootstrap failed")
        workspace = json.loads(bootstrap.stdout)
        token, workspace_id = workspace["token"], workspace["workspace_id"]
        bad_token = "oc_invalid_smoke_" + uuid.uuid4().hex
        forbidden.extend([token, bad_token])
        sanitized(bootstrap.stderr.decode("utf-8", errors="replace"), forbidden)
        env["OC_API_KEY"] = token
        with Host(binary, env, token, forbidden) as host:
            asset_id = host.publish(OLD_CONTENT)
            require(host.publish(CONTENT) == asset_id, "fixture update created a second asset")
            reader = host.request("/admin/keys", {"workspace_id": workspace_id, "role": "reader"})
            offline_reader = host.request("/admin/keys", {"workspace_id": workspace_id, "role": "reader"})
            forbidden.extend([reader["token"], offline_reader["token"]])
            # Remote MCP receives a deliberately separate, empty data
            # directory: forwarding must not accidentally open the host's DB.
            remote_env = dict(env, OC_API_KEY=reader["token"], OC_DATA_DIR=str(Path(directory) / "remote-unused"))
            Path(remote_env["OC_DATA_DIR"]).mkdir()
            command = [binary, "--server-url", host.base, "mcp"]
            schemas = protocols(command, remote_env, forbidden)
            invalid_startup(command, dict(remote_env, OC_API_KEY=bad_token), forbidden)
            with Mcp(command, remote_env, forbidden) as mcp:
                mcp.initialize("2025-03-26", "2025-03-26")
                exercise(mcp, asset_id, workspace_id)
                mcp.error("context_bundle", {"query": QUERY, "routes": [{"route": "session"}]},
                          -32600, "FORBIDDEN: permission denied")
                revoked = host.request("/admin/keys/" + reader["key_id"], method="DELETE")
                require(revoked.get("revoked") is True, "MCP reader key revocation failed")
                for name in TOOL_FIELDS:
                    arguments = {"asset_id": asset_id} if name == "context_get" else {"query": QUERY}
                    mcp.error(name, arguments, -32600, "UNAUTHENTICATED: authentication required")
                mcp.shutdown()
            invalid_startup(command, remote_env, forbidden)
            require(list(Path(remote_env["OC_DATA_DIR"]).iterdir()) == [],
                    "host-forwarding MCP unexpectedly created local storage")
            host.check_stderr()
        print("PASS: host-forwarding MCP, version negotiation, schemas, tools, safe errors, live revocation, EOF", flush=True)
        # The host is now stopped. Honor its exclusive DB lock; do not mutate
        # SQLite behind an offline MCP process to simulate key revocation.
        offline = [binary, "--offline", "mcp"]
        invalid_startup(offline, dict(env, OC_API_KEY=bad_token), forbidden)
        invalid_startup(offline, dict(env, OC_API_KEY=reader["token"]), forbidden)
        offline_env = dict(env, OC_API_KEY=offline_reader["token"])
        protocols(offline, offline_env, forbidden, schemas)
        with Mcp(offline, offline_env, forbidden) as mcp:
            mcp.initialize("2025-03-26", "2025-03-26")
            exercise(mcp, asset_id, workspace_id)
            mcp.error("context_bundle", {"query": QUERY, "routes": [{"route": "session"}]},
                      -32600, "FORBIDDEN: permission denied")
            mcp.shutdown()
        # Reopen after EOF proves the process released the exclusive DB lock.
        with Mcp(offline, env, forbidden) as mcp:
            mcp.initialize("2025-03-26", "2025-03-26")
            require(mcp.tool("context_get", {"asset_id": asset_id})["content"] == CONTENT,
                    "offline MCP failed to reopen after clean shutdown")
            mcp.shutdown()
        print("PASS: offline MCP, version negotiation, matching schemas/tools, safe errors, invalid/revoked startup, EOF/reopen")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, KeyError, ValueError, OSError, subprocess.SubprocessError, queue.Empty) as error:
        # Never dump child logs, environment, HTTP requests, or issued tokens.
        # Only our own contract failure messages are suitable for CI output.
        detail = str(error) if type(error) is RuntimeError else type(error).__name__
        raise SystemExit("FAIL: MCP smoke: " + detail) from None
