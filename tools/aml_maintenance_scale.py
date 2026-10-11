"""Full observed public history: offline compaction, restart, continued writes and re-compaction."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
from aml_drill import DrillError, Host, preflight
from aml_evidence import decode_fragments, source_index
from aml_public import load_public
from aml_quality import digest
from aml_compaction_profile import timings

ROOT = Path(__file__).resolve().parents[1]
PHASES = ("baseline", "compacted", "reopened", "grown", "recompacted")


def payload_hash(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True,
                                    separators=(",", ":")).encode()).hexdigest()


def compact(host, dry_run=False):
    command = [host.binary, "--data-dir", host.env["OC_DATA_DIR"], "--offline", "vector-compact"]
    if dry_run:
        command.append("--dry-run")
    result = subprocess.run(command, env=host.env, capture_output=True, timeout=300)
    if result.returncode:
        raise DrillError("compaction_command_failed")
    receipt = json.loads(result.stdout)
    if receipt["dry_run"] != dry_run or not receipt["historical_versions_retained"]:
        raise DrillError("compaction_contract_failed")
    if not dry_run and any(t["before"]["rows"] != t["after"]["rows"] for t in receipt["tables"]):
        raise DrillError("compaction_changed_row_count")
    return receipt


def experiment(binary, report, responses):
    corpus, _ = load_public(ROOT)
    questions = sorted(corpus["questions"], key=lambda q: q["id"])
    batches = corpus["batches"]
    report.update(binary_sha256=digest(binary), runner_sha256=digest(__file__),
                  public_preparation_sha256=digest(ROOT / "evals/aml/public/preparation.json"),
                  embedding=preflight(os.environ), users=len(questions), batches=len(batches),
                  messages=sum(len(b["messages"]) for b in batches), searches=[], receipts={})
    known = {q["id"]: source_index(batches, q["user_id"]) for q in questions}
    parent = ROOT / "target/aml-drills"
    parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="maintenance-scale-", dir=parent) as directory:
        created = Path(directory).resolve()
        if created.parent != parent.resolve():
            raise DrillError("unsafe_temporary_path")
        host = Host(binary, created, {k: v for k, v in os.environ.items() if not k.startswith("AML_CHAT_")})
        try:
            host.bootstrap()
            host.env["RUST_LOG"] = "error,origence::search_timing=debug"
            host.env["NO_COLOR"] = "1"
            host.start()
            host.call("/admin/aml/namespace", {})
            def add(batch):
                result, elapsed = host.call("/aml/add", batch)
                if result != {"success": True, **{k: batch[k] for k in ("request_id", "user_id", "session_id")}}:
                    raise DrillError("add_contract_failed")
                return elapsed
            started = time.perf_counter()
            with ThreadPoolExecutor(max_workers=4) as pool:
                for n, _ in enumerate(pool.map(add, batches), 1):
                    if n % 32 == 0:
                        print(json.dumps({"ingested": n, "total": len(batches)}), flush=True)
            report["ingest_ms"] = (time.perf_counter() - started) * 1000
            previous = {}
            cursor = 0
            def search(q, phase):
                nonlocal cursor
                result, elapsed = host.call("/aml/search", {"user_id": q["user_id"], "query": q["query"], "top_k": 100})
                hits = result["data"]
                decode_fragments(hits, known[q["id"]])
                if len(hits) != 100 or (q["id"] in previous and previous[q["id"]] != hits):
                    raise DrillError("changed_search_results")
                previous[q["id"]] = hits
                host.log.seek(0)
                measured = timings(host.log.read())
                if len(measured) != cursor + 1:
                    raise DrillError("missing_search_profile")
                cursor += 1
                row = {"phase": phase, "question_id": q["id"], "http_ms": elapsed,
                       "payload_sha256": payload_hash(hits), **measured[-1]}
                report["searches"].append(row)
                responses.append({"phase": phase, "question_id": q["id"], "data": hits})
                print(json.dumps({"phase": phase, "question_id": q["id"], "http_ms": elapsed}), flush=True)
            for phase in PHASES:
                if phase in ("compacted", "reopened", "recompacted"):
                    host.stop()
                    report["receipts"][phase + "_preview"] = compact(host, True)
                    if phase != "reopened":
                        report["receipts"][phase] = compact(host)
                    host.start()
                    cursor = 0
                if phase == "grown":
                    background = [{"request_id": f"regrowth-{i}", "user_id": f"regrowth-user-{i % 4}",
                        "session_id": f"regrowth-{i}", "messages": [{"role": "user",
                        "content": f"Unrelated regrowth marker {i}: warehouse shelf {i}."}]} for i in range(256)]
                    # One writer makes progress while eight original-user queries validate isolation.
                    def append_background():
                        for i, batch in enumerate(background, 1):
                            add(batch)
                            if i % 32 == 0:
                                print(json.dumps({"regrowth_added": i}), flush=True)
                    with ThreadPoolExecutor(max_workers=1) as pool:
                        writer = pool.submit(append_background)
                        for q in questions[:8]:
                            search(q, "during_growth")
                        writer.result()
                    report["appended_batches"] = len(background)
                for q in questions:
                    search(q, phase)
            report["exact_payload_order_match"] = True
            report["source_scope_verified"] = True
        finally:
            host.stop()
    report["temporary_tree_removed"] = not created.exists()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--report", required=True)
    parser.add_argument("--responses", required=True)
    args = parser.parse_args()
    report = {"schema": "aml-maintenance-scale-v1", "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "phases": PHASES, "limitations": ["all 32 histories already observed; no new quality score",
              "debug build, one query per user per phase, no SLO or peak memory claim",
              "host process killed only after completed requests; maintenance not interrupted",
              "real BGE-M3 is one profile; multi-profile coverage is a separate library regression"]}
    responses = []
    # Reserve both destinations before starting a host; never overwrite evidence.
    with open(args.report, "x", encoding="utf-8", newline="\n") as output, gzip.open(args.responses, "xt", encoding="utf-8") as raw:
        try:
            experiment(args.binary, report, responses)
            report["status"] = "completed"
        except DrillError as error:
            report.update(status="failed", error=str(error))
        except Exception:
            report.update(status="failed", error="scale_experiment_failed")
        finally:
            json.dump(responses, raw, ensure_ascii=False)
            raw.close()
            report["responses_sha256"] = digest(args.responses)
            report["finished_at_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
            json.dump(report, output, indent=2)
            output.write("\n")
    print(json.dumps({"status": report["status"], "error": report.get("error")}))
    return 0 if report["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
