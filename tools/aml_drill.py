"""Synthetic-only AML preflight, load and restart drill (never official input).

Credentials are inherited through OC_* environment variables, never arguments.
Creates and removes its own isolated temporary host; prints only aggregate results.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import math
import os
import shutil
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


class DrillError(Exception):
    pass


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request(base, path, key, body=None, timeout=60):
    # Do not forward credentials to redirects or include provider diagnostics.
    opener = urllib.request.build_opener(NoRedirect(), urllib.request.ProxyHandler({}))
    headers = {"Authorization": "Bearer " + key}
    if body is not None:
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(base.rstrip("/") + path,
        data=None if body is None else json.dumps(body).encode(), headers=headers)
    try:
        with opener.open(req, timeout=timeout) as response:
            data = response.read(8_000_001)
            if len(data) > 8_000_000:
                raise DrillError("response_too_large")
            return json.loads(data)
    except urllib.error.HTTPError as error:
        code = error.code
        error.close()
        raise DrillError("http_" + str(code)) from None
    except (urllib.error.URLError, TimeoutError, ConnectionError):
        raise DrillError("request_failed") from None
    except (ValueError, UnicodeError):
        raise DrillError("invalid_json") from None


def preflight(env):
    base = env.get("OC_MODEL_BASE_URL", "")
    model = env.get("OC_EMBEDDING_MODEL", "")
    key = env.get("OC_MODEL_API_KEY", "")
    try:
        dimension = int(env.get("OC_EMBEDDING_DIMENSION", "0"))
    except ValueError:
        raise DrillError("invalid_dimension") from None
    from urllib.parse import urlsplit
    url = urlsplit(base)
    if not (url.scheme == "https" or (url.scheme == "http" and url.hostname in ("localhost", "127.0.0.1", "::1"))):
        raise DrillError("https_required")
    if url.username or url.password or url.query or url.fragment:
        raise DrillError("invalid_endpoint")
    if not model or not 1 <= dimension <= 4096:
        raise DrillError("embedding_configuration_required")
    response = request(base, "/embeddings", key,
        {"model": model, "input": "Synthetic connection check. 合成连接检查。", "dimensions": dimension})
    try:
        vector = response["data"][0]["embedding"]
        valid = (len(vector) == dimension and any(x != 0 for x in vector)
                 and all(type(x) in (int, float) and math.isfinite(x) for x in vector))
    except (KeyError, IndexError, TypeError):
        valid = False
    if not valid:
        raise DrillError("invalid_embedding")
    return {"model": model, "dimension": dimension,
            "endpoint_sha256": hashlib.sha256(base.encode()).hexdigest(),
            "extraction": None, "summaries": False, "graph": False,
            "preflight": "passed"}


def percentiles(values):
    ordered = sorted(values)
    return {"count": len(ordered), **{name: round(ordered[min(len(ordered)-1, math.ceil(p*len(ordered))-1)], 2)
            for name, p in [("p50_ms", .5), ("p95_ms", .95), ("p99_ms", .99)]}} if ordered else {"count": 0}


class Host:
    def __init__(self, binary, directory, env):
        self.binary = str(Path(binary).resolve(strict=True))
        self.directory = Path(directory)
        self.env = {k: v for k, v in env.items() if not k.startswith("OC_")}
        self.env.update({k: v for k, v in env.items() if k.startswith("OC_MODEL_") or k.startswith("OC_EMBEDDING_")})
        self.env.update(OC_ENABLE_MODELS="true", OC_EXTRACTION_MODEL="", OC_DATA_DIR=str(self.directory / "data"), RUST_LOG="error")
        self.process = None
        self.log = None

    def bootstrap(self):
        result = subprocess.run([self.binary, "--offline", "workspace-create", "synthetic AML drill"],
            env=self.env, capture_output=True, timeout=120)
        if result.returncode:
            raise DrillError("bootstrap_failed")
        try:
            self.key = json.loads(result.stdout)["token"]
        except (ValueError, KeyError):
            raise DrillError("bootstrap_response_invalid") from None

    def start(self):
        self.log = tempfile.TemporaryFile(mode="w+b", dir=self.directory)
        self.process = subprocess.Popen([self.binary, "serve", "--bind", "127.0.0.1:0"],
            env=self.env, stdout=self.log, stderr=self.log)
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise DrillError("host_exited")
            self.log.seek(0)
            for line in self.log.readlines():
                try:
                    address = json.loads(line).get("listening")
                except ValueError:
                    continue
                if address:
                    self.base = "http://" + address
                    request(self.base, "/health/ready", "")
                    return
            time.sleep(.1)
        raise DrillError("host_start_timeout")

    def stop(self):
        if self.process is not None:
            if self.process.poll() is None:
                # Deliberate process death for restart evidence; not a graceful shutdown claim.
                self.process.kill()
            self.process.wait(timeout=30)
            self.process = None
        if self.log is not None:
            self.log.close()
            self.log = None

    def call(self, path, body=None):
        started = time.perf_counter()
        result = request(self.base, path, self.key, body, timeout=1560)
        return result, (time.perf_counter() - started) * 1000


def drill(args, env, report):
    # The only recursive cleanup is TemporaryDirectory's own newly-created child.
    parent = Path(__file__).resolve().parents[1] / "target" / "aml-drills"
    parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="synthetic-", dir=parent) as directory:
        created = Path(directory).resolve()
        if created.parent != parent.resolve():
            raise DrillError("unsafe_temporary_path")
        host = Host(args.binary, created, env)
        with open(host.binary, "rb") as binary:
            report["binary_sha256"] = hashlib.file_digest(binary, "sha256").hexdigest()
        try:
            host.bootstrap()
            host.start()
            host.call("/admin/aml/namespace", {})
            batches = []
            for user in range(args.users):
                for session in range(2):
                    marker = f"USER{user:04d}SESSION{session}"
                    batches.append({"request_id": f"request-{session}", "user_id": f"user-{user}",
                        "session_id": f"session-{session}", "messages": [{"role": "user", "timestamp": 1720000000000 + session,
                        "content": f"{marker} Atlas approval requires {user+1} reviewers. 审批人数为{user+1}。🙂"}]})
            def add(batch):
                result, elapsed = host.call("/aml/add", batch)
                expected = {"success": True, **{k: batch[k] for k in ("request_id", "user_id", "session_id")}}
                if result != expected:
                    raise DrillError("add_contract_failed")
                return elapsed
            with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
                add_times = list(pool.map(add, batches))
            def search(user):
                result, elapsed = host.call("/aml/search", {"user_id": f"user-{user}", "query": "Atlas approval 审批人数", "top_k": 100})
                data = result.get("data", [])
                expected = {f"USER{user:04d}SESSION{s}" for s in range(2)}
                if len(data) != 2 or len({x["id"] for x in data}) != 2:
                    raise DrillError("search_count_failed")
                for item in data:
                    if not any(marker in item["content"] for marker in expected):
                        raise DrillError("isolation_failed")
                if not all(any(marker in item["content"] for item in data) for marker in expected):
                    raise DrillError("session_evidence_missing")
                return elapsed, [item["id"] for item in data]
            with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
                found = list(pool.map(search, [u for _ in range(args.rounds) for u in range(args.users)]))
            if host.call("/aml/search", {"user_id": "unknown", "query": "Atlas", "top_k": 100})[0] != {"data": []}:
                raise DrillError("unknown_user_failed")
            before = search(0)[1]
            add(batches[0])
            if search(0)[1] != before:
                raise DrillError("idempotency_failed")
            host.stop()
            # Copy the entire stopped-host data tree, including WAL and native stores.
            backup = created / "backup"
            shutil.copytree(created / "data", backup)
            recovery_start = time.perf_counter()
            host.start()
            add(batches[0])
            if search(0)[1] != before:
                raise DrillError("restart_replay_failed")
            restart_ms = round((time.perf_counter()-recovery_start)*1000, 2)
            host.stop()
            host.env["OC_DATA_DIR"] = str(backup)
            restore_start = time.perf_counter()
            host.start()
            add(batches[0])
            if search(0)[1] != before:
                raise DrillError("backup_restore_failed")
            restore_ms = round((time.perf_counter()-restore_start)*1000, 2)
            report.update(add=percentiles(add_times), search=percentiles([r[0] for r in found]),
                users=args.users, concurrency=args.concurrency, rounds=args.rounds,
                restart_ms=restart_ms, backup_restore_ms=restore_ms,
                disk_bytes=sum(p.stat().st_size for p in created.rglob("*") if p.is_file()),
                checks=["isolated_users", "cross_session", "unknown_user", "stable_ids", "idempotency", "restart_after_publication", "stopped_host_backup_restore"])
        finally:
            host.stop()
    report["temporary_tree_removed"] = not created.exists()
    if created.exists():
        raise DrillError("temporary_cleanup_failed")
    report["limitations"] = ["synthetic protocol drill, not semantic ranking or official Smoke",
        "process death after completed publication, not in-flight crash or power loss",
        "logical temporary-tree removal including test backup, not media erasure, external copies or provider retention",
        "no peak memory, provider usage, sustained Full-duration load or production SLO"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary")
    parser.add_argument("--preflight-only", action="store_true")
    parser.add_argument("--users", type=int, default=4)
    parser.add_argument("--concurrency", type=int, default=2)
    parser.add_argument("--rounds", type=int, default=2)
    parser.add_argument("--report", required=True)
    args = parser.parse_args()
    if not args.preflight_only and not args.binary:
        parser.error("--binary required unless --preflight-only")
    if not (1 <= args.users <= 100 and 1 <= args.concurrency <= 16 and 1 <= args.rounds <= 100):
        parser.error("users 1..100, concurrency 1..16, rounds 1..100")
    output = Path(args.report)
    if output.exists():
        parser.error("report already exists; use a new run path")
    report = {"started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "status": "failed", "data_kind": "self_generated_synthetic", "schema": "aml-drill-v1"}
    try:
        report["configuration"] = preflight(os.environ)
        if not args.preflight_only:
            drill(args, os.environ, report)
        report["status"] = "passed"
    except DrillError as error:
        report["error_code"] = str(error)
    except Exception:
        report["error_code"] = "drill_internal_failure"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], "error_code": report.get("error_code")}))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
