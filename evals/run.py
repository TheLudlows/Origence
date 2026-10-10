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
import urllib.parse


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


def validate_evidence(corpus, cases, required=False):
    documents = {doc["id"]: doc for doc in corpus}
    for case in cases:
        evidence = case.get("evidence", [])
        if not isinstance(evidence, list):
            raise ValueError("evidence must be a list")
        if required and case["relevant_source_ids"] and not evidence:
            raise ValueError("answerable case requires evidence")
        for item in evidence:
            doc = documents.get(item.get("source_id"))
            if (doc is None or item.get("source_version") != doc.get("source_version", 1)
                    or item.get("locator") != doc.get("locator", "source-document")
                    or not isinstance(item.get("quote"), str) or not item["quote"]
                    or item["quote"] not in doc["content"]):
                raise ValueError("evidence must cite the exact source version, locator and verbatim quote")
        if evidence and {e["source_id"] for e in evidence} != set(case["relevant_source_ids"]):
            raise ValueError("evidence sources must equal gold sources")


def evidence_recall(hits, evidence, mapping, k):
    if not evidence:
        return None
    top_sources = list(dict.fromkeys(mapping[h["asset_id"]] for h in hits))[:k]
    return sum(any(mapping[h["asset_id"]] == e["source_id"]
                   and h.get("version") == e["source_version"]
                   and e["quote"] in h.get("content", "")
                   for h in hits if mapping[h["asset_id"]] in top_sources)
               for e in evidence) / len(evidence)


def context_coverage(context, hits, evidence, mapping):
    # Graph provenance alone is not an exact quotation. Only whole cited original
    # chunks actually rendered by resolve count toward this evidence metric.
    if not evidence:
        return None
    included = {(s.get("asset_id"), s.get("version"), s.get("chunk_id"))
                for s in context["sources"] if "asset_id" in s
                and isinstance(s.get("citation"), str)
                and s["citation"] in context["rendered_context"]}
    present = [h for h in hits if (h["asset_id"], h.get("version"), h.get("chunk_id")) in included]
    return evidence_recall(present, evidence, mapping, len(mapping))


