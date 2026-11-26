"""Calibrate an experimental abstention rule on S1 development responses only."""

import argparse
import hashlib
import json
import math
from collections import Counter
from pathlib import Path

from run import summarize


MODES = ("vector", "hybrid-original")
QUALITY = ("document_recall_at_k", "evidence_recall_at_k", "coverage_at_budget")
MAX_DEVELOPMENT_LOSS = 0.01


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_split(root, split):
    groups = {}
    for mode in MODES:
        with (root / mode / "results.jsonl").open(encoding="utf-8") as stream:
            rows = [row for row in map(json.loads, stream) if row["split"] == split]
        groups[mode] = {row["id"]: row for row in rows}
        if not rows or len(rows) != len(groups[mode]):
            raise ValueError("missing or duplicate case IDs")
    if groups["vector"].keys() != groups["hybrid-original"].keys():
        raise ValueError("unpaired responses")
    for case_id, vector in groups["vector"].items():
        hybrid = groups["hybrid-original"][case_id]
        if any(vector[key] != hybrid[key] for key in ("query", "relevant_source_ids")):
            raise ValueError("unpaired query or gold sources")
    return groups


def best_scores(vectors):
    return {
        case_id: max((hit["score"] for hit in row["response"]["hits"]), default=None)
        if row["status"] == "ok" else None
        for case_id, row in vectors.items()
    }


def gate(row, score, threshold, vector_failed=False):
    if row["status"] != "ok" or row.get("resolve_status") == "error" or (threshold is not None and vector_failed):
        return row, "request_failed"
    if row.get("resolve_status") != "ok":
        return row, "not_run"
    if threshold is not None and (score is None or score < threshold):
        if row["relevant_source_ids"]:
            return dict(row, recall=0, mrr=0, ndcg=0, evidence_recall=0, coverage=0,
                        fully_covered=0), "insufficient_evidence"
        return dict(row, false_positive=False), "insufficient_evidence"
    state = "evidence_returned" if row["context"]["sources"] else "insufficient_evidence"
    return row, state


def compare(groups, threshold):
    scores = best_scores(groups["vector"])
    pairs = {}
    summary = {}
    for mode, records in groups.items():
        outcomes = []
        for case_id, row in records.items():
            before, baseline_state = gate(row, scores[case_id], None)
            after, gated_state = gate(row, scores[case_id], threshold,
                                      vector_failed=groups["vector"][case_id]["status"] != "ok")
            outcomes.append((before, baseline_state, after, gated_state))
            fields = ("recall", "evidence_recall", "coverage", "false_positive")
            pairs.setdefault(mode, []).append({
                "id": case_id,
                "vector_score": scores[case_id],
                "before": {key: before.get(key) for key in fields} | {"context_state": baseline_state},
                "after": {key: after.get(key) for key in fields} | {"context_state": gated_state},
            })
        summary[mode] = {}
        for name, row_index, state_index in (("before", 0, 1), ("after", 2, 3)):
            metrics = summarize([pair[row_index] for pair in outcomes], [])
            metrics["context_states"] = dict(Counter(pair[state_index] for pair in outcomes))
            metrics["no_answer_context_returned"] = sum(
                not pair[row_index]["relevant_source_ids"] and pair[state_index] == "evidence_returned"
                for pair in outcomes
            )
            summary[mode][name] = metrics
    return summary, pairs


def lock(root):
    development = read_split(root, "development")
    scores = best_scores(development["vector"])
    cutoffs = sorted({math.nextafter(scores[case_id], math.inf)
                      for case_id, row in development["vector"].items()
                      if not row["relevant_source_ids"] and scores[case_id] is not None})
    eligible = []
    for cutoff in cutoffs:
        summary, _ = compare(development, cutoff)
        loss = max(group["before"][metric] - group["after"][metric]
                   for group in summary.values() for metric in QUALITY)
        avoided = sum(group["before"]["no_answer_false_positives"] - group["after"]["no_answer_false_positives"]
                      for group in summary.values())
        if avoided > 0 and loss <= MAX_DEVELOPMENT_LOSS + 1e-12:
            eligible.append((-avoided, loss, cutoff))
    # If the guardrail cannot be met, still freeze the smallest candidate for
    # a held-out diagnostic. Do not present that candidate as a recommendation.
    threshold = min(eligible)[2] if eligible else cutoffs[0] if cutoffs else None
    return {
        "calibration": "development",
        "rule": "withhold successful output when the highest authorized native vector hit is below the threshold",
        "threshold": threshold,
        "meets_development_guardrail": bool(eligible),
        "max_development_loss": MAX_DEVELOPMENT_LOSS,
        "candidate_count": len(cutoffs),
        "raw_sha256": {mode: digest(root / mode / "results.jsonl") for mode in MODES},
    }


def report(root, frozen):
    if frozen["calibration"] != "development" or any(
        frozen["raw_sha256"][mode] != digest(root / mode / "results.jsonl") for mode in MODES
    ):
        raise ValueError("calibration lock does not match the recorded baseline")
    output = {"lock": frozen, "latency": "recorded baseline request timings reused in offline replay", "splits": {}}
    for split in ("development", "heldout"):
        summary, pairs = compare(read_split(root, split), frozen["threshold"])
        output["splits"][split] = {"summary": summary, "pairs": pairs}
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("lock", "report"))
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--lock", type=Path)
    args = parser.parse_args()
    if args.action == "report":
        if args.lock is None:
            parser.error("report requires a pre-existing --lock")
        output = report(args.run, json.loads(args.lock.read_bytes()))
    else:
        output = lock(args.run)
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(output, stream, ensure_ascii=False, indent=2)
        stream.write("\n")


if __name__ == "__main__":
    main()
