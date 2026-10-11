"""Gold-evidence Answer diagnosis only; not a deployable retrieval arm or AML score."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import time
from aml_answer import ANSWER, assess, validate_answer
from aml_diagnose import DATASETS, load_dataset, oracle_hits
from aml_drill import DrillError
from aml_experiment import Chat, evidence_payload
from aml_quality import decode_hits, digest, source_key


def evaluate(report):
    chat = Chat(os.environ, report)
    report["answer_prompt_sha256"] = hashlib.sha256(ANSWER.encode()).hexdigest()
    report["inputs"] = {}
    report["queries"] = []
    report["excluded_no_answer_ids"] = []
    for dataset in DATASETS:
        corpus, questions, refs, _, _, hashes = load_dataset(dataset)
        report["inputs"][dataset] = hashes
        for q in questions:
            if not q["required"]:
                # An empty gold context would make abstention artificially easy.
                report["excluded_no_answer_ids"].append(q["id"])
                continue
            candidates = oracle_hits(q, corpus)
            row = {"id": q["id"], "dataset": dataset, "evidence": candidates, "status": "running"}
            report["queries"].append(row)
            result = chat.call("diagnostic_gold_evidence", q["id"], ANSWER,
                               {"question": q["query"], "evidence": evidence_payload(candidates)})
            cited = validate_answer(result, candidates)
            keys = decode_hits(cited, corpus["batches"], q["user_id"])
            metrics = assess(result, refs[q["id"]], [source_key(x) for x in q["required"]], keys)
            row.update(status="completed", output=result, cited_ids=[x["id"] for x in cited], metrics=metrics)
            print(json.dumps({"completed": q["id"], "calls": len(chat.calls)}), flush=True)
    report["summary"] = {}
    for dataset in DATASETS:
        rows = [r for r in report["queries"] if r["dataset"] == dataset]
        report["summary"][dataset] = {"answerable": len(rows),
            "supported_proxy_successes": sum(r["metrics"]["supported_proxy_success"] for r in rows),
            "answerable_abstentions": sum(r["metrics"]["abstained"] for r in rows)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", required=True)
    parser.add_argument("--allow-chat-calls", action="store_true")
    args = parser.parse_args()
    if not args.allow_chat_calls:
        parser.error("--allow-chat-calls required")
    report = {"schema": "aml-gold-evidence-diagnosis-v1", "status": "running", "runner_sha256": digest(__file__),
              "chat_adapter_sha256": digest(Path(__file__).with_name("aml_experiment.py")),
              "answer_adapter_sha256": digest(Path(__file__).with_name("aml_answer.py")),
              "diagnosis_adapter_sha256": digest(Path(__file__).with_name("aml_diagnose.py")),
              "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "limitations": ["oracle gold-source selection is diagnostic only; never a deployable or official result",
                              "no-answer questions excluded; empty gold context cannot evaluate distractor refusal",
                              "old synthetic regression data and substring/citation proxy, no independent review",
                              "one fresh Answer pass; comparison to historical passes is exploratory"]}
    with open(args.report, "x", encoding="utf-8") as stream:
        try:
            evaluate(report)
            report["status"] = "completed"
        except DrillError as error:
            report.update(status="failed", error=str(error))
        except Exception:
            report.update(status="failed", error="oracle_diagnosis_failed")
        finally:
            json.dump(report, stream, ensure_ascii=False, indent=2)
            stream.write("\n")
    print(json.dumps({"status": report["status"], "summary": report.get("summary"), "error": report.get("error")}))
    return 0 if report["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