def summarize(records, imports):
    answerable = [r for r in records if r["relevant_source_ids"]]
    unanswerable = [r for r in records if not r["relevant_source_ids"]]
    def mean(key):
        values = [r[key] for r in answerable if r.get(key) is not None]
        return sum(values) / len(values) if values else None
    latencies = [r["elapsed_ms"] for r in records if r["status"] == "ok"]
    resolve_latencies = [r["resolve_elapsed_ms"] for r in records if r.get("resolve_status") == "ok"]
    return {"cases": len(records), "answerable": len(answerable), "unanswerable": len(unanswerable),
            "errors": sum(r["status"] == "error" for r in records),
            "resolve_errors": sum(r.get("resolve_status") == "error" for r in records),
            "document_recall_at_k": mean("recall"), "mrr_at_k": mean("mrr"), "ndcg_at_k": mean("ndcg"),
            "evidence_recall_at_k": mean("evidence_recall"), "coverage_at_budget": mean("coverage"),
            "fully_covered_at_budget": mean("fully_covered"),
            "no_answer_false_positives": sum(r["false_positive"] is True for r in unanswerable),
            "no_answer_errors": sum(r["status"] == "error" for r in unanswerable),
            "successful_request_latency_ms": {"p50": percentile(latencies, .5),
                                              "p95": percentile(latencies, .95), "p99": percentile(latencies, .99)},
            "successful_resolve_latency_ms": {"p50": percentile(resolve_latencies, .5),
                                              "p95": percentile(resolve_latencies, .95), "p99": percentile(resolve_latencies, .99)},
            "import_ms": sum(r["elapsed_ms"] for r in imports)}


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
    parser.add_argument("--components", choices=["original", "summary", "graph", "all"], default="all")
    parser.add_argument("--matrix", action="store_true", help="Compare six branches on one immutable publication")
    parser.add_argument("--budget", type=int, default=2000, help="Shared resolve UTF-8 byte budget (not exact model tokens)")
    parser.add_argument("--require-evidence", action="store_true")
    parser.add_argument("--model-metadata", help="JSON with model revisions, inference policy and local inference cost")
    parser.add_argument("--usage-url", help="Optional loopback model-server usage endpoint")
    args = parser.parse_args()
    if not 1 <= args.k <= 100:
        parser.error("k must be 1..100")
    if (args.mode != "keyword" or args.matrix) and not args.allow_model_calls:
        parser.error("vector/hybrid require --allow-model-calls and configured embedding")
    if args.allow_model_calls and os.environ.get("OC_ENABLE_MODELS") != "true":
        parser.error("model calls require OC_ENABLE_MODELS=true")
    if not 0 <= args.budget <= 32000:
        parser.error("budget must be 0..32000 bytes")
    if args.usage_url:
        url = urllib.parse.urlparse(args.usage_url)
        if url.scheme != "http" or url.hostname not in ("127.0.0.1", "localhost", "::1"):
            parser.error("usage endpoint must be loopback HTTP")
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
    try:
        validate_evidence(corpus, cases, args.require_evidence)
    except ValueError as error:
        parser.error(str(error))
    output = Path(args.output)
    if output.exists():
        parser.error("output already exists; choose a new run path")
    output.mkdir(parents=True)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    def usage():
        if not args.usage_url:
            return None
        with opener.open(args.usage_url, timeout=10) as response:
            return json.load(response)["usage"]
    def usage_delta(before, after):
        return {k: after[k] - before[k] for k in before} if before is not None else None
    usage_before_import = usage()
    env = os.environ.copy()
    for key in ("OC_DATA_DIR", "OC_API_KEY", "OC_BIND", "OC_SERVER_URL"):
        env.pop(key, None)
    if not args.allow_model_calls:
        for key in list(env):
            if key.startswith("OC_"):
                del env[key]
        env["OC_ENABLE_MODELS"] = "false"
    manifest = {"schema_version": 2, "commit": args.commit, "platform": platform.platform(),
                "mode": "matrix" if args.matrix else args.mode, "k": args.k,
                "components": args.components, "budget_bytes": args.budget, "model_calls_enabled": args.allow_model_calls,
                "corpus_sha256": hashlib.sha256(Path(args.corpus).read_bytes()).hexdigest(),
                "cases_sha256": hashlib.sha256(Path(args.cases).read_bytes()).hexdigest(),
                "embedding_model": env.get("OC_EMBEDDING_MODEL"),
                "extraction_model": env.get("OC_EXTRACTION_MODEL"),
                "embedding_dimension": env.get("OC_EMBEDDING_DIMENSION"),
                "model_cost": "unknown" if args.allow_model_calls else "no model calls",
                "scope": "fresh workspace in temporary data directory",
                "limitations": ["synthetic corpus; not representative production data",
                                "verbatim quote coverage, not semantic entailment or answer support",
                                "no Agent generation, capture or lifecycle quality scoring",
                                "single sequential run; no load-test or statistical confidence claim"]}
    if args.model_metadata:
        manifest["model_metadata"] = json.loads(Path(args.model_metadata).read_text(encoding="utf-8"))
        manifest["model_cost"] = manifest["model_metadata"].get("cost", "unknown")
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
                (output / "import-usage.json").write_text(json.dumps(usage_delta(usage_before_import, usage()), indent=2), encoding="utf-8")
                stage = "query"
                settings = {"original": {"summaries": False, "graph": False},
                            "summary": {"summaries": True, "graph": False},
                            "graph": {"summaries": False, "graph": True},
                            "all": {"summaries": True, "graph": True}}
                configurations = [("keyword", "keyword", "original"), ("vector", "vector", "original"),
                                  ("hybrid-original", "hybrid", "original"),
                                  ("hybrid-summary", "hybrid", "summary"),
                                  ("hybrid-graph", "hybrid", "graph"),
                                  ("hybrid-all", "hybrid", "all")] if args.matrix else [(args.mode, args.mode, args.components)]
                comparisons = {}
                for label, mode, components in configurations:
                    usage_before_query = usage()
                    destination = output / label if args.matrix else output
                    destination.mkdir(exist_ok=True)
                    if args.matrix:
                        config = dict(manifest, mode=mode, components=components, shared_publication="../imports.json")
                        (destination / "manifest.json").write_text(json.dumps(config, ensure_ascii=False, indent=2), encoding="utf-8")
                    records = []
                    with (destination / "results.jsonl").open("w", encoding="utf-8") as stream:
                        for case in cases:
                            started = time.perf_counter()
                            record = {"id": case["id"], "category": case.get("category"), "split": case.get("split"), "query": case["query"],
                                      "relevant_source_ids": case["relevant_source_ids"]}
                            evidence = case.get("evidence", [])
                            body = {"query": case["query"], "mode": mode, "limit": 100,
                                    "allow_partial": False, "components": settings[components]}
                            try:
                                response = request("/v1/search", body)
                                if response["effective_mode"] != mode:
                                    raise RuntimeError("unexpected retrieval mode")
                                hits = response["hits"]
                                ids = [mapping[hit["asset_id"]] for hit in hits]
                                record.update({"status": "ok", "response": response,
                                               "ranked_source_ids": list(dict.fromkeys(ids))[:args.k],
                                               "evidence_recall": evidence_recall(hits, evidence, mapping, args.k),
                                               **score(ids, case["relevant_source_ids"], args.k)})
                            except (RuntimeError, urllib.error.URLError, TimeoutError, KeyError, ValueError):
                                record.update({"status": "error", "error": "request or response invalid",
                                               "ranked_source_ids": [], "evidence_recall": 0 if evidence else None,
                                               **score([], case["relevant_source_ids"], args.k)})
                                if not case["relevant_source_ids"]:
                                    record["false_positive"] = None
                            record["elapsed_ms"] = (time.perf_counter() - started) * 1000
                            started = time.perf_counter()
                            if record["status"] == "ok" and evidence:
                                try:
                                    context = request("/v1/resolve", {k: v for k, v in body.items() if k != "limit"}
                                                      | {"budget_tokens": args.budget})
                                    rendered_bytes = len(context["rendered_context"].encode("utf-8"))
                                    if context["effective_mode"] != mode or rendered_bytes > args.budget:
                                        raise ValueError("unexpected context mode or byte budget")
                                    coverage = context_coverage(context, hits, evidence, mapping)
                                    record.update({"resolve_status": "ok", "context": context, "coverage": coverage,
                                                   "fully_covered": int(coverage == 1), "context_bytes": rendered_bytes})
                                except (RuntimeError, urllib.error.URLError, TimeoutError, KeyError, ValueError):
                                    record.update({"resolve_status": "error", "coverage": 0, "fully_covered": 0})
                            elif evidence:
                                record.update({"resolve_status": "not_run", "coverage": 0, "fully_covered": 0})
                            record["resolve_elapsed_ms"] = (time.perf_counter() - started) * 1000
                            records.append(record)
                            stream.write(json.dumps(record, ensure_ascii=False) + "\n")
                            stream.flush()
                    summary = summarize(records, imports)
                    summary["model_usage"] = usage_delta(usage_before_query, usage())
                    summary["by_category"] = {category: summarize([r for r in records if r["category"] == category], [])
                                              for category in sorted({r["category"] for r in records}, key=str)}
                    summary["by_split"] = {split: summarize([r for r in records if r["split"] == split], [])
                                           for split in sorted({r["split"] for r in records if r["split"] is not None})}
                    (destination / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
                    comparisons[label] = summary
                    print(json.dumps({"configuration": label, **summary}, ensure_ascii=False), flush=True)
                if args.matrix:
                    (output / "comparison.json").write_text(json.dumps(comparisons, ensure_ascii=False, indent=2), encoding="utf-8")
                if any(s["errors"] or s["resolve_errors"] for s in comparisons.values()):
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
