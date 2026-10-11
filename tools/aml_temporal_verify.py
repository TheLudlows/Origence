"""Rebuild model inputs and derived metrics without making calls."""
import hashlib
import json
from aml_answer import validate_answer
from aml_budget_eval import load_data, read
from aml_evidence import decode_fragments, source_index
from aml_quality import digest
from aml_temporal import payload, support_result
from aml_temporal_eval import ROOT, OUT


def verify():
    freeze, report = read(OUT / "freeze.json"), read(OUT / "run.json")
    assert report["status"] == "completed"
    assert report["freeze_sha256"] == digest(OUT / "freeze.json")
    for name, expected in freeze["files"].items():
        assert digest(ROOT / name) == expected, name
    corpus, _ = load_data("public")
    questions = {q["id"]: q for q in corpus["questions"]}
    original = {r["id"]: r["hits"] for r in read(ROOT / "evals/aml/budget-v1/public-prepare.json.gz")["rows"]}
    actual = [(r["id"], r["arm"]) for r in report["temporal"]]
    assert len(actual) == len(set(actual)) == 30
    assert set(actual) == {(qid, arm) for qid in freeze["question_ids"] for arm in freeze["arms"]}
    for row in report["temporal"]:
        assert row["status"] == "completed"
        q, hits = questions[row["id"]], original[row["id"]]
        decode_fragments(hits, source_index(corpus["batches"], q["user_id"]))
        data = payload(q["query"], q["question_date"], hits, row["arm"])
        assert row["payload_sha256"] == hashlib.sha256(json.dumps(data, ensure_ascii=False).encode()).hexdigest()
        assert row["date_context"] == data.get("date_context")
        assert row["hit_ids"] == [h["id"] for h in hits]
        assert row["cited_ids"] == [h["id"] for h in validate_answer(row["output"], hits)]
    cases = {c["id"]: c for c in read(OUT / "calibration.json")["cases"]}
    keys = [(r["id"], r["arm"]) for r in report["calibration"]]
    assert len(keys) == len(set(keys)) == 24
    assert set(keys) == {(key, arm) for key in cases for arm in ("old", "support")}
    for row in report["calibration"]:
        assert row["status"] == "completed"
        assert row["expected"] == cases[row["id"]]["expected"]
        supported = (support_result(row["output"]) == "supported" if row["arm"] == "support"
                     else row["output"]["judgments"][0]["supported"])
        assert supported == row["predicted_supported"]
        assert row["binary_agreement"] == (supported == (row["expected"] == "supported"))
    calls = report["chat_calls"]
    assert len(calls) == freeze["call_budget"] == 54
    assert all(c["status"] == "completed" and c["input_utf8_bytes"] <= 40000 for c in calls)
    assert sum(c["input_utf8_bytes"] for c in calls) <= 1200000
    # Persist observed output fields unchanged even if their reason contradicts them.
    old = [r for r in report["calibration"] if r["arm"] == "old"]
    return {"status": "passed", "freeze_sha256": digest(OUT / "freeze.json"), "run_sha256": digest(OUT / "run.json"),
            "temporal_rows": 30, "calibration_rows": 24, "calls": len(calls),
            "reported_total_tokens": sum(c["usage"]["total_tokens"] for c in calls),
            "input_utf8_bytes": sum(c["input_utf8_bytes"] for c in calls),
            "old_joint_correct_and_supported_false_accepts": sum(r["expected"] != "supported" and
                r["output"]["judgments"][0]["correct"] and r["predicted_supported"] for r in old),
            "notes": ["old support field and new support-only classifier are compared, not end-to-end correct-and-supported rates",
                      "synthetic authored labels are not independent validation", "reference labels never enter new support prompt or temporal Answer"]}


if __name__ == "__main__":
    value = verify()
    with open(OUT / "audit.json", "x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")
    print(json.dumps(value))
