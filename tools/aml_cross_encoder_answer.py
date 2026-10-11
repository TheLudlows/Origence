"""Fixed Answer proxy for offline cross-encoder evidence, separate from official AML."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import time
from aml_answer import ANSWER, assess, validate_answer
from aml_cross_encoder import rank_indices
from aml_diagnose import DATASETS, indexed, load_dataset, read_json
from aml_drill import DrillError
from aml_experiment import Chat, evidence_payload
from aml_quality import decode_hits, digest, source_key


def selected_candidates(row, seed, depth):
    if row["candidate_ids"] != [hit["id"] for hit in seed] or len(row["scores"]) != len(seed):
        raise DrillError("cross_encoder_candidates_mismatch")
    expected = rank_indices(row["scores"], depth)
    if row["ranked_indices"][str(depth)] != expected:
        raise DrillError("cross_encoder_ranking_mismatch")
    return [seed[i] for i in expected[:5]]


def evaluate(args, report):
    experiment = read_json(args.input)
    if experiment.get("status") != "completed" or set(experiment.get("datasets", {})) != set(DATASETS):
        raise DrillError("incomplete_cross_encoder_report")
    report.update(input_sha256=digest(args.input), depth=args.depth, output_limit=5,
                  answer_prompt_sha256=hashlib.sha256(ANSWER.encode()).hexdigest())
    chat = Chat(os.environ, report)
    report["datasets"] = {}
    for dataset in DATASETS:
        corpus, questions, refs, sources, _, hashes = load_dataset(dataset)
        part = experiment["datasets"][dataset]
        if part["inputs"] != hashes:
            raise DrillError("cross_encoder_source_mismatch")
        ce_rows = indexed(part["queries"], {q["id"] for q in questions})
        output = {"inputs": hashes, "queries": []}
        report["datasets"][dataset] = output
        for q in questions:
            candidates = selected_candidates(ce_rows[q["id"]], sources[q["id"]]["seed"], args.depth)
            decode_hits(candidates, corpus["batches"], q["user_id"])
            row = {"id": q["id"], "answerable_reference": bool(q["required"]), "selected_ids": [h["id"] for h in candidates], "status": "running"}
            output["queries"].append(row)
            result = chat.call("cross_encoder_answer", q["id"], ANSWER,
                               {"question": q["query"], "evidence": evidence_payload(candidates)})
            cited = validate_answer(result, candidates)
            keys = decode_hits(cited, corpus["batches"], q["user_id"])
            metrics = assess(result, refs[q["id"]], [source_key(x) for x in q["required"]], keys)
            row.update(status="completed", output=result, cited_ids=[h["id"] for h in cited], metrics=metrics)
            print(json.dumps({"completed": q["id"], "calls": len(chat.calls)}), flush=True)
        can = [r for r in output["queries"] if r["answerable_reference"]]
        no = [r for r in output["queries"] if not r["answerable_reference"]]
        output["summary"] = {"answerable": len(can), "no_answer": len(no),
            "supported_proxy_successes": sum(r["metrics"]["supported_proxy_success"] for r in can),
            "answerable_abstentions": sum(r["metrics"]["abstained"] for r in can),
            "no_answer_correct_abstentions": sum(r["metrics"]["abstained"] for r in no)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True)
    parser.add_argument("--report", required=True)
    parser.add_argument("--depth", type=int, choices=(40, 100), default=40)
    parser.add_argument("--allow-chat-calls", action="store_true")
    args = parser.parse_args()
    if not args.allow_chat_calls:
        parser.error("--allow-chat-calls required")
    report = {"schema": "aml-cross-encoder-answer-v1", "status": "running", "runner_sha256": digest(__file__),
              "dependencies": {n: digest(Path(__file__).with_name(n)) for n in
                               ("aml_diagnose.py", "aml_answer.py", "aml_experiment.py", "aml_quality.py", "aml_cross_encoder.py")},
              "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "limitations": ["exploratory Answer comparison added after retrieval results; not preregistered unseen evaluation",
                              "fixed historical prompt/model settings but fresh calls; no deterministic comparison claim",
                              "substring/citation proxy does not validate all numeric semantics or extra claims"]}
    with open(args.report, "x", encoding="utf-8") as stream:
        try:
            evaluate(args, report)
            report["status"] = "completed"
        except DrillError as error:
            report.update(status="failed", error=str(error))
        except Exception:
            report.update(status="failed", error="cross_encoder_answer_failed")
        finally:
            json.dump(report, stream, ensure_ascii=False, indent=2)
            stream.write("\n")
    print(json.dumps({"status": report["status"], "error": report.get("error")}))
    return 0 if report["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
