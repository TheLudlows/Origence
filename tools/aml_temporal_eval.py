"""Frozen diagnostic over observed evidence; no retrieval changes or historical rescoring."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import time
from aml_answer import ANSWER, validate_answer
from aml_budget_eval import JUDGE, load_data, read
from aml_drill import DrillError
from aml_evidence import decode_fragments, source_index
from aml_experiment import Chat
from aml_quality import digest
from aml_temporal import payload, SUPPORT, support_result

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "evals/aml/temporal-v1"


def evaluate(report):
    frozen = read(OUT / "freeze.json")
    for name, expected in frozen["files"].items():
        if digest(ROOT / name) != expected:
            raise DrillError("frozen_input_changed")
    report["freeze_sha256"] = digest(OUT / "freeze.json")
    chat = Chat(os.environ, report)
    if chat.model != frozen["model"]:
        raise DrillError("model_changed")
    report["prompt_sha256"] = {name: hashlib.sha256(value.encode()).hexdigest()
                               for name, value in {"answer": ANSWER, "old_judge": JUDGE, "support": SUPPORT}.items()}
    corpus, _ = load_data("public")
    questions = {q["id"]: q for q in corpus["questions"]}
    prepared = read(ROOT / "evals/aml/budget-v1/public-prepare.json.gz")
    retrieved = {r["id"]: r for r in prepared["rows"]}
    rows = report["temporal"] = []
    for qid in frozen["question_ids"]:
        q, original = questions[qid], retrieved[qid]
        if original["status"] != "completed":
            raise DrillError("upstream_failed")
        hits = original["hits"]
        decode_fragments(hits, source_index(corpus["batches"], q["user_id"]))
        # Rotate arm order deterministically; no extra retry or posthoc prompt adjustment.
        arms = ["raw", "dates", "offsets"]
        offset = int(hashlib.sha256(qid.encode()).hexdigest(), 16) % 3
        for arm in arms[offset:] + arms[:offset]:
            data = payload(q["query"], q["question_date"], hits, arm)
            row = {"id": qid, "arm": arm, "hit_ids": [h["id"] for h in hits],
                   "payload_sha256": hashlib.sha256(json.dumps(data, ensure_ascii=False).encode()).hexdigest(),
                   "date_context": data.get("date_context")}
            rows.append(row)
            try:
                output = chat.call("temporal_" + arm, qid, ANSWER, data)
                if not isinstance(output, dict):
                    raise DrillError("invalid_answer")
                cited = validate_answer(output, hits)
                row.update(status="completed", output=output, cited_ids=[h["id"] for h in cited])
            except DrillError as error:
                row.update(status="failed", error=str(error))
            print(json.dumps({"id": qid, "arm": arm, "status": row["status"]}), flush=True)
    calibration = report["calibration"] = []
    for case in read(OUT / "calibration.json")["cases"]:
        for arm in ("old", "support"):
            row = {"id": case["id"], "arm": arm, "expected": case["expected"]}
            calibration.append(row)
            try:
                if arm == "old":
                    data = {"question": case["question"], "reference": {"answerable": True,
                            "expected_answer": case["reference"], "rubric": "Respect question units, event, time and conditions."},
                            "answers": [{"index": 0, "answer": {"answerable": True, "answer": case["answer"],
                            "indices": [e["index"] for e in case["evidence"]]}, "cited_original_fragments": case["evidence"]}]}
                    output = chat.call("calibration_old", case["id"], JUDGE, data)
                    entries = output.get("judgments") if isinstance(output, dict) else None
                    if (not isinstance(entries, list) or len(entries) != 1 or entries[0].get("index") != 0
                            or type(entries[0].get("supported")) is not bool or type(entries[0].get("correct")) is not bool):
                        raise DrillError("invalid_old_judgment")
                    supported = entries[0]["supported"]
                    state = None
                else:
                    data = {"question": case["question"], "answer": case["answer"], "cited_evidence": case["evidence"]}
                    output = chat.call("calibration_support", case["id"], SUPPORT, data)
                    state = support_result(output)
                    supported = state == "supported"
                row.update(status="completed", output=output, predicted_state=state,
                           predicted_supported=supported,
                           binary_agreement=supported == (case["expected"] == "supported"),
                           state_agreement=state == case["expected"] if state else None)
            except DrillError as error:
                row.update(status="failed", error=str(error))
            print(json.dumps({"id": case["id"], "arm": arm, "status": row["status"]}), flush=True)
    report["summary"] = {"temporal_assigned": len(rows), "temporal_errors": sum(r["status"] != "completed" for r in rows), "calibration": {}}
    for arm in ("old", "support"):
        selected = [r for r in calibration if r["arm"] == arm]
        report["summary"]["calibration"][arm] = {
            "assigned": len(selected), "errors": sum(r["status"] != "completed" for r in selected),
            "binary_agreement": sum(r.get("binary_agreement", False) for r in selected),
            "state_agreement": sum(r.get("state_agreement", False) or False for r in selected) if arm == "support" else None,
            "false_accept": sum(r.get("predicted_supported") is True and r["expected"] != "supported" for r in selected),
            "false_reject": sum(r.get("predicted_supported") is False and r["expected"] == "supported" for r in selected)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-chat-calls", action="store_true")
    args = parser.parse_args()
    if not args.allow_chat_calls:
        parser.error("--allow-chat-calls required")
    report = {"schema": "aml-temporal-diagnostic-v1", "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    with open(OUT / "run.json", "x", encoding="utf-8", newline="\n") as stream:
        try:
            evaluate(report)
            report["status"] = "completed"
        except DrillError as error:
            report.update(status="failed", error=str(error))
        except Exception:
            report.update(status="failed", error="temporal_diagnostic_failed")
        finally:
            json.dump(report, stream, ensure_ascii=False, indent=2)
            stream.write("\n")
    print(json.dumps({"status": report["status"], "summary": report.get("summary")}))
    return 0 if report["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
