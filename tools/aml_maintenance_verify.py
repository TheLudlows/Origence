"""Offline audit of frozen maintenance-scale timing, receipts and complete HTTP responses."""
import argparse
import gzip
import json
import math
from pathlib import Path
from aml_drill import DrillError
from aml_evidence import decode_fragments, source_index
from aml_maintenance_scale import PHASES, payload_hash
from aml_public import load_public
from aml_quality import digest

ROOT = Path(__file__).resolve().parents[1]


def require(value, code):
    if not value:
        raise DrillError(code)


def validate_schedule(rows, question_ids):
    expected = [(phase, qid) for phase in PHASES for qid in question_ids]
    position = expected.index(("grown", question_ids[0]))
    expected[position:position] = [("during_growth", qid) for qid in question_ids[:8]]
    require([(r["phase"], r["question_id"]) for r in rows] == expected, "invalid_search_schedule")


def validate_receipts(receipts):
    require(set(receipts) == {"compacted_preview", "compacted", "reopened_preview",
                             "recompacted_preview", "recompacted"}, "receipt_set_mismatch")
    for name, receipt in receipts.items():
        preview = name.endswith("preview")
        require(receipt["dry_run"] == preview and receipt["historical_versions_retained"], "receipt_flags")
        require(receipt["concurrency"] == 2 and receipt["target_rows_per_fragment"] == 4096, "receipt_limits")
        require(len(receipt["tables"]) == 1, "unexpected_profile_count")
        require(receipt["tables"][0]["table_index"] == 0, "receipt_table_index")
        table = receipt["tables"][0]
        require((table["after"] is None) == preview, "receipt_dry_run_mutated")
        if not preview:
            require(table["before"]["rows"] == table["after"]["rows"], "receipt_row_count_changed")
            require(table["after"]["fragments"] < table["before"]["fragments"], "no_fragment_reduction")
            require(table["after"]["version"] > table["before"]["version"], "no_compaction_commit")
    table = lambda name: receipts[name]["tables"][0]
    require(table("compacted_preview")["before"] == table("compacted")["before"], "dry_run_changed_layout")
    require(table("compacted")["after"] == table("reopened_preview")["before"], "restart_changed_layout")
    require(table("recompacted_preview")["before"] == table("recompacted")["before"], "second_dry_run_changed_layout")
    before = table("compacted")["after"]
    grown = table("recompacted")["before"]
    require(grown["rows"] - before["rows"] == 256 and grown["fragments"] > before["fragments"], "regrowth_missing")


def distribution(values):
    values = sorted(values)
    return {"n": len(values), "p50": values[math.ceil(len(values) * .5) - 1],
            "p95": values[math.ceil(len(values) * .95) - 1], "min": values[0], "max": values[-1]}


def verify(directory):
    plan = json.loads((directory / "plan.json").read_text(encoding="utf-8"))
    report = json.loads((directory / "run.json").read_text(encoding="utf-8"))
    require(report["status"] == "completed" and report["temporary_tree_removed"], "run_incomplete")
    require(report["exact_payload_order_match"] and report["source_scope_verified"], "runtime_checks_missing")
    require(report["users"] == 32 and report["batches"] == 1524 and report["messages"] == 15685
            and report["appended_batches"] == 256, "fixture_counts_changed")
    require(plan["runner_sha256"] == report["runner_sha256"] == digest(ROOT / "tools/aml_maintenance_scale.py"), "runner_hash_changed")
    require(plan["binary_sha256"] == report["binary_sha256"], "binary_link_changed")
    require(plan["public_preparation_sha256"] == report["public_preparation_sha256"] == digest(ROOT / "evals/aml/public/preparation.json"), "public_freeze_changed")
    require(report["responses_sha256"] == digest(directory / "responses.json.gz"), "response_archive_changed")
    corpus, _ = load_public(ROOT)
    questions = sorted(corpus["questions"], key=lambda q: q["id"])
    known = {q["id"]: source_index(corpus["batches"], q["user_id"]) for q in questions}
    ids = [q["id"] for q in questions]
    validate_schedule(report["searches"], ids)
    validate_receipts(report["receipts"])
    with gzip.open(directory / "responses.json.gz", "rt", encoding="utf-8") as file:
        responses = json.load(file)
    validate_schedule(responses, ids)
    baseline = {}
    for row, response in zip(report["searches"], responses):
        data = response["data"]
        require(len(data) == 100 and row["returned_count"] == 100, "response_count_changed")
        decode_fragments(data, known[row["question_id"]], allow_legacy=True)
        require(payload_hash(data) == row["payload_sha256"], "response_hash_mismatch")
        if row["phase"] == "baseline":
            baseline[row["question_id"]] = data
        else:
            require(data == baseline[row["question_id"]], "payload_or_order_changed")
        for key, value in row.items():
            if key.endswith("_ms"):
                require(isinstance(value, (int, float)) and math.isfinite(value) and value >= 0, "invalid_timing")
    return {"schema": "aml-maintenance-scale-audit-v1", "status": "passed", "queries": len(responses),
            "evidence_items": sum(len(r["data"]) for r in responses),
            "percentile_method": "nearest-rank; all rows including cold first request; phases share fixed question order",
            "phases": {p: {key: distribution([r[key] for r in report["searches"] if r["phase"] == p])
                           for key in ("http_ms", "vector_ms")} for p in (*PHASES, "during_growth")},
            "run_sha256": digest(directory / "run.json"), "responses_sha256": report["responses_sha256"],
            "verifier_sha256": digest(__file__),
            "checks": ["frozen_inputs", "complete_schedule", "original_utf8_metadata_and_scope",
                       "complete_payload_and_order_equality", "dry_run_and_restart_layout", "rows_preserved", "regrowth_then_recompaction"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    with args.output.open("x", encoding="utf-8", newline="\n") as output:
        try:
            result = verify(args.directory)
        except DrillError as error:
            result = {"status": "failed", "error": str(error)}
        except Exception:
            result = {"status": "failed", "error": "invalid_audit_input"}
        json.dump(result, output, indent=2)
        output.write("\n")
    print(json.dumps({"status": result["status"], "error": result.get("error")}))
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
