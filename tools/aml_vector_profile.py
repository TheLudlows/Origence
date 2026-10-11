"""Sequential baseline/candidate/baseline on one isolated, unchanged AML database."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import re
import tempfile
import time
from aml_drill import DrillError, Host, preflight
from aml_evidence import decode_fragments, source_index
from aml_public import load_public
from aml_quality import digest

ROOT = Path(__file__).resolve().parents[1]
FIELDS = ("authorization_ms", "embedding_ms", "eligibility_ms", "vector_ms", "branches_ms", "verification_ms", "total_ms",
          "vector_batches", "native_count", "returned_count")


def timings(data, target="origence::search_timing", message="search completed", fields=FIELDS):
    text = re.sub(r"\x1b\[[0-9;]*m", "", data.decode("utf-8"))
    rows = []
    for line in text.splitlines():
        if target not in line or message not in line:
            continue
        values = dict(re.findall(r"([a-z_]+)=([0-9.eE+-]+)", line))
        if set(values) != set(fields):
            raise DrillError("profile_fields_changed")
        rows.append({key: float(value) for key, value in values.items()})
    return rows


def profile(binary, candidate, report, background_batches=512):
    corpus, _ = load_public(ROOT)
    # Predeclared choice: largest observed history in source UTF-8 bytes, ID tie-break.
    sizes = {q["user_id"]: 0 for q in corpus["questions"]}
    for batch in corpus["batches"]:
        sizes[batch["user_id"]] += sum(len(m["content"].encode()) for m in batch["messages"])
    q = sorted(corpus["questions"], key=lambda q: (-sizes[q["user_id"]], q["id"]))[0]
    batches = [b for b in corpus["batches"] if b["user_id"] == q["user_id"]]
    report.update(question_id=q["id"], source_bytes=sizes[q["user_id"]], batches=len(batches),
                  messages=sum(len(b["messages"]) for b in batches), binary_sha256=digest(binary),
                  runner_sha256=digest(__file__), retrieval_sha256=digest(ROOT / "src/retrieval.rs"),
                  embedding=preflight(os.environ))
    parent = ROOT / "target/aml-drills"
    parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="profile-", dir=parent) as directory:
        created = Path(directory).resolve()
        if created.parent != parent.resolve():
            raise DrillError("unsafe_temporary_path")
        host = Host(binary, created, {k: v for k, v in os.environ.items() if not k.startswith("AML_CHAT_")})
        try:
            host.bootstrap()
            host.env["RUST_LOG"] = "error,origence::search_timing=debug,origence::vector_timing=debug"
            host.env["NO_COLOR"] = "1"
            host.start()
            host.call("/admin/aml/namespace", {})
            def add(batch):
                result, _ = host.call("/aml/add", batch)
                if result != {"success": True, **{k: batch[k] for k in ("request_id", "user_id", "session_id")}}:
                    raise DrillError("add_contract_failed")
            with ThreadPoolExecutor(max_workers=4) as pool:
                list(pool.map(add, batches))
            background = [{"request_id": f"background-{i}", "user_id": f"profile-background-{i % 4}",
                           "session_id": f"background-{i}", "messages": [{"role": "user",
                           "content": f"Unrelated isolation marker BACKGROUND{i}: warehouse shelf {i}."}]}
                          for i in range(background_batches)]
            with ThreadPoolExecutor(max_workers=4) as pool:
                for number, _ in enumerate(pool.map(add, background), 1):
                    if number % 64 == 0:
                        print(json.dumps({"background_added": number}), flush=True)
            report["background_batches"] = background_batches
            known = source_index(batches, q["user_id"])
            report["searches"] = []
            previous_hits = {}
            for phase in ("baseline", "candidate", "baseline_recheck"):
                if phase != "baseline":
                    host.stop()
                    if phase == "candidate":
                        # Only test orchestration waits for a separately built executable.
                        # No internal production CLI reentry or independent durable worker.
                        deadline = time.monotonic() + 900
                        print(json.dumps({"waiting_for_candidate": True}), flush=True)
                        while not Path(candidate).is_file():
                            if time.monotonic() > deadline:
                                raise DrillError("candidate_wait_timeout")
                            time.sleep(1)
                        report["candidate_binary_sha256"] = digest(candidate)
                    host.binary = str(Path(candidate if phase == "candidate" else binary).resolve(strict=True))
                    host.start()
                backend_cursor = 0
                for number, top_k in enumerate([5, 100] * 4):
                    result, elapsed = host.call("/aml/search", {"user_id": q["user_id"], "query": q["query"], "top_k": top_k})
                    hits = result["data"]
                    decode_fragments(hits, known)
                    if len(hits) != top_k or (top_k in previous_hits and previous_hits[top_k] != hits):
                        raise DrillError("changed_search_results")
                    previous_hits[top_k] = hits
                    host.log.seek(0)
                    log = host.log.read()
                    measured = timings(log)
                    backend = timings(log, "origence::vector_timing", "vector query completed",
                                      ("list_ms", "open_ms", "prepare_ms", "execute_ms", "collect_ms", "decode_ms", "total_ms", "returned_count"))
                    if len(measured) != number + 1 or len(backend) - backend_cursor != measured[-1]["vector_batches"]:
                        raise DrillError("missing_search_profile")
                    report["searches"].append({"phase": phase, "iteration": number, "top_k": top_k,
                        "http_ms": elapsed, **measured[-1], "backend": backend[backend_cursor:]})
                    backend_cursor = len(backend)
                    print(json.dumps(report["searches"][-1]), flush=True)
            report["stable_ids"] = True
        finally:
            host.stop()
    report["temporary_tree_removed"] = not created.exists()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--report", required=True)
    parser.add_argument("--candidate", required=True)
    parser.add_argument("--background-batches", type=int, default=0)
    args = parser.parse_args()
    if not 0 <= args.background_batches <= 512:
        parser.error("background-batches must be 0..512")
    if Path(args.candidate).exists():
        parser.error("candidate must be a new path, created only after inspecting baseline")
    report = {"schema": "aml-vector-comparison-v1", "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "selection": "largest source UTF-8 history among observed public 32, question ID tie-break",
              "schedule": {"phases": ["baseline", "candidate", "baseline_recheck"], "top_k": [5, 100] * 4},
              "limitations": ["one observed history, repeated query, no concurrent writes after ingestion", "debug build, not production SLO", "first request includes cold search; all rows retained", "branch timing includes hydration, fusion and any enabled auxiliary branches", "timer excludes outer AML scope lookup and response/network; HTTP records end-to-end"]}
    with open(args.report, "x", encoding="utf-8", newline="\n") as output:
        try:
            profile(args.binary, args.candidate, report, args.background_batches)
            report["status"] = "completed"
        except DrillError as error:
            report.update(status="failed", error=str(error))
        except Exception:
            report.update(status="failed", error="profile_failed")
        finally:
            json.dump(report, output, indent=2)
            output.write("\n")
    print(json.dumps({"status": report["status"], "error": report.get("error")}))
    return 0 if report["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
