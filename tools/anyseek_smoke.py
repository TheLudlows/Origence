#!/usr/bin/env python3
"""Isolated real AnySeek + Origence HTTP MCP acceptance; no model calls.

Run with --anyseek-binary PATH and --test-executable PATH and/or --binary PATH.
Credentials and data live only in this invocation's environment/temp directory.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

from mcp_smoke import Host, Mcp, isolated_env, require


def call(base, path, key, body=None):
    headers = {"apikey": key}
    if body is not None:
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(base + path, headers=headers,
        data=None if body is None else json.dumps(body).encode())
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with opener.open(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"AnySeek fixture HTTP {error.code}") from None


def schema(dimension=None):
    fields = ["format", "kind", "tenant", "workspace", "chunk_id", "source_id", "source_version", "generation", "publication", "profile", "dimension"]
    result = [{"field": name, "field_type": "String", "store": True,
               "index_lexical": False, "index_vector": False, "filterable": True, "boost": 1.0}
              for name in fields]
    for field in ("terms", "summary_terms"):
        result.append({"field": field, "field_type": "Text", "store": True,
                       "index_lexical": True, "index_vector": False, "filterable": False, "boost": 1.0})
    for name, field_type, filterable in [
        ("payload", "Json", False),
        ("graph", "Json", False),
        ("endpoint_ids", "StringSet", True),
        ("graph_keys", "StringSet", True),
    ]:
        result.append({"field": name, "field_type": field_type, "store": True,
                       "index_lexical": False, "index_vector": False,
                       "filterable": filterable, "boost": 1.0})
    config = {"index_name": "origence-content-sha256-v1", "schema": result,
              "tokenizer": "TantivySimpleV1", "text_normalization": "None", "stop_words": "None"}
    if dimension is not None:
        require(1 <= dimension <= 4096, "invalid fixture vector dimension")
        result.append({"field": "vector", "field_type": "Json", "store": True,
                       "index_lexical": False, "index_vector": True, "filterable": False, "boost": 1.0})
        config["vector"] = {"dimensions": dimension}
    return config


def wait_ready(process, base, key):
    deadline = time.monotonic() + 60
    while True:
        require(process.poll() is None, "AnySeek fixture exited before readiness")
        try:
            call(base, "/api/v1/live", key)
            return
        except (RuntimeError, urllib.error.URLError, TimeoutError):
            require(time.monotonic() < deadline, "AnySeek readiness timed out")
            time.sleep(0.1)


def mcp_contract(binary, env, directory):
    env = dict(env, OC_DATA_DIR=str(Path(directory) / "origence"), OC_ENABLE_MODELS="false")
    advertised = subprocess.run([binary, "anyseek-schema"], env=env, capture_output=True, timeout=30)
    require(advertised.returncode == 0 and json.loads(advertised.stdout) == schema(),
            "CLI AnySeek schema differs from the workspace collection contract")
    bootstrap = subprocess.run([binary, "--offline", "workspace-create", "AnySeek smoke"],
        env=env, capture_output=True, timeout=60)
    require(bootstrap.returncode == 0, "Origence bootstrap failed")
    workspace = json.loads(bootstrap.stdout)
    token = workspace["token"]
    env["OC_API_KEY"] = token
    forbidden = [token, env["OC_ANYSEEK_API_KEY"]]
    with Host(binary, env, token, forbidden) as host:
        old = "AnySeek smoke policy requires token approval."
        asset = host.publish(old)
        run_id = str(uuid.uuid4())
        host.request("/v1/runs", {"run_id": run_id, "summary": "Verify retrieval diagnostics"})
        preparation_request = {"event_id": str(uuid.uuid4()), "context": {"query": "token approval"}}
        prepared = host.request(f"/v1/runs/{run_id}/context", preparation_request)
        observations = prepared["bundle"]["retrieval"]["requests"]
        require(observations and any(item["mode"] == "lexical" for item in observations),
                "real Run preparation omitted retrieval observations")
        for item in observations:
            backend = item["backend"]
            require(item["backend_request_id"] == backend["request_id"], "backend correlation differs")
            require(backend["analysis_fingerprint"] == item["checked_config"]["analysis_fingerprint"],
                    "backend analysis differs from admitted configuration")
            require(backend["format_version"] == 2 and
                    backend["embedding_space_sha256"] == item["checked_config"]["embedding_space_sha256"],
                    "backend embedding space differs from admitted configuration")
            require(backend["index_progress"]["sampling"] == "after_query_not_atomic_with_result",
                    "index progress lost its sampling boundary")
        require(host.request(f"/v1/runs/{run_id}/context", preparation_request) == prepared,
                "real Run retry changed recorded preparation observations")
        recorded = host.request(f"/v1/runs/{run_id}")["events"]
        require(recorded[0]["report"]["retrieval"] == prepared["bundle"]["retrieval"],
                "real Run trace lost observations")
        reader = host.request("/admin/keys", {"workspace_id": workspace["workspace_id"], "role": "reader"})
        client = Mcp(host.base, reader["token"], forbidden + [reader["token"]])
        client.initialize("2025-03-26", "2025-03-26")
        terms = {"query": "token approval"}
        http_hits = host.request("/v1/search", terms)["hits"]
        mcp_hits = client.tool("context_search", terms)["hits"]
        require([hit["asset_id"] for hit in http_hits] == [asset], "HTTP keyword search missed AnySeek publication")
        require([hit["asset_id"] for hit in mcp_hits] == [asset], "MCP keyword search missed AnySeek publication")
        require(mcp_hits[0]["locator"] and mcp_hits[0]["locator"] == http_hits[0]["locator"] and
                mcp_hits[0]["source_event_id"] == http_hits[0]["source_event_id"], "HTTP and MCP source citations differ")
        for query in ("token fixtureabsent", "fixtureabsent fixturemissing"):
            require(host.request("/v1/search", {"query": query})["hits"] == [],
                    f"HTTP keyword search matched a query with missing terms: {query}")
            require(client.tool("context_search", {"query": query})["hits"] == [],
                    f"MCP keyword search matched a query with missing terms: {query}")
        require(client.tool("context_get", {"asset_id": asset})["content"] == old, "MCP get lost source content")
        require(host.publish("AnySeek smoke policy now requires token review.") == asset, "update changed asset ID")
        require(host.request(f"/v1/runs/{run_id}/context", preparation_request) == prepared,
                "updated corpus changed an existing preparation receipt")
        require(client.tool("context_search", {"query": "approval"})["hits"] == [], "superseded version remained searchable")
        require(client.tool("context_get", {"asset_id": asset, "version": 1})["content"] == old, "historical source changed")
        host.request("/admin/keys/" + reader["key_id"], method="DELETE")
        client.unauthorized("tools/call", {"name": "context_search", "arguments": {"query": "token"}})
    # Reopen the same authorities and projection through its HTTP MCP endpoint.
    with Host(binary, env, token, forbidden) as host:
        require(host.request(f"/v1/runs/{run_id}")["events"] == recorded, "reopen changed recorded Run observations")
        require(host.request(f"/v1/runs/{run_id}/context", preparation_request) == prepared,
                "reopen changed the private preparation snapshot")
        client = Mcp(host.base, token, forbidden)
        client.initialize("2025-03-26", "2025-03-26")
        require(client.tool("context_search", {"query": "token review"})["hits"][0]["asset_id"] == asset,
                "reopen lost AnySeek publication")
    print("PASS: real AnySeek -> Origence HTTP and MCP default keyword AND, read, citations, update, history, key revocation and reopen; Run diagnostic correlation, retry and durability", flush=True)
    return env, token, asset, old


def embedded_contract(binary):
    with tempfile.TemporaryDirectory(prefix="origence embedded anyseek ") as directory:
        env = isolated_env(directory)
        for name in ("OC_ANYSEEK_URL", "OC_ANYSEEK_API_KEY", "MASTER_KEY_SECRET"):
            env.pop(name, None)
        env.update(ANYSEEK_STORAGE_BACKEND="s3", PATH="")
        serve_args = ["--local"]
        workspace = subprocess.run([binary, "--offline", "workspace-create", "local smoke"],
                                   env=env, check=True, capture_output=True, timeout=30)
        token = json.loads(workspace.stdout)["token"]
        old = "Embedded local search requires approval."
        new = "Embedded local search requires review."
        with Host(binary, env, token, [token], serve_args) as host:
            asset = host.publish(old)
            require(host.publish(new) == asset, "embedded local update changed asset ID")
            run_id = str(uuid.uuid4())
            host.request("/v1/runs", {"run_id": run_id, "summary": "Verify SDK retrieval"})
            request = {"event_id": str(uuid.uuid4()), "context": {"query": "embedded review"}}
            prepared = host.request(f"/v1/runs/{run_id}/context", request)
            observations = prepared["bundle"]["retrieval"]["requests"]
            require(observations and any(item["mode"] == "lexical" and item["backend"] for item in observations)
                    and all(item["http_status"] is None for item in observations), "SDK diagnostics used an HTTP status or omitted retrieval")
            require(host.request(f"/v1/runs/{run_id}/context", request) == prepared, "SDK replay changed retrieval observations")
            host.process.send_signal(signal.SIGTERM)
            require(host.process.wait(timeout=45) == 0, "embedded local shutdown failed")
            host.check_stderr()

        with Host(binary, env, token, [token], serve_args) as host:
            require(host.request(f"/v1/assets/{asset}?version=1")["content"] == old,
                    "embedded local restart lost historical content")
            require(host.request(f"/v1/assets/{asset}")["content"] == new,
                    "embedded local restart lost current content")
            found = host.request("/v1/search", {"query": "embedded review", "mode": "keyword"})
            require(found["hits"] and found["hits"][0]["asset_id"] == asset,
                    "embedded local restart lost the lexical index")
            host.process.send_signal(signal.SIGTERM)
            require(host.process.wait(timeout=45) == 0, "reopened embedded local shutdown failed")
            host.check_stderr()

        index = Path(directory) / "anyseek"
        index.rename(index.with_name("retained-catalog"))
        missing = subprocess.run([binary, "serve", "--bind", "127.0.0.1:0", *serve_args],
                                 env=env, capture_output=True, timeout=10)
        require(missing.returncode != 0 and b"data directory is missing" in missing.stderr,
                "embedded startup accepted a missing Catalog for an existing binding")
        require(not index.exists(), "embedded startup recreated a lost Catalog")

    print("PASS: embedded AnySeek SDK publication, diagnostics, replay, restart, and refusal to recreate a lost Catalog", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--anyseek-binary")
    parser.add_argument("--test-executable", action="append", default=[])
    parser.add_argument("--cargo-tests", action="store_true", help="run the real-backend Rust target through Cargo")
    parser.add_argument("--binary")
    parser.add_argument("--embedded", action="store_true", help="run the in-process local startup and restart contract on Unix")
    parser.add_argument("--run", nargs=argparse.REMAINDER, help="run another isolated smoke/eval command against this fixture")
    args = parser.parse_args()
    if args.embedded:
        require(args.binary, "--embedded needs an Origence binary")
        embedded_contract(str(Path(args.binary).resolve()))
        return
    require(args.anyseek_binary, "external mode needs --anyseek-binary")
    require(args.test_executable or args.binary or args.cargo_tests or args.run, "provide a test executable, Cargo tests or Origence binary")
    with tempfile.TemporaryDirectory(prefix="origence-anyseek-") as directory:
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        secret = secrets.token_hex(32)
        master = base64.b64encode(hashlib.sha256(secret.encode()).digest()).decode()
        key = master
        env = dict(os.environ, MASTER_KEY_SECRET=secret, RUST_LOG="error", ANYSEEK_STORAGE_BACKEND="local",
                   NO_PROXY="127.0.0.1,localhost,::1", no_proxy="127.0.0.1,localhost,::1")
        base = f"http://127.0.0.1:{port}"
        with open(Path(directory) / "anyseek.log", "wb") as log:
            process = subprocess.Popen([str(Path(args.anyseek_binary).resolve()),
                "local_ip=127.0.0.1", f"local_port={port}", f"index_path={directory}/anyseek"],
                env=env, stdout=log, stderr=log)
            try:
                wait_ready(process, base, key)
                env.update(OC_FULL_TEXT_BACKEND="anyseek", OC_ANYSEEK_URL=base, OC_ANYSEEK_API_KEY=key)
                env.update(ORIGENCE_TEST_ANYSEEK_URL=base, ORIGENCE_TEST_ANYSEEK_API_KEY=key)
                # Workspace collections are provisioned by Origence on first use.
                env.pop("MASTER_KEY_SECRET", None)
                for executable in args.test_executable:
                    subprocess.run([str(Path(executable).resolve()), "--ignored", "--nocapture", "--test-threads=1"],
                                   env=env, check=True, timeout=300)
                if args.cargo_tests:
                    subprocess.run(["cargo", "test", "--locked", "--no-default-features", "--features", "host",
                                    "--test", "anyseek", "--", "--ignored", "--nocapture", "--test-threads=1"],
                                   env=env, check=True, timeout=300, cwd=Path(__file__).resolve().parent.parent)
                if args.run:
                    subprocess.run(args.run, env=env, check=True, timeout=600)
                if args.binary:
                    binary = str(Path(args.binary).resolve())
                    probe_env, token, asset, original = mcp_contract(binary, env, directory)
                    process.send_signal(signal.SIGTERM)
                    process.wait(timeout=30)
                    process = subprocess.Popen(process.args, env=dict(env, MASTER_KEY_SECRET=secret), stdout=log, stderr=log)
                    wait_ready(process, base, key)
                    with Host(binary, probe_env, token, [token, key]) as host:
                        require(host.request(f"/v1/assets/{asset}?version=1")["content"] == original,
                                "AnySeek restart lost historical stored content")
                        current = host.request(f"/v1/assets/{asset}")
                        require(current["content"] == "AnySeek smoke policy now requires token review.",
                                "AnySeek restart lost current stored content")
                        found = host.request("/v1/search", {"query":"token review", "mode":"keyword"})
                        require(found["hits"] and found["hits"][0]["asset_id"] == asset,
                                "AnySeek restart lost the matching content index")
                        source = found["hits"][0]["source_event_id"]
                        host.request(f"/v1/events/{source}", {}, method="DELETE")
                        terms = {"query": "token review"}
                        require(host.request("/v1/search", terms)["hits"] == [],
                                "HTTP keyword search returned a withdrawn source")
                        client = Mcp(host.base, token, [token, key])
                        client.initialize("2025-03-26", "2025-03-26")
                        require(client.tool("context_search", terms)["hits"] == [],
                                "MCP keyword search returned a withdrawn source")
                    print("PASS: AnySeek process restart preserves historical/current body and its index; HTTP and MCP exclude withdrawn sources", flush=True)
            finally:
                if process.poll() is None:
                    process.send_signal(signal.SIGTERM)
                    try:
                        process.wait(timeout=30)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()


if __name__ == "__main__":
    main()
