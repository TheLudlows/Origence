"""Synthetic local AML retrieval evaluation through Add/Search."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile
import time
from aml_drill import DrillError, Host, percentiles, preflight


def stream_digest(stream):
    """Bounded-memory SHA-256, also supported by local GPU Python 3.10."""
    value = hashlib.sha256()
    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
        value.update(chunk)
    return value.hexdigest()


def digest(path):
    with open(path, "rb") as stream:
        return stream_digest(stream)


def source_key(value):
    return (value["session_id"], value["message_index"])


def decode_hits(data, batches, user_id, *, allow_legacy=False):
    """Validate full source fragments, then resolve the old gold-key vocabulary.

    These short-message quality labels omit batch IDs. Refuse any ambiguous
    session/index in their corpus instead of silently crediting another batch.
    Richer batch-aware metrics use aml_evidence directly.
    """
    from aml_evidence import source_index, decode_fragments
    if not isinstance(data, list) or len(data) > 100:
        raise DrillError("invalid_hit_list")
    known = source_index(batches, user_id)
    legacy_keys = [(session, index) for session, _batch, index in known]
    if len(set(legacy_keys)) != len(legacy_keys):
        raise DrillError("ambiguous_quality_source")
    if allow_legacy and data:
        # The earliest frozen reports predate even the legacy range parser.
        # Their explicit archival path means whole-message equality, never
        # interpreting missing ranges as current message-batch provenance.
        metadata = []
        for hit in data:
            try:
                header, separator, content = hit["content"].partition("\n")
                if not separator or not header.startswith("Source metadata: "):
                    raise ValueError()
                meta = json.loads(header.removeprefix("Source metadata: "))
                if not isinstance(meta, dict):
                    raise ValueError()
                metadata.append((hit, meta, content))
            except (KeyError, TypeError, AttributeError, ValueError):
                raise DrillError("invalid_legacy_source") from None
        if all("parser" not in meta for _hit, meta, _content in metadata):
            original = {(session, index): message for (session, _batch, index), (message, _count) in known.items()}
            ids, keys = set(), []
            for hit, meta, content in metadata:
                try:
                    ident = hit["id"]
                    if not isinstance(ident, str) or not ident or ident in ids:
                        raise ValueError()
                    ids.add(ident)
                    if set(meta) != {"session_id", "message_index", "role", "timestamp"} or type(meta["message_index"]) is not int:
                        raise ValueError()
                    if meta["timestamp"] is not None and type(meta["timestamp"]) is not int:
                        raise ValueError()
                    key = source_key(meta)
                    message = original[key]
                    if content != message["content"] or any(meta[field] != message.get(field) for field in ("role", "timestamp")):
                        raise ValueError()
                    keys.append(key)
                except (KeyError, TypeError, ValueError):
                    raise DrillError("invalid_legacy_source") from None
            return keys
    parts = decode_fragments(data, known, allow_legacy=allow_legacy)
    if any(part["start"] != 0 or part["end"] != part["message_bytes"] for part in parts):
        raise DrillError("source_mismatch")
    return [(part["key"][0], part["key"][2]) for part in parts]


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
    relative = "evals/aml/corpus.json" if args.dataset == "v1" else "evals/aml/v2/corpus.json"
    corpus_path = Path(__file__).resolve().parents[1] / relative
    corpus = json.loads(corpus_path.read_text(encoding="utf-8"))
    report.update(dataset=args.dataset, corpus_sha256=digest(corpus_path), runner_sha256=digest(__file__),
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
                rows.append({"id": q["id"], "category": q["category"], "split": q.get("split", "development"), "latency_ms": round(elapsed, 2),
                             "required": q["required"], "response": result,
                             "metrics": {str(k): score(required, ranked, k) for k in report["cutoffs"]}})
            report["search_latency"] = percentiles([r["latency_ms"] for r in rows])
            report["summary"] = {str(k): summarize(rows, k) for k in report["cutoffs"]}
            report["by_category"] = {category: {str(k): summarize([r for r in rows if r["category"] == category], k)
                                      for k in report["cutoffs"]} for category in sorted({r["category"] for r in rows})}
            report["by_split"] = {split: {str(k): summarize([r for r in rows if r["split"] == split], k)
                                   for k in report["cutoffs"]} for split in sorted({r["split"] for r in rows})}
        finally:
            host.stop()
    report["temporary_tree_removed"] = not created.exists()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--report", required=True)
    parser.add_argument("--dataset", choices=("v1", "v2"), default="v1")
    args = parser.parse_args()
    report = {"schema": "aml-local-quality-report-v1", "status": "running",
              "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "limitations": ["synthetic data without independent human review; no official AML score",
                              "retrieval only, no Answer/Eval; nonempty candidates do not prove wrong answers",
                              "short messages only; longer history does not validate long-message chunking or sustained capacity",
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
