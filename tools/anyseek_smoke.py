#!/usr/bin/env python3
"""Isolated real AnySeek + Origence HTTP/stdio MCP acceptance; no model calls.

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

from mcp_smoke import Host, Mcp, require


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
    fields = ["format", "tenant", "workspace", "chunk_id", "source_id", "source_version", "generation", "publication", "profile", "dimension"]
    result = [{"field": name, "field_type": "String", "store": True,
               "index_lexical": False, "index_vector": False, "filterable": True, "boost": 1.0}
              for name in fields]
    result.append({"field": "terms", "field_type": "Text", "store": True,
                   "index_lexical": True, "index_vector": False, "filterable": False, "boost": 1.0})
    config = {"index_name": "origence-search-sha256-v2", "schema": result,
              "tokenizer": "TantivySimpleV1", "text_normalization": "None", "stop_words": "None"}
    if dimension is not None:
        require(1 <= dimension <= 4096, "invalid fixture vector dimension")
        result.append({"field": "vector", "field_type": "Json", "store": True,
                       "index_lexical": False, "index_vector": True, "filterable": False, "boost": 1.0})
        config["vector"] = {"dimensions": dimension}
    return config


def mcp_contract(binary, env, directory):
    env = dict(env, OC_DATA_DIR=str(Path(directory) / "origence"), OC_ENABLE_MODELS="false")
    advertised = subprocess.run([binary, "anyseek-schema"], env=env, capture_output=True, timeout=30)
    require(advertised.returncode == 0 and json.loads(advertised.stdout) == schema(),
            "CLI AnySeek schema differs from the provisioned contract")
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
        reader = host.request("/admin/keys", {"workspace_id": workspace["workspace_id"], "role": "reader"})
        mcp_env = dict(env, OC_API_KEY=reader["token"])
        with Mcp([binary, "--server-url", host.base, "mcp"], mcp_env, forbidden + [reader["token"]]) as client:
            client.initialize("2025-03-26", "2025-03-26")
            found = client.tool("context_search", {"query": "token approval", "mode": "keyword"})
            require(found["hits"] and found["hits"][0]["asset_id"] == asset, "MCP search missed AnySeek publication")
            require(found["hits"][0]["locator"], "MCP search omitted source locator")
            require(client.tool("context_get", {"asset_id": asset})["content"] == old, "MCP get lost source content")
            require(host.publish("AnySeek smoke policy now requires token review.") == asset, "update changed asset ID")
            require(client.tool("context_search", {"query": "approval"})["hits"] == [], "superseded version remained searchable")
            require(client.tool("context_get", {"asset_id": asset, "version": 1})["content"] == old, "historical source changed")
            host.request("/admin/keys/" + reader["key_id"], method="DELETE")
            client.error("context_search", {"query": "token"}, -32600, "UNAUTHENTICATED: authentication required")
            client.shutdown()
    # Reopen the same authorities and projection with a fresh MCP process.
    with Host(binary, env, token, forbidden) as host:
        with Mcp([binary, "--server-url", host.base, "mcp"], env, forbidden) as client:
            client.initialize("2025-03-26", "2025-03-26")
            require(client.tool("context_search", {"query": "token review"})["hits"][0]["asset_id"] == asset,
                    "reopen lost AnySeek publication")
            client.shutdown()
    print("PASS: real AnySeek -> Origence HTTP/stdio MCP search, read, citations, update, history, key revocation and reopen", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--anyseek-binary", required=True)
    parser.add_argument("--test-executable", action="append", default=[])
    parser.add_argument("--cargo-tests", action="store_true", help="run the real-backend Rust target through Cargo")
    parser.add_argument("--binary")
    parser.add_argument("--run", nargs=argparse.REMAINDER, help="run another isolated smoke/eval command against this fixture")
    args = parser.parse_args()
    require(args.test_executable or args.binary or args.cargo_tests or args.run, "provide a test executable, Cargo tests or Origence binary")
    with tempfile.TemporaryDirectory(prefix="origence-anyseek-") as directory:
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        secret = secrets.token_hex(32)
        master = base64.b64encode(hashlib.sha256(secret.encode()).digest()).decode()
        key = base64.b64encode(secrets.token_bytes(32)).decode()
        env = dict(os.environ, MASTER_KEY_SECRET=secret, RUST_LOG="error", ANYSEEK_STORAGE_BACKEND="local",
                   NO_PROXY="127.0.0.1,localhost,::1", no_proxy="127.0.0.1,localhost,::1")
        base = f"http://127.0.0.1:{port}"
        with open(Path(directory) / "anyseek.log", "wb") as log:
            process = subprocess.Popen([str(Path(args.anyseek_binary).resolve()),
                "local_ip=127.0.0.1", f"local_port={port}", f"index_path={directory}/anyseek"],
                env=env, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 60
                while True:
                    require(process.poll() is None, "AnySeek fixture exited before readiness")
                    try:
                        call(base, "/api/v1/live", key)
                        break
                    except (RuntimeError, urllib.error.URLError, TimeoutError):
                        require(time.monotonic() < deadline, "AnySeek readiness timed out")
                        time.sleep(0.1)
                def operation():
                    return {"epoch": int(time.time() // 86400), "nonce": "smoke-" + uuid.uuid4().hex}
                call(base, "/api/v1/tenants", master, {"operation_id": operation(), "tenant_id": "origence"})
                call(base, "/api/v1/apikey", master, {"operation_id": operation(), "tenant_id": "origence", "key_id": "origence", "apikey_base64": key})
                call(base, "/api/v1/index", key, {"operation_id": operation(), "collection_id": "origence-lexical", "config": schema()})
                call(base, "/api/v1/index", key, {"operation_id": operation(), "collection_id": "origence-vector", "config": schema(2)})
                env.update(OC_ANYSEEK_VECTOR_COLLECTION="origence-vector", OC_FULL_TEXT_BACKEND="anyseek", OC_ANYSEEK_URL=base,
                           OC_ANYSEEK_API_KEY=key, OC_ANYSEEK_COLLECTION="origence-lexical")
                env.update(ORIGENCE_TEST_ANYSEEK_URL=base, ORIGENCE_TEST_ANYSEEK_API_KEY=key,
                           ORIGENCE_TEST_ANYSEEK_COLLECTION="origence-lexical")
                # Never send bootstrap master credentials to Origence or test processes.
                env.pop("MASTER_KEY_SECRET", None)
                for executable in args.test_executable:
                    subprocess.run([str(Path(executable).resolve()), "--ignored", "--nocapture", "--test-threads=1"],
                                   env=env, check=True, timeout=300)
                if args.cargo_tests:
                    subprocess.run(["cargo", "test", "--locked", "--test", "anyseek", "--", "--ignored", "--nocapture", "--test-threads=1"],
                                   env=env, check=True, timeout=300, cwd=Path(__file__).resolve().parent.parent)
                if args.run:
                    subprocess.run(args.run, env=env, check=True, timeout=600)
                if args.binary:
                    mcp_contract(str(Path(args.binary).resolve()), env, directory)
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
