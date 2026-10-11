"""Descriptive paired/clustered statistics for the frozen budget experiment."""
import json
import random
from collections import defaultdict
from aml_budget_eval import ARMS, OUT, load_data, load_artifact
from aml_drill import percentiles


def interval(rows, value):
    groups = defaultdict(list)
    for row in rows:
        groups[row["cluster"]].append(row)
    keys = sorted(groups)
    rng = random.Random(20261011)
    samples = []
    for _ in range(10000):
        sample = [row for key in rng.choices(keys, k=len(keys)) for row in groups[key]]
        samples.append(sum(value(row) for row in sample) / len(sample))
    samples.sort()
    return {"clusters": len(keys), "lower": samples[249], "upper": samples[9749], "draws": 10000}


def summarize(dataset):
    corpus, labels = load_data(dataset)
    grade, receipt = load_artifact(dataset, "grade")
    retrieve, _ = load_artifact(dataset, "retrieve")
    prepared, _ = load_artifact(dataset, "prepare")
    answers, ranks = {}, {"vector": prepared}
    for arm in ARMS:
        answers[arm] = load_artifact(dataset, "answer", arm)[0]
        if arm != "vector":
            ranks[arm] = load_artifact(dataset, "rank", arm)[0]
    rows = []
    for i, q in enumerate(corpus["questions"]):
        label = labels[q["id"]]
        row = {"id": q["id"], "split": q["split"], "cluster": label["cluster"],
               "answerable": label["answerable"], "arms": {}}
        for arm in ARMS:
            answer = answers[arm]["rows"][i]
            judged = grade["rows"][i]["arms"][arm]
            ranked = ranks[arm]["rows"][i]
            row["arms"][arm] = {"success": bool(judged.get("success", False)),
                "answered": answer.get("output", {}).get("answerable", False),
                "answer_error": answer["status"] != "completed", "judge_error": judged["status"] != "completed",
                "correct": bool(judged.get("correct", False)), "supported": bool(judged.get("supported", False)),
                "retrieval": ranked.get("metrics"), "reason": judged["reason"]}
        rows.append(row)
    splits = {}
    for split in sorted({r["split"] for r in rows}):
        group = [r for r in rows if r["split"] == split]
        split_summary = {"assigned": len(group), "arms": {}}
        for arm in ARMS:
            values = [r["arms"][arm] for r in group]
            can = [r for r in group if r["answerable"]]
            cannot = [r for r in group if not r["answerable"]]
            answered = sum(v["answered"] for v in values)
            successful_answered = sum(v["success"] and v["answered"] for v in values)
            key = "full_message_recall" if dataset == "v4" else "session_recall"
            recalls = [r["arms"][arm]["retrieval"].get(key, 0) if r["arms"][arm]["retrieval"] else 0 for r in can]
            split_summary["arms"][arm] = {"supported_successes": sum(v["success"] for v in values),
                "supported_success_rate": sum(v["success"] for v in values) / len(group),
                "success_cluster_interval": interval(group, lambda r: int(r["arms"][arm]["success"])),
                "answered": answered, "answer_coverage": answered / len(group),
                "correct_supported_answered": successful_answered,
                "selective_accuracy": successful_answered / answered if answered else None,
                "answerable_count": len(can), "answerable_successes": sum(r["arms"][arm]["success"] for r in can),
                "answerable_abstentions": sum(not r["arms"][arm]["answered"] and not r["arms"][arm]["answer_error"] for r in can),
                "no_answer_count": len(cannot), "correct_abstentions": sum(r["arms"][arm]["success"] for r in cannot),
                "answer_errors": sum(v["answer_error"] for v in values), "judge_errors": sum(v["judge_error"] for v in values),
                key: sum(recalls)/len(recalls) if recalls else None}
        split_summary["paired_bge_minus_chat_interval"] = interval(group, lambda r: int(r["arms"]["bge"]["success"])-int(r["arms"]["chat"]["success"]))
        splits[split] = split_summary
    usage = {"calls": 0, "reported_total_tokens": 0, "missing_usage_calls": 0}
    for report in [grade, *answers.values(), ranks["chat"]]:
        for call in report.get("chat_calls", []):
            usage["calls"] += 1
            tokens = call.get("usage", {}).get("total_tokens")
            if isinstance(tokens, int): usage["reported_total_tokens"] += tokens
            else: usage["missing_usage_calls"] += 1
    return {"dataset": dataset, "grading_receipt": receipt, "splits": splits, "rows": rows, "usage": usage,
            "search_latency": percentiles([r["search_ms"] for r in retrieve["rows"] if r["status"] == "completed"]),
            "rerank_extra_latency": {a: percentiles([r["extra_ms"] for r in ranks[a]["rows"] if r["status"] == "completed"]) for a in ("chat", "bge")},
            "bge_gpu_allocator": ranks["bge"].get("gpu_memory"),
            "notes": ["same-model semantic judge; not independent human validation", "cluster bootstrap is descriptive with few clusters", "public session recall is not full evidence recall", "reference-token upper limits, not equal actual reader context length"]}


if __name__ == "__main__":
    result = {dataset: summarize(dataset) for dataset in ("v4", "public")}
    with open(OUT / "summary.json", "x", encoding="utf-8") as stream:
        json.dump(result, stream, ensure_ascii=False, indent=2)
        stream.write("\n")
    print(json.dumps({d: {"splits": v["splits"], "usage": v["usage"]} for d, v in result.items()}, ensure_ascii=False))
