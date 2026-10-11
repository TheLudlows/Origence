"""Offline integrity audit of completed frozen experiment artifacts; no model calls."""
from pathlib import Path
from aml_answer import validate_answer
from aml_budget import Budget, checked_order
from aml_budget_eval import ROOT, OUT, ARMS, load_data, load_artifact, validate_rows, metrics, verify_freeze
from aml_cross_encoder import rank_indices
from aml_evidence import source_index, decode_fragments
from aml_experiment import select
from aml_quality import digest
import json


def verify(dataset):
    verify_freeze()
    corpus, labels = load_data(dataset)
    reports = {}
    for stage, arm in [("retrieve", None), ("prepare", None), ("rank", "chat"), ("rank", "bge"),
                       *(("answer", a) for a in ARMS), ("grade", None)]:
        report, receipt = load_artifact(dataset, stage, arm)
        validate_rows(report["rows"], corpus["questions"])
        for upstream in report.get("upstream", []):
            if digest(OUT / upstream["file"]) != upstream["sha256"]:
                raise ValueError("artifact_chain_changed")
        calls = report.get("chat_calls", [])
        if len(calls) > 60 or sum(c["input_utf8_bytes"] for c in calls) > 1200000 or any(c["input_utf8_bytes"] > 40000 for c in calls):
            raise ValueError("chat_budget_violation")
        reports[(stage, arm)] = report
    budget = Budget(ROOT / "target/aml-rerank/model/tokenizer.json")
    errors = []
    for i, q in enumerate(corpus["questions"]):
        known = source_index(corpus["batches"], q["user_id"])
        seed = reports[("retrieve", None)]["rows"][i]
        prep = reports[("prepare", None)]["rows"][i]
        if seed["status"] != "completed":
            errors.append(q["id"] + ":retrieve")
            continue
        decode_fragments(seed["hits"], known)
        candidates, size = budget.pack(seed["hits"], 40, 8000, 32000)
        assert prep["candidates"] == candidates and prep["candidate_size"] == size
        for arm in ARMS:
            ranked = prep if arm == "vector" else reports[("rank", arm)]["rows"][i]
            answer = reports[("answer", arm)]["rows"][i]
            if ranked["status"] != "completed":
                errors.append(q["id"] + ":rank:" + arm)
                assert answer["status"] == "failed"
                continue
            if arm == "vector":
                selected = candidates
            elif arm == "chat":
                selected = select(ranked["output"], candidates) if candidates else []
            else:
                order = checked_order(rank_indices(ranked["scores"], 40), len(candidates)) if candidates else []
                selected = [candidates[n] for n in order]
            expected, size = budget.pack(selected, 5, 2000, 9000)
            assert expected == ranked["hits"] and size == ranked["evidence_size"]
            assert metrics(expected, known, labels[q["id"]], dataset) == ranked["metrics"]
            if answer["status"] != "completed":
                errors.append(q["id"] + ":answer:" + arm)
                continue
            cited = validate_answer(answer["output"], expected)
            assert cited == answer["cited"]
            assert metrics(cited, known, labels[q["id"]], dataset) == answer["citation_metrics"]
            judged = reports[("grade", None)]["rows"][i]["arms"][arm]
            if judged["status"] != "completed":
                errors.append(q["id"] + ":grade:" + arm)
            else:
                assert judged["success"] == (judged["correct"] and judged["supported"])
                if judged["correct"]:
                    assert answer["output"]["answerable"] == labels[q["id"]]["answerable"]
    return {"dataset": dataset, "questions": len(corpus["questions"]), "artifacts": len(reports),
            "integrity": "passed", "execution_errors": errors}


if __name__ == "__main__":
    result = [verify(dataset) for dataset in ("v4", "public")]
    with open(OUT / "integrity-audit.json", "x", encoding="utf-8") as stream:
        json.dump(result, stream, ensure_ascii=False, indent=2)
        stream.write("\n")
    print(json.dumps(result))
