"""No-model Node UI -> Origence -> real AnySeek smoke; fresh temporary data only."""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

from native_fixture import workspace as seed_workspace
from mcp_smoke import Host, isolated_env, require


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--ui-entry", required=True)
    args = parser.parse_args()
    binary, entry = str(Path(args.binary).resolve()), str(Path(args.ui_entry).resolve())
    with tempfile.TemporaryDirectory(prefix="origence-ui-smoke-") as directory:
        env = isolated_env(directory)
        workspace = seed_workspace(env, "UI smoke")
        token = workspace["token"]
        other = seed_workspace(env, "UI smoke isolated workspace", workspace["workspace_id"])
        with Host(binary, env, token, [token, other["token"]]) as host, tempfile.TemporaryFile() as log:
            with socket.socket() as sock:
                sock.bind(("127.0.0.1", 0))
                port = sock.getsockname()[1]
            process = subprocess.Popen(["node", entry], env=dict(os.environ, ORIGENCE_API_BASE_URL=host.base, ORIGENCE_UI_PORT=str(port)), stdout=log, stderr=log)
            base = f"http://127.0.0.1:{port}"
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

            def request(path, data=None, key=token, method=None):
                headers = {"Authorization": "Bearer " + key}
                if method == "DELETE":
                    headers["Idempotency-Key"] = uuid.uuid4().hex
                raw = data if isinstance(data, bytes) else None if data is None else json.dumps(data).encode()
                if raw is not None:
                    headers.update({"Content-Type": "application/octet-stream" if isinstance(data, bytes) else "application/json", "Idempotency-Key": uuid.uuid4().hex})
                with opener.open(urllib.request.Request(base + "/api" + path, data=raw, headers=headers, method=method), timeout=30) as response:
                    return json.load(response)

            def finish(receipt):
                deadline = time.monotonic() + 45
                while True:
                    job = request("/v1/jobs/" + receipt["job_id"])
                    if job["state"] == "completed":
                        require(job["outcome"] == "published", "job completed without publication")
                        return
                    require(job["state"] not in ("failed", "cancelled", "superseded"), "publication failed")
                    require(time.monotonic() < deadline, "publication timed out")
                    time.sleep(.1)

            try:
                deadline = time.monotonic() + 30
                while True:
                    require(process.poll() is None, "Node UI exited before readiness")
                    try:
                        require(request("/health/ready")["status"] == "ready", "upstream not ready")
                        break
                    except (urllib.error.URLError, TimeoutError):
                        require(time.monotonic() < deadline, "Node UI startup timed out")
                        time.sleep(.1)
                require(request("/v1/whoami")["workspace_id"] == workspace["workspace_id"], "identity changed in proxy")
                identity = {"subject": {"kind": "service", "stable_id": "Atlas"}, "predicate": "release.rule", "context": {"env": "production"}}
                memory = request("/v1/memories/identified", {"identity": identity, "content": "发布前须人工确认 中文 🦀 é", "expected_version": 0})
                finish(memory)
                document = "# Atlas\n发布前须检查回滚方案。中文 🦀 é".encode()
                upload = request("/v1/files?name=atlas.md&format=markdown", document)
                knowledge = request("/v1/knowledge", {"title": "Atlas 发布手册", "file_id": upload["file_id"], "format": "markdown"})
                finish(knowledge)
                require(request("/v1/assets?kind=memory")["items"][0]["asset_id"] == memory["asset_id"], "memory catalog missing publication")
                require(request("/v1/assets?kind=knowledge")["items"][0]["asset_id"] == knowledge["asset_id"], "knowledge catalog missing publication")
                require(request("/v1/memories/lookup", identity)["version"] == 1, "memory lookup changed")
                require(request("/v1/search", {"query": "Atlas", "mode": "keyword"})["hits"], "search missed document")
                bundle = request("/v1/context", {"query": "Atlas", "budget": 4000})
                require(bundle["used_bytes"] <= 4000 and bundle["items"], "context budget or content changed")
                updated = request("/v1/knowledge", {"title": "Atlas 手册 v2", "content": "Atlas 新版操作说明", "format": "text", "asset_id": knowledge["asset_id"], "expected_version": 1})
                finish(updated)
                require(request(f'/v1/assets/{knowledge["asset_id"]}?version=1')["title"] == "Atlas 发布手册", "historical title changed")
                run = str(uuid.uuid4())
                host.request("/v1/runs", {"run_id": run, "summary": "UI smoke private Run"})
                host.request(f"/v1/runs/{run}/context", {"event_id": str(uuid.uuid4()), "context": {"query": "Atlas"}})
                require(request("/v1/runs")["items"][0]["run_id"] == run, "run catalog missing owner record")
                trace = request(f"/v1/runs/{run}")
                require(trace["events"][0]["evidence_origin"] == "server_prepared", "preparation origin lost")
                require(request("/v1/runs", key=other["token"])["items"] == [], "private Run leaked to another key")
                try:
                    request(f"/v1/runs/{run}", key=other["token"])
                    raise RuntimeError("private Run detail leaked")
                except urllib.error.HTTPError as error:
                    require(error.code == 404, "unexpected private Run denial")
                request("/v1/events/" + memory["source_event_id"], method="DELETE")
                require(request("/v1/assets?kind=memory")["items"] == [], "withdrawn source remained visible")
                print("PASS: Node UI -> Origence -> real AnySeek: identity, upload, publication, catalogs, history, search, context, private Run isolation and source withdrawal")
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)
                log.seek(0)
                require(token.encode() not in log.read(), "Node UI logged credentials")


if __name__ == "__main__":
    main()
