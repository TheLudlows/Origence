#!/usr/bin/env python3
"""Exercise private archives through the Origence CLI, HTTP and MCP entry points."""
import argparse
import base64
import concurrent.futures
import contextlib
import hashlib
import http.server
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
import uuid

from anyseek_smoke import wait_ready
from mcp_smoke import Host, Mcp, isolated_env

MARKER = "privatearchiveneedle"
ANSWER = "Synthetic archive answer Café 审批 🦀"
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def request(base, path, method="GET", token=None, body=None, key=None, headers=None, upstream_headers=None):
    headers = dict(headers or {})
    if token is not None:
        headers["Authorization"] = "Bearer " + token
    if body is not None:
        headers["Content-Type"] = "application/json"
        headers["Idempotency-Key"] = key or uuid.uuid4().hex
    data = json.dumps(body).encode() if body is not None else None
    call = urllib.request.Request(base + path, method=method, data=data, headers=headers)
    try:
        with OPENER.open(call, timeout=40) as response:
            if upstream_headers is not None:
                upstream_headers.update(response.headers.items())
            return response.status, json.load(response)
    except urllib.error.HTTPError as error:
        if upstream_headers is not None:
            upstream_headers.update(error.headers.items())
        data = error.read()
        return error.code, json.loads(data) if data.startswith(b"{") else None


def expect(result, status):
    assert result[0] == status, f"expected HTTP {status}, received {result[0]}"
    return result[1]


