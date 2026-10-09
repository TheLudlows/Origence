"""Evaluate document-level retrieval through a fresh isolated Origence host.

No paid calls by default. This is a retrieval adapter, not an Agent benchmark.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def read_jsonl(path):
    rows = [json.loads(line) for line in Path(path).read_text(encoding="utf-8").splitlines() if line.strip()]
    ids = [row["id"] for row in rows]
    if len(ids) != len(set(ids)):
        raise ValueError("duplicate dataset IDs")
    return rows


def score(ranked_ids, relevant_ids, k):
    # Each source document is a single evidence unit regardless of chunk count.
    ranked = list(dict.fromkeys(ranked_ids))[:k]
    relevant = set(relevant_ids)
    if not relevant:
        return {"recall": None, "mrr": None, "ndcg": None, "false_positive": bool(ranked)}
    found = set(ranked) & relevant
    dcg = sum(1 / math.log2(i + 2) for i, source in enumerate(ranked) if source in relevant)
    ideal = sum(1 / math.log2(i + 2) for i in range(min(k, len(relevant))))
    return {"recall": len(found) / len(relevant),
            "mrr": next((1 / (i + 1) for i, source in enumerate(ranked) if source in relevant), 0),
            "ndcg": dcg / ideal, "false_positive": None}


def percentile(values, quantile):
    values = sorted(values)
    return values[max(0, math.ceil(quantile * len(values)) - 1)] if values else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--corpus", default=str(Path(__file__).with_name("corpus.jsonl")))
    parser.add_argument("--cases", default=str(Path(__file__).with_name("retrieval.jsonl")))
    parser.add_argument("--mode", choices=["keyword", "vector", "hybrid"], default="keyword")
    parser.add_argument("--k", type=int, default=5)
    parser.add_argument("--output", required=True)
    parser.add_argument("--commit", default="unknown")
    parser.add_argument("--allow-model-calls", action="store_true",
                        help="Explicitly enable model calls using OC_* configuration; may incur cost")
    args = parser.parse_args()
    if not 1 <= args.k <= 100:
        parser.error("k must be 1..100")
    if args.mode != "keyword" and not args.allow_model_calls:
        parser.error("vector/hybrid require --allow-model-calls and configured embedding")
    if args.allow_model_calls and os.environ.get("OC_ENABLE_MODELS") != "true":
        parser.error("model calls require OC_ENABLE_MODELS=true")
    corpus, cases = read_jsonl(args.corpus), read_jsonl(args.cases)
    corpus_ids = {doc["id"] for doc in corpus}
    if not corpus or not cases:
        parser.error("corpus and cases must not be empty")
    for doc in corpus:
        if not isinstance(doc.get("content"), str) or not doc["content"].strip():
            parser.error("corpus content must be nonempty text")
    for case in cases:
        if not isinstance(case.get("query"), str) or not case["query"].strip():
            parser.error("case query must be nonempty text")
        gold = case.get("relevant_source_ids")
        if not isinstance(gold, list) or not set(gold) <= corpus_ids:
            parser.error("case gold sources must be known corpus IDs")
    output = Path(args.output)
    if output.exists():
        parser.error("output already exists; choose a new run path")
    output.mkdir(parents=True)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    env = os.environ.copy()
    for key in ("OC_DATA_DIR", "OC_API_KEY", "OC_BIND", "OC_SERVER_URL"):
        env.pop(key, None)
    if not args.allow_model_calls:
        for key in list(env):
            if key.startswith("OC_"):
                del env[key]
        env["OC_ENABLE_MODELS"] = "false"
    manifest = {"schema_version": 1, "commit": args.commit, "platform": platform.platform(),
                "mode": args.mode, "k": args.k, "model_calls_enabled": args.allow_model_calls,
                "corpus_sha256": hashlib.sha256(Path(args.corpus).read_bytes()).hexdigest(),
                "cases_sha256": hashlib.sha256(Path(args.cases).read_bytes()).hexdigest(),
                "embedding_model": env.get("OC_EMBEDDING_MODEL"),
                "extraction_model": env.get("OC_EXTRACTION_MODEL"),
                "embedding_dimension": env.get("OC_EMBEDDING_DIMENSION"),
                "model_cost": "unknown" if args.allow_model_calls else "no model calls",
                "scope": "fresh workspace in temporary data directory",
                "limitations": ["synthetic seed corpus; not the planned 300-case dataset",
                                "document-level evidence only; no claim-level citation scoring",
                                "no Agent generation, capture, lifecycle or component ablation",
                                "single sequential run; no load-test or statistical confidence claim"]}
    (output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    process = None
    stage = "bootstrap"
    try:
        with tempfile.TemporaryDirectory(prefix="oc-eval-") as directory, tempfile.TemporaryFile(mode="w+t") as hostlog:
            env["OC_DATA_DIR"] = directory
            binary = str(Path(args.binary).resolve())
            bootstrap = subprocess.run([binary, "--offline", "workspace-create", "eval"], env=env,
                                       capture_output=True, text=True, timeout=120)
            if bootstrap.returncode:
                raise RuntimeError("workspace bootstrap failed")
            token = json.loads(bootstrap.stdout)["token"]
            process = subprocess.Popen([binary, "serve", "--bind", "127.0.0.1:0"], env=env,
                                       stdout=hostlog, stderr=hostlog, text=True)
            try:
                deadline = time.monotonic() + 120
                base = None
                while base is None and time.monotonic() < deadline:
                    if process.poll() is not None:
                        raise RuntimeError("host exited during startup")
                    hostlog.seek(0)
                    for line in hostlog.readlines():
                        try:
                            message = json.loads(line)
                        except json.JSONDecodeError:
                            continue
                        if "listening" in message:
                            base = "http://" + message["listening"]
                    time.sleep(0.1)
                if base is None:
                    raise RuntimeError("host startup timed out")

                def request(path, body=None):
                    headers = {"Authorization": "Bearer " + token}
                    if body is not None:
                        headers.update({"Content-Type": "application/json",
                                        "Idempotency-Key": hashlib.sha256((path + json.dumps(body)).encode()).hexdigest()})
                    req = urllib.request.Request(base + path, headers=headers,
                                                 data=None if body is None else json.dumps(body).encode())
                    try:
                        with opener.open(req, timeout=60) as response:
                            return json.load(response)
                    except urllib.error.HTTPError as error:
                        # Never emit an error body, token, or model response into diagnostics.
                        raise RuntimeError("HTTP status " + str(error.code)) from None

                while True:
                    try:
                        request("/health/ready")
                        break
                    except (RuntimeError, urllib.error.URLError):
                        if time.monotonic() >= deadline:
                            raise RuntimeError("readiness timed out") from None
                        time.sleep(0.2)
                mapping = {}
                imports = []
                stage = "import"
                for doc in corpus:
                    started = time.perf_counter()
                    accepted = request("/v1/knowledge", {"title": doc.get("title", doc["id"]),
                                                         "content": doc["content"], "format": "text"})
                    deadline = time.monotonic() + 300
                    while True:
                        job = request("/v1/jobs/" + accepted["job_id"])
                        if job["state"] == "completed" and job["outcome"] == "published":
                            break
                        if job["state"] in ("failed", "cancelled", "superseded", "completed"):
                            raise RuntimeError("knowledge publication did not succeed")
                        if time.monotonic() >= deadline:
                            raise RuntimeError("knowledge publication timed out")
                        time.sleep(0.1)
                    mapping[accepted["asset_id"]] = doc["id"]
                    imports.append({"source_id": doc["id"], "asset_id": accepted["asset_id"],
                                    "job_id": accepted["job_id"], "job": job,
                                    "elapsed_ms": (time.perf_counter() - started) * 1000})
                (output / "imports.json").write_text(json.dumps(imports, ensure_ascii=False, indent=2), encoding="utf-8")
                stage = "query"
                records = []
                with (output / "results.jsonl").open("w", encoding="utf-8") as stream:
                    for case in cases:
                        started = time.perf_counter()
                        record = {"id": case["id"], "category": case.get("category"), "query": case["query"],
                                  "relevant_source_ids": case["relevant_source_ids"]}
                        try:
                            response = request("/v1/search", {"query": case["query"], "mode": args.mode,
                                                             "limit": 100, "allow_partial": False})
                            if response["effective_mode"] != args.mode:
                                raise RuntimeError("unexpected retrieval mode")
                            ids = [mapping[hit["asset_id"]] for hit in response["hits"]]
                            record.update({"status": "ok", "response": response,
                                           "ranked_source_ids": list(dict.fromkeys(ids))[:args.k],
                                           **score(ids, case["relevant_source_ids"], args.k)})
                        except (RuntimeError, urllib.error.URLError, TimeoutError, KeyError, ValueError):
                            # Failed answerable requests count as zero; no-answer errors cannot count as correct abstention.
                            record.update({"status": "error", "error": "request or response invalid",
                                           "ranked_source_ids": [], **score([], case["relevant_source_ids"], args.k)})
                            if not case["relevant_source_ids"]:
                                record["false_positive"] = None
                        record["elapsed_ms"] = (time.perf_counter() - started) * 1000
                        records.append(record)
                        stream.write(json.dumps(record, ensure_ascii=False) + "\n")
                        stream.flush()
                answerable = [r for r in records if r["relevant_source_ids"]]
                unanswerable = [r for r in records if not r["relevant_source_ids"]]
                mean = lambda key: sum(r[key] for r in answerable) / len(answerable) if answerable else None
                successful_latency = [r["elapsed_ms"] for r in records if r["status"] == "ok"]
                summary = {"cases": len(records), "answerable": len(answerable), "unanswerable": len(unanswerable),
                           "errors": sum(r["status"] == "error" for r in records),
                           "document_recall_at_k": mean("recall"), "mrr_at_k": mean("mrr"), "ndcg_at_k": mean("ndcg"),
                           "no_answer_false_positives": sum(r["false_positive"] is True for r in unanswerable),
                           "no_answer_errors": sum(r["status"] == "error" for r in unanswerable),
                           "successful_request_latency_ms": {"p50": percentile(successful_latency, .5),
                                                             "p95": percentile(successful_latency, .95),
                                                             "p99": percentile(successful_latency, .99)},
                           "import_ms": sum(r["elapsed_ms"] for r in imports)}
                (output / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
                print(json.dumps(summary, ensure_ascii=False))
                if summary["errors"]:
                    raise RuntimeError("evaluation contains request errors; see report")
            finally:
                process.terminate()
                try:
                    process.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
    except (RuntimeError, subprocess.SubprocessError, OSError, KeyError, ValueError):
        (output / "failure.json").write_text(json.dumps({"stage": stage, "error": "evaluation failed; inspect CI/local host securely"}), encoding="utf-8")
        raise SystemExit("evaluation failed at " + stage + "; see failure.json") from None


if __name__ == "__main__":
    main()
