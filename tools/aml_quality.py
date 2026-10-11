"""Synthetic development-only AML retrieval evaluation through Add/Search."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile
import time
from aml_drill import DrillError, Host, percentiles, preflight


def digest(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source_key(value):
    return (value["session_id"], value["message_index"])


def decode_hits(data, batches, user_id):
    known = {(b["session_id"], i): message for b in batches
             if b["user_id"] == user_id for i, message in enumerate(b["messages"])}
    keys = []
    ids = set()
    for hit in data:
        if not isinstance(hit.get("id"), str) or not hit["id"] or hit["id"] in ids:
            raise DrillError("invalid_or_duplicate_hit_id")
        ids.add(hit["id"])
        header, separator, content = hit.get("content", "").partition("\n")
        if not separator or not header.startswith("Source metadata: "):
            raise DrillError("missing_source_metadata")
        try:
            metadata = json.loads(header[len("Source metadata: "):])
            key = source_key(metadata)
            message = known[key]
        except (ValueError, KeyError, TypeError):
            raise DrillError("unknown_or_foreign_source") from None
        # Corpus messages fit a single chunk; full original text is required.
        if content != message["content"] or any(metadata.get(k) != message.get(k) for k in ("role", "timestamp")):
            raise DrillError("source_mismatch")
        keys.append(key)
    return keys


def score(required, ranked, k):
    gold = set(required)
    retrieved = set(ranked[:k])
    if not gold:
        return {"recall": None, "all_evidence": None, "reciprocal_rank": None,
                "nonempty_no_answer": bool(ranked[:k])}
    return {"recall": len(gold & retrieved) / len(gold),
            "all_evidence": gold <= retrieved,
            "reciprocal_rank": next((1 / (i + 1) for i, key in enumerate(ranked[:k]) if key in gold), 0),
            "nonempty_no_answer": None}


def summarize(rows, k):
    scored = [row["metrics"][str(k)] for row in rows]
    answerable = [x for x in scored if x["recall"] is not None]
    absent = [x for x in scored if x["nonempty_no_answer"] is not None]
    return {"questions": len(rows), "answerable": len(answerable), "no_answer": len(absent),
            "recall": sum(x["recall"] for x in answerable) / len(answerable) if answerable else None,
            "all_evidence": sum(x["all_evidence"] for x in answerable) / len(answerable) if answerable else None,
            "mrr": sum(x["reciprocal_rank"] for x in answerable) / len(answerable) if answerable else None,
            "nonempty_no_answer": sum(x["nonempty_no_answer"] for x in absent)}


def evaluate(args, report):
    corpus_path = Path(__file__).resolve().parents[1] / "evals/aml/corpus.json"
    corpus = json.loads(corpus_path.read_text(encoding="utf-8"))
    report.update(corpus_sha256=digest(corpus_path), runner_sha256=digest(__file__),
                  corpus_origin=corpus["origin"], split=corpus["split"],
                  binary_sha256=digest(args.binary), configuration=preflight(os.environ),
                  cutoffs=[1, 5, 10, 100], requested_top_k=100,
                  batches=len(corpus["batches"]), messages=sum(len(b["messages"]) for b in corpus["batches"]))
    parent = Path(__file__).resolve().parents[1] / "target/aml-drills"
    parent.mkdir(parents=True, exist_ok=True)
    rows = report["queries"] = []
    with tempfile.TemporaryDirectory(prefix="quality-", dir=parent) as directory:
        created = Path(directory).resolve()
        if created.parent != parent.resolve():
            raise DrillError("unsafe_temporary_path")
        host = Host(args.binary, created, os.environ)
        try:
            host.bootstrap()
            host.start()
            host.call("/admin/aml/namespace", {})
            add_times = []
            for batch in corpus["batches"]:
                result, elapsed = host.call("/aml/add", batch)
                if result != {"success": True, **{k: batch[k] for k in ("request_id", "user_id", "session_id")}}:
                    raise DrillError("add_contract_failed")
                add_times.append(elapsed)
            report["add_latency"] = percentiles(add_times)
            for q in corpus["questions"]:
                result, elapsed = host.call("/aml/search", {"user_id": q["user_id"], "query": q["query"], "top_k": 100})
                data = result.get("data")
                if not isinstance(data, list) or len(data) > 100:
                    raise DrillError("search_contract_failed")
                ranked = decode_hits(data, corpus["batches"], q["user_id"])
                required = [source_key(x) for x in q["required"]]
                rows.append({"id": q["id"], "category": q["category"], "latency_ms": round(elapsed, 2),
                             "required": q["required"], "response": result,
                             "metrics": {str(k): score(required, ranked, k) for k in report["cutoffs"]}})
            report["search_latency"] = percentiles([r["latency_ms"] for r in rows])
            report["summary"] = {str(k): summarize(rows, k) for k in report["cutoffs"]}
            report["by_category"] = {category: {str(k): summarize([r for r in rows if r["category"] == category], k)
                                      for k in report["cutoffs"]} for category in sorted({r["category"] for r in rows})}
        finally:
            host.stop()
    report["temporary_tree_removed"] = not created.exists()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--report", required=True)
    args = parser.parse_args()
    report = {"schema": "aml-local-quality-report-v1", "status": "running",
              "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "limitations": ["synthetic development only; no held-out generalization or official AML score",
                              "retrieval only, no Answer/Eval; nonempty candidates do not prove wrong answers",
                              "72 short messages per user, not full long-context or sustained capacity validation",
                              "no provider usage or peak-memory instrumentation; latency is single local run"]}
    # Reserve an exclusive artifact before spending work; never overwrite evidence.
    with open(args.report, "x", encoding="utf-8") as output:
        try:
            evaluate(args, report)
            report["status"] = "completed"
        except DrillError as error:
            report.update(status="failed", error=str(error))
        except Exception:
            report.update(status="failed", error="evaluation_failed")
        finally:
            json.dump(report, output, ensure_ascii=False, indent=2)
            output.write("\n")
    print(json.dumps({"status": report["status"], "summary": report.get("summary"), "error": report.get("error")}, ensure_ascii=False))
    return 0 if report["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