class Peer(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def read_json(self):
        return json.loads(self.rfile.read(int(self.headers["Content-Length"])))

    def reply(self, status, value, upstream_headers=None):
        data = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        for name, value in (upstream_headers or {}).items():
            if name.lower() in {"x-anyseek-request-id", "x-anyseek-error-code"}:
                self.send_header(name, value)
        self.end_headers()
        with contextlib.suppress(BrokenPipeError, ConnectionResetError):
            self.wfile.write(data)

    def do_GET(self):
        self.handle_call()

    def do_POST(self):
        self.handle_call()


class Model(Peer):
    def handle_call(self):
        body = self.read_json()
        assert self.path == "/v1/embeddings"
        text = body["input"].lower()
        vector = [1.0, 0.0, 0.0] if MARKER in text or "semantic paraphrase" in text else [0.0, 1.0, 0.0]
        self.reply(200, {"data": [{"embedding": vector}]})


class Proxy(Peer):
    def handle_call(self):
        peer = self.server
        body = self.read_json() if self.command == "POST" else None
        private = "/pr_" in self.path
        # Fault modes are armed for one request on the isolated peer.
        replace = self.path.endswith("/group/replace") and body
        put = replace and body["records"]
        delete = replace and not body["records"]
        action = peer.action
        if private and delete and action == "delete_before":
            peer.action = None
            self.reply(503, {})
            return
        if private and put and action == "before":
            peer.action = None
            self.reply(503, {})
            return
        if private and put and action == "late":
            peer.action = None
            peer.entered.set()
            assert peer.release.wait(20)
        if private and action == "block_read":
            self.reply(503, {})
            return
        if private and action == "missing_group" and "/group?group_id=" in self.path:
            self.reply(404, {})
            return
        headers = {name: value for name in ("apikey", "X-AnySeek-Tenant", "X-AnySeek-Request-Id")
                   if (value := self.headers.get(name)) is not None}
        upstream_headers = {}
        status, value = request(peer.upstream, self.path, self.command, body=body, headers=headers,
                                upstream_headers=upstream_headers)
        if private and delete and action == "delete_after" and status == 200:
            peer.action = None
            self.reply(503, {})
            return
        if private and put and action == "after" and status == 200:
            peer.action = "block_read"
            self.reply(503, {})
            return
        if private and status == 200 and self.path.endswith("/query") and "body" in body.get("fields", []):
            if action == "hold_read":
                peer.action = None
                peer.entered.set()
                assert peer.release.wait(20)
            if action == "missing":
                value["hits"] = [hit for hit in value["hits"] if hit["record"]["document"]["kind"] != "search"]
                value["count_total"] = len(value["hits"])
            if action == "corrupt":
                for hit in value["hits"]:
                    doc = hit["record"]["document"]
                    if doc["kind"] == "body":
                        doc["body"]["answer"] = "Corrupted isolated fixture"
        self.reply(status, value, upstream_headers)


@contextlib.contextmanager
def peer(handler):
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server, f"http://127.0.0.1:{server.server_port}"
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


@contextlib.contextmanager
def native(binary, directory, port, secret):
    env = dict(os.environ, MASTER_KEY_SECRET=secret, RUST_LOG="error", ANYSEEK_STORAGE_BACKEND="local")
    key = base64.b64encode(hashlib.sha256(secret.encode()).digest()).decode()
    base = f"http://127.0.0.1:{port}"
    with tempfile.TemporaryFile() as log:
        process = subprocess.Popen([binary, "local_ip=127.0.0.1", f"local_port={port}",
                                    f"index_path={directory}"], env=env, stdout=log, stderr=log)
        try:
            wait_ready(process, base, key)
            yield base, key
        finally:
            process.terminate()
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            log.seek(0)
            data = log.read()
            assert MARKER.encode() not in data and key.encode() not in data


def command(binary, env, *args, ok=True):
    child = subprocess.run([binary, "--offline", *args], env=env, capture_output=True, timeout=120)
    assert (child.returncode == 0) == ok, "offline command status did not match"
    assert MARKER.encode() not in child.stderr
    return json.loads(child.stdout) if ok else None


def new_turn(question, answer=ANSWER):
    return {"turn_id": str(uuid.uuid4()), "question": question, "answer": answer, "used_evidence": []}


@contextlib.contextmanager
def database(directory):
    with contextlib.closing(sqlite3.connect(directory / "context.db", isolation_level=None)) as db:
        yield db


def check(directory, markers):
    with database(directory) as db:
        rows = db.execute("SELECT content FROM oc_events WHERE kind='session_turn'").fetchall()
        assert rows and all(row[0] == "" for row in rows)
    for path in directory.rglob("*"):
        if path.is_file() and "anyseek" not in path.relative_to(directory).parts:
            data = path.read_bytes()
            assert not any(marker.encode() in data for marker in markers), f"private body in {path.name}"


def run_contract(binary, root, model, anyseek):
    directory = root / "origence"
    directory.mkdir()
    env = isolated_env(str(directory))
    env.update(OC_ENABLE_MODELS="true", OC_EMBEDDING_BASE_URL=model + "/v1",
               OC_EMBEDDING_MODEL="synthetic-vector", OC_EMBEDDING_DIMENSION="3")
    env.pop("OC_ANYSEEK_URL", None)
    env.pop("OC_ANYSEEK_API_KEY", None)
    args = ["--local"] if anyseek is None else []
    forbidden = [MARKER, ANSWER]
    with contextlib.ExitStack() as stack:
        if anyseek:
            secret = secrets.token_hex(32)
            with socket.socket() as sock:
                sock.bind(("127.0.0.1", 0))
                port = sock.getsockname()[1]
            backend_dir = root / "native"
            backend = stack.enter_context(native(anyseek, backend_dir, port, secret))
            proxy, proxy_url = stack.enter_context(peer(Proxy))
            proxy.upstream, key = backend
            proxy.action = None
            proxy.entered = threading.Event()
            proxy.release = threading.Event()
            env.update(OC_ANYSEEK_URL=proxy_url, OC_ANYSEEK_API_KEY=key)
            forbidden.append(key)
        owner = command(binary, env, "workspace-create", "private fixture")
        foreign = command(binary, env, "workspace-create", "second workspace", "--in-tenant-of",
                          owner["workspace_id"])
        token = owner["token"]
        forbidden.extend([token, foreign["token"]])
        turn = new_turn(MARKER + " Café question")
        control = new_turn("Unrelated fixture orchard")
        rich = new_turn("界" * 21845 + "x", "\u0001" * 65535 + "y")
        assert len(rich["question"].encode()) == 65536 and len(rich["answer"].encode()) == 65536

        def start(binary=binary, env=env):
            return Host(binary, env, token, forbidden, args)

        with start() as host:
            session = host.request("/v1/session")["session_id"]
            route = f"/v1/sessions/{session}/turns"
            saved = []
            for item in (turn, control, rich):
                saved.append(host.request(route, item))
                time.sleep(0.005)
            with database(directory) as db:
                assert db.execute("SELECT COUNT(*) FROM oc_session_turns").fetchone() == (0,)
                assert not db.execute(
                    "SELECT 1 FROM sqlite_schema WHERE name IN ('oc_private_archives','oc_private_archive_migration')"
                ).fetchall()
            context = {"query": MARKER, "routes": [{"route": "session"}]}
            run_id, event_id = str(uuid.uuid4()), str(uuid.uuid4())
            host.request("/v1/runs", {"run_id": run_id, "summary": "Synthetic replay"})
            prepare = f"/v1/runs/{run_id}/context"
            run_request = {"event_id": event_id, "context": context}
            prepared = host.request(prepare, run_request)
            sequence = prepared["preparation"]["sequence"]
            prefix_path = f"/v1/runs/{run_id}/prefix?through_sequence={sequence}"
            prefix = host.request(prefix_path)
            verification_path = f"/v1/runs/{run_id}/verifications"
            verification = {
                "verification_id": str(uuid.uuid4()), "through_sequence": sequence,
                "prefix_digest": prefix["digest"], "verifier_label": "Synthetic verifier",
                "assessed_at_ms": 0, "conclusion": "passed",
                "basis_summary": "Synthetic archive reference", "basis_sequences": [sequence],
                "reported_skill_version": None,
            }
            recorded_verification = host.request(verification_path, verification)

        with start() as host:
            listed = host.request(route + "?limit=2")
            assert [item["turn_id"] for item in listed["turns"]] == [turn["turn_id"], control["turn_id"]]
            listing = host.request(route + f"?after={listed['next_after']}")
            assert listing["turns"][0]["question"] == rich["question"]
            assert listing["turns"][0]["answer"] == rich["answer"]
            assert host.request(route + "/" + rich["turn_id"]) == listing["turns"][0]
            assert host.request(route, turn) == saved[0]
            original = host.request(f"/v1/events/{saved[0]['source_event_id']}")
            assert json.loads(original["content"]) == {"question": turn["question"], "answer": turn["answer"]}
            mcp = Mcp(host.base, token, forbidden)
            mcp.initialize("2025-11-25", "2025-11-25")
            assert mcp.tool("context_turn_get", {"turn_id": turn["turn_id"]})["answer"] == ANSWER
            assert mcp.tool("context_turn_archive", {"turn": turn}) == saved[0]
            expect(request(host.base, route, "POST", token, dict(turn, answer="Changed synthetic answer")), 409)
            assert host.request(prepare, run_request) == prepared
            assert host.request(prefix_path) == prefix
            assert host.request(verification_path, verification) == recorded_verification
            source = uuid.UUID(saved[0]["source_event_id"]).bytes
            with database(directory) as db:
                db.execute("UPDATE oc_events SET kind='knowledge' WHERE id=?", (source,))
            try:
                assert host.request(route + "/" + turn["turn_id"])["answer"] == ANSWER
                assert host.request("/v1/context", context)["items"][0]["provenance"]["turn_id"] == turn["turn_id"]
            finally:
                with database(directory) as db:
                    db.execute("UPDATE oc_events SET kind='session_turn' WHERE id=?", (source,))
            for mode, query in (("keyword", MARKER), ("vector", "semantic paraphrase"), ("hybrid", MARKER)):
                found = host.request("/v1/context", dict(context, query=query, mode=mode))
                assert found["items"][0]["provenance"]["turn_id"] == turn["turn_id"]
                assert len({item["provenance"]["turn_id"] for item in found["items"]}) == len(found["items"])
            shared = host.request("/v1/memories", {"fact_key": "isolated.shared", "content": "Workspace shared fixture banana. Café."})
            wait_for(lambda: host.request(f"/v1/jobs/{shared['job_id']}")["state"] == "completed")
            assert host.request("/v1/search", {"query": "banana"})["hits"]
            empty = host.request("/v1/search", {"query": MARKER})
            assert empty["hits"] == []
            writer = host.request("/admin/keys", {"workspace_id": owner["workspace_id"], "role": "writer"})
            for other in [writer["token"], foreign["token"]]:
                expect(request(host.base, route + "/" + turn["turn_id"], token=other), 404)
                other_results = expect(request(host.base, "/v1/context", "POST", other, context), 200)
                assert other_results["items"] == []
            # Multiple client submissions settle to the first stable source ID.
            concurrent_turn = new_turn("Concurrent synthetic turn")
            with concurrent.futures.ThreadPoolExecutor(max_workers=5) as pool:
                results = list(pool.map(lambda _: host.request(route, concurrent_turn), range(5)))
            assert all(result == results[0] for result in results)
            source_path = f"/v1/events/{results[0]['source_event_id']}"
            assert json.loads(host.request(source_path)["content"])["question"] == concurrent_turn["question"]
            deleted = expect(request(host.base, source_path, "DELETE", token, {}, "withdraw-native-source"), 200)
            assert expect(request(host.base, source_path, "DELETE", token, {}, "withdraw-native-source"), 200) == deleted
            expect(request(host.base, route + "/" + concurrent_turn["turn_id"], token=token), 404)
            with database(directory) as db:
                assert db.execute("SELECT 1 FROM oc_session_turns WHERE turn_id=?",
                                  (uuid.UUID(concurrent_turn["turn_id"]).bytes,)).fetchone() is None

            if anyseek:
                # The directory hit and Run proof still exist. Losing the
                # native group is an integrity error on all read paths.
                proxy.action = "missing_group"
                try:
                    expect(request(host.base, f"/v1/events/{saved[0]['source_event_id']}", token=token), 503)
                    expect(request(host.base, route + "/" + turn["turn_id"], token=token), 503)
                    expect(request(host.base, "/v1/context", "POST", token, context), 503)
                finally:
                    proxy.action = None
                mixed_run = str(uuid.uuid4())
                host.request("/v1/runs", {"run_id": mixed_run, "summary": "Synthetic mixed replay"})
                mixed_path = f"/v1/runs/{mixed_run}/context"
                mixed_request = {
                    "event_id": str(uuid.uuid4()),
                    "context": {"query": "Café", "routes": [{"route": "session"}, {"route": "memory"}]},
                }
                mixed = host.request(mixed_path, mixed_request)
                assert {i["provenance"]["type"] for i in mixed["bundle"]["items"]} == {"asset_chunk", "session_turn"}
                assert host.request(mixed_path, mixed_request) == mixed
                proxy.entered.clear()
                proxy.release.clear()
                proxy.action = "hold_read"
                with concurrent.futures.ThreadPoolExecutor() as pool:
                    inflight = pool.submit(request, host.base, mixed_path, "POST", token, mixed_request)
                    try:
                        assert proxy.entered.wait(15), "replay did not reach private body read"
                        expect(request(host.base, f"/v1/events/{shared['source_event_id']}",
                                       "DELETE", token, {}), 200)
                    finally:
                        proxy.release.set()
                    expect(inflight.result(), 404)
                proxy.entered.clear()
                proxy.release.clear()
                assert host.request(route + "/" + turn["turn_id"])["answer"] == ANSWER

                disappearing = new_turn("withdrawalduringsearchneedle")
                host.request(route, disappearing)
                search = dict(context, query=disappearing["question"])
                assert host.request("/v1/context", search)["items"][0]["provenance"]["turn_id"] == disappearing["turn_id"]
                proxy.action = "hold_read"
                with concurrent.futures.ThreadPoolExecutor() as pool:
                    inflight = pool.submit(request, host.base, "/v1/context", "POST", token, search)
                    try:
                        assert proxy.entered.wait(15), "context did not reach private body read"
                        expect(request(host.base, route + "/" + disappearing["turn_id"], "DELETE", token), 200)
                    finally:
                        proxy.release.set()
                    result = expect(inflight.result(), 200)
                    assert result["items"] == []
                    assert result["routes"][0]["invalidated_before_render"] == 1
                proxy.entered.clear()
                proxy.release.clear()
                proxy.action = "before"
                delayed = new_turn("unwrittenpendingneedle")
                expect(request(host.base, route, "POST", token, delayed, "unwritten"), 503)
                expect(request(host.base, route + "/" + delayed["turn_id"], token=token), 404)
                assert expect(request(host.base, "/v1/context", "POST", token,
                                      dict(context, query="unwrittenpendingneedle")), 200)["items"] == []
                # An unwritten native turn has no SQLite archive row to promote.
                with database(directory) as db:
                    assert db.execute("SELECT 1 FROM oc_session_turns WHERE turn_id=?",
                                      (uuid.UUID(delayed["turn_id"]).bytes,)).fetchone() is None
                expect(request(host.base, route, "POST", token, delayed, "unwritten"), 200)

                for action in ["missing", "corrupt"]:
                    proxy.action = action
                    expect(request(host.base, route + "/" + turn["turn_id"], token=token), 503)
                proxy.action = "late"
                late = new_turn("latependingneedle")
                with concurrent.futures.ThreadPoolExecutor() as pool:
                    delayed_call = pool.submit(request, host.base, route, "POST", token, late)
                    assert proxy.entered.wait(15)
                    expect(request(host.base, route + "/" + late["turn_id"], "DELETE", token), 200)
                    proxy.release.set()
                    assert delayed_call.result()[0] in (404, 503)
                expect(request(host.base, route + "/" + late["turn_id"], token=token), 404)
                expect(request(host.base, route, "POST", token, late), 404)
                proxy.action = "after"
                lost = new_turn("lostreceiptneedle")
                expect(request(host.base, route, "POST", token, lost, "lostreceipt"), 503)
                proxy.action = None

        with start() as host:
            if anyseek:
                found = host.request("/v1/context", dict(context, query="lostreceiptneedle"))
                assert found["items"][0]["provenance"]["turn_id"] == lost["turn_id"]
                recovered = host.request(route + "/" + lost["turn_id"])
                assert recovered["question"] == lost["question"]
                assert host.request(route, lost)["source_event_id"] == recovered["source_event_id"]
            rotate_path = f"/admin/keys/{owner['key_id']}/rotate"
            if anyseek:
                proxy.entered.clear()
                proxy.release.clear()
                proxy.action = "hold_read"
                with concurrent.futures.ThreadPoolExecutor() as pool:
                    inflight = pool.submit(request, host.base, route + "/" + turn["turn_id"], "GET", token)
                    assert proxy.entered.wait(15)
                    rotation = host.request(rotate_path, {})
                    proxy.release.set()
                    expect(inflight.result(), 403)
            else:
                rotation = host.request(rotate_path, {})
            replacement = rotation["token"]
            forbidden.append(replacement)
            expect(request(host.base, route + "/" + turn["turn_id"], token=token), 401)
            rotated = expect(request(host.base, route + "/" + turn["turn_id"], token=replacement), 200)
            assert rotated["source_event_id"] == saved[0]["source_event_id"]
            expect(request(host.base, route + "/" + control["turn_id"], "DELETE", replacement), 200)
            expect(request(host.base, route + "/" + control["turn_id"], token=replacement), 404)


        token = replacement
        check(directory, [MARKER, ANSWER, "unwrittenpendingneedle", "lostreceiptneedle"])
        params = [*args, "--isolated-copy"]
        manifest = command(binary, env, "archive-verify", *params)
        assert manifest["format"] == "origence-private-archive-restore-v2"
        assert manifest["archived"] >= 2 and len(manifest["directory_sha256"]) == 64
        with database(directory) as db:
            try:
                db.execute("UPDATE oc_events SET content=? WHERE id=?", (ANSWER, source))
            except sqlite3.IntegrityError:
                pass
            else:
                raise AssertionError("SQLite accepted a private turn body")

        restored = root / "restored-origence"
        shutil.copytree(directory, restored)
        if not anyseek:
            incomplete = root / "authority-only"
            shutil.copytree(directory, incomplete, ignore=shutil.ignore_patterns("anyseek"))
            command(binary, dict(env, OC_DATA_DIR=str(incomplete)), "archive-verify",
                    *params, ok=False)
        if anyseek:
            stack.close()
            native_copy = root / "restored-native"
            shutil.copytree(backend_dir, native_copy)
            stack.enter_context(native(anyseek, native_copy, port, secret))
            # Keep the endpoint name saved in the binding, and change the data
            # only behind it. A different endpoint requires a separate policy.
            proxy_server = http.server.ThreadingHTTPServer(("127.0.0.1", int(proxy_url.rsplit(":", 1)[1])), Proxy)
            proxy_server.upstream, proxy_server.action = proxy.upstream, None
            thread = threading.Thread(target=proxy_server.serve_forever, daemon=True)
            thread.start()
            stack.callback(proxy_server.server_close)
            stack.callback(proxy_server.shutdown)
        restore_env = dict(env, OC_DATA_DIR=str(restored))
        restored_manifest = command(binary, restore_env, "archive-verify", *params)
        assert restored_manifest == manifest
        with start(env=restore_env) as host:
            assert host.request(route + "/" + turn["turn_id"])["answer"] == ANSWER
            assert host.request(route + "?limit=1")["turns"][0]["sequence"] == 1
            assert host.request("/v1/context", context)["items"][0]["provenance"]["turn_id"] == turn["turn_id"]
            assert host.request("/v1/context", dict(context, query="semantic paraphrase", mode="vector"))["items"][0]["provenance"]["turn_id"] == turn["turn_id"]
            assert host.request("/v1/search", {"query": MARKER})["hits"] == []
            if anyseek:
                for fault in ("delete_before", "delete_after"):
                    sample = new_turn(fault)
                    host.request(route, sample)
                    target = route + "/" + sample["turn_id"]
                    proxy_server.action = fault
                    expect(request(host.base, target, "DELETE", token), 503)
                    expect(request(host.base, target, token=token), 200 if fault == "delete_before" else 404)
                    expect(request(host.base, target, "DELETE", token), 200)
                    expect(request(host.base, target, token=token), 404)

            for index in range(17):
                host.request(route, new_turn(f"Multi-page clear fixture {index}"))
            cutoff = host.request(route)["through_sequence"]
            newer = new_turn("New turn outside the confirmed range")
            host.request(route, newer)
            clear = f"/v1/sessions/{session}?through_sequence={cutoff}"
            for _ in range(2):
                after = "0"
                while after is not None:
                    batch = expect(request(host.base, f"{clear}&after={after}", "DELETE", token), 200)
                    after = batch["next_after"]
                history = host.request(route)["turns"]
                assert [row["turn_id"] for row in history] == [newer["turn_id"]]
            expect(request(host.base, f"/v1/sessions/{session}", "DELETE", token), 400)
            trace = host.request(f"/v1/runs/{run_id}")
            assert trace["events"][0]["report"]["private_sources_unavailable"] == 1
            assert trace["events"][0]["references"] == []
            expect(request(host.base, prepare, "POST", token, run_request), 404)


def wait_for(predicate):
    until = time.monotonic() + 15
    while not predicate():
        assert time.monotonic() < until, "fixture did not converge"
        time.sleep(0.05)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--anyseek-binary")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="origence-private-archive-") as root, peer(Model) as (_, model):
        run_contract(str(Path(args.binary).resolve()), Path(root), model,
                     str(Path(args.anyseek_binary).resolve()) if args.anyseek_binary else None)
    print("Private archive HTTP, isolation, integrity, recovery and restore contract passed.")


if __name__ == "__main__":
    main()
