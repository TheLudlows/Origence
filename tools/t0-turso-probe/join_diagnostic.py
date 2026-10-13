#!/usr/bin/env python3
"""Single authorized post-hoc JOIN diagnostic. No policy tuning or scale ladder."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import re
import tempfile
import time

SPEC = importlib.util.spec_from_file_location("t0_fts_cost_diagnostic", Path(__file__).with_name("fts_cost.py"))
fts = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fts)
SECONDS = 180
SHAPES = ("current_materialized", "forced_scored_first_cross_join")
ORDERS = [list(SHAPES), list(reversed(SHAPES))]
CASE = {"case": "posthoc-n10000-all-dense-k100", "corpus_rows": 10000, "query": "all", "bucket": "dense", "k": 100, "mode": "unbudgeted"}
POLICY = {
    "name": "single-posthoc-fts-join-order-diagnostic-v1",
    "point": CASE, "shapes": list(SHAPES), "orders": ORDERS,
    "initial_head": "fa19309ff6a0f8623f1670b0a3a34a686073ad85",
    "initial_report_sha256": "b249c41b7efbc640053593e05cdc0a784490d4f9b64a4d69be29eb4fd9b11b7d",
    "initial_dense_current_pipeline_ms": [14701.258052999947, 14677.99370900002],
    "initial_matrix_is_not_replaced": True, "posthoc": True,
    "max_driver_seconds": SECONDS, "full_index_matches": 10000,
    "expected_eligible_rows": 9000, "expected_top_k": 100,
    "scoring": "native global-index BM25; same bind slot; full scoped metadata and deterministic ties",
    "audit_order": "after all four measured paths",
    "timing": "same Python/bridge request boundaries as frozen cost matrix; not production Rust latency",
    "acceptance": "correctness/visibility/actual-plan evidence only; no latency or speedup threshold",
    "stop": "one comparison then conclude regardless of improvement; no other shape/scale tuning",
    "hard_cap": "CI groups original driver and this diagnostic under one external900-second process-group cap",
    "production_adoption": False,
}


def statements(data):
    current = fts.candidate_sql(data, "dense", 100)
    visibility, params = fts.visibility_sql(2)
    cross = visibility.replace(" JOIN ", " CROSS JOIN ")
    cross = cross.replace("FROM oc_chunks c", "FROM scored s CROSS JOIN oc_chunks c ON s.tenant_id=c.tenant_id AND s.workspace_id=c.workspace_id AND s.id=c.id", 1)
    forced = (f"WITH scored AS MATERIALIZED ({fts.SCORE_SQL}) SELECT {fts.META_COLUMNS},s.score "
              f"{cross} ORDER BY s.score DESC,c.tenant_id,c.workspace_id,c.id LIMIT 100")
    return {SHAPES[0]: current, SHAPES[1]: (forced, params)}


class DiagnosticDeadline(fts.OverallDeadline):
    def expire(self, _signal, _frame):
        if self.active[0] is not None:
            self.active[0].kill()
        raise TimeoutError("post-hoc diagnostic180-second deadline exceeded")


class CountedBridge(fts.compat.Bridge):
    deadline_seconds = SECONDS

    def __init__(self, *args, **kwargs):
        self.request_count = 0
        super().__init__(*args, **kwargs)

    def raw(self, *args, **kwargs):
        self.request_count += 1
        return super().raw(*args, **kwargs)


def audit(db, data, sqls, deadline, result):
    meter = fts.Meter(db, deadline)
    query = fts.compat.literal_fts_query(fts.QUERIES["all"])
    result.update({"position": POLICY["audit_order"], "calls": meter.calls})
    result["scores"] = meter.call("audit_direct_scores", fts.SCORE_SQL, [query])
    visibility, params = fts.visibility_sql()
    result["eligible_metadata"] = meter.call("audit_dense_eligibility", "SELECT " + fts.META_COLUMNS + " " + visibility, params)
    result["physical_index"] = meter.call("audit_physical_index", "SELECT name,sql FROM sqlite_master WHERE name='t0_cost_chunks_fts'")
    result["plans"] = {}
    result["plans"]["direct"] = meter.call("plan_direct", "EXPLAIN QUERY PLAN " + fts.SCORE_SQL, [query])
    for shape in SHAPES:
        sql, params = sqls[shape]
        result["plans"][shape] = meter.call("plan_" + shape, "EXPLAIN QUERY PLAN " + sql, [query] + params)
    result["matching_rows"] = len(result["scores"])


def check_audit(observed, data):
    assert observed["position"] == POLICY["audit_order"]
    assert observed["matching_rows"] == len(observed["scores"]) == 10000
    assert {fts.scoped_key(row) for row in observed["scores"]} == {fts.scoped_key(row["meta"]) for row in data["rows"]}
    assert all(math.isfinite(row[3]) and row[3] > 0 for row in observed["scores"])
    assert sorted(map(fts.metadata_key, observed["eligible_metadata"])) == sorted(fts.metadata_key(row["meta"]) for row in fts.bucket_rows(data, "dense"))
    indexes = observed["physical_index"]
    assert len(indexes) == 1 and indexes[0][0] == "t0_cost_chunks_fts" and re.search(r"\bUSING\s+fts\b", indexes[0][1], re.I)
    assert set(observed["plans"]) == {"direct", *SHAPES}
    assert all("QUERY INDEX METHOD fts" in fts.encoded(plan) for plan in observed["plans"].values())
    forced = [row[-1] for row in observed["plans"][SHAPES[1]]]
    scored = [i for i, text in enumerate(forced) if "SCAN scored AS s" in text]
    chunks = [i for i, text in enumerate(forced) if re.match(r"(?:SCAN|SEARCH) c\b", text)]
    assert scored and chunks, "actual forced plan lacks recognizable scored/chunks scans"
    assert scored[0] < chunks[0], "actual plan does not put scored input before chunks"
    assert [call["label"] for call in observed["calls"]] == ["audit_direct_scores", "audit_dense_eligibility", "audit_physical_index", "plan_direct", "plan_" + SHAPES[0], "plan_" + SHAPES[1]]
    assert all(call["ok"] and math.isfinite(call["engine_elapsed_ms"]) and call["engine_elapsed_ms"] >= 0 for call in observed["calls"])


def report_errors(report):
    errors = ["fatal: " + str(error) for error in report.get("errors", [])]
    try:
        assert report["policy"] == POLICY and report["point"] == CASE
        assert report["completed"] and report["status"] == "PASS", "incomplete/failed diagnostic"
        assert 0 <= report["driver_wall_seconds"] <= SECONDS, "diagnostic deadline exceeded"
        data = fts.fixture(10000)
        assert report["fixture_sha256"] == fts.digest(data)
        assert report["sql"] == {shape: sql for shape, (sql, _params) in statements(data).items()}, "diagnostic SQL changed"
        assert report["provenance"]["resolved_versions"] == {"turso": "0.8.2", "turso_core": "0.8.2", "turso_sdk_kit": "0.8.2"}
        assert re.fullmatch(r"[0-9a-f]{64}", report["provenance"]["binary_sha256"])
        assert re.fullmatch(r"[0-9a-f]{64}", report["provenance"]["join_diagnostic_sha256"])
        assert report["connection_setup"]["calls"][0]["label"] == "connection_setup" and report["connection_setup"]["calls"][0]["rows"] == 1
        assert 0 <= report["measured_started_ms"] <= report["measured_finished_ms"] <= report["audit_started_ms"] <= report["audit_finished_ms"]
        check_audit(report["audit"], data)
        expected = fts.reference_hits(data, report["audit"])[("dense", 100)]
        assert len(expected) == 100
        assert len(report["trials"]) == 2
        for i, trial in enumerate(report["trials"]):
            assert trial["order"] == ORDERS[i]
            assert [item["shape"] for item in trial["runs"]] == ORDERS[i]
            for item in trial["runs"]:
                assert item["status"] == "PASS"
                run = item["measurement"]
                assert run["arm"] == "candidate" and run["outcome"] == "OK"
                fts.check_measurement(run, CASE, report["audit"], data, expected)
        counts = report["request_counts"]
        assert counts["measured"] == 32 and counts["measured_queries"] == 16 and counts["measured_transaction_executes"] == 16
        assert counts["post_measurement_audit"] == len(report["audit"]["calls"]) == 6
        assert counts["phase_connection_setup"] == 1 and counts["implicit_connection_pragmas"] == 2
        assert counts["measurement_process_total"] == counts["measured"] + counts["post_measurement_audit"] + counts["phase_connection_setup"] == 39
        assert counts["all_bridge_requests"] == counts["fixture_schema_index"] + counts["measurement_process_total"]
        actual_runs = [item["measurement"] for trial in report["trials"] for item in trial["runs"]]
        assert sum(run["native_calls"] for run in actual_runs) == counts["measured"]
    except (AssertionError, KeyError, TypeError, ValueError, StopIteration) as error:
        errors.append(str(error) or "diagnostic invariant failed")
    return errors


def initial_report():
    return {"policy": POLICY, "point": CASE, "status": "NOT_RUN", "completed": False,
            "errors": [], "trials": [{"order": order, "runs": [{"shape": shape, "status": "NOT_RUN"} for shape in order]} for order in ORDERS],
            "limitations": ["Post-hoc and single-point; does not replace initial66-config evidence.",
                            "Global BM25 relevance policy, production integration and latency SLO remain unaccepted.",
                            "Same Python/bridge cost boundary and synthetic SQL authority scope as original FTS probe."]}


def save(report, output):
    temporary = output / "report.json.tmp"
    temporary.write_text(json.dumps(report, indent=2) + "\n")
    temporary.replace(output / "report.json")


def execute(binary, output):
    output.mkdir(parents=True, exist_ok=False)
    report = initial_report()
    report["provenance"] = fts.provenance(binary)
    report["provenance"]["join_diagnostic_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    save(report, output)
    started = time.monotonic()
    deadline = started + SECONDS
    active = [None]
    guard = DiagnosticDeadline(SECONDS, active)
    guard.__enter__()
    try:
        with tempfile.TemporaryDirectory(prefix="origence-t0-join-diagnostic-") as temporary:
            directory = Path(temporary)
            try:
                data = fts.fixture(10000)
                report["fixture_sha256"] = fts.digest(data)
                sqls = statements(data)
                report["sql"] = {shape: sql for shape, (sql, _params) in sqls.items()}
                path = directory / "context.db"
                setup_start = time.perf_counter()
                active[0] = CountedBridge(binary, path)
                fts.require_time(deadline, SECONDS)
                fts.populate(active[0], data)
                fts.require_time(deadline, SECONDS)
                fixture_requests = active[0].request_count
                active[0].close()
                active[0] = None
                report["fixture_setup_ms"] = (time.perf_counter() - setup_start) * 1000
                active[0] = CountedBridge(binary, path, create=False)
                fts.require_time(deadline, SECONDS)
                meter = fts.Meter(active[0], deadline)
                assert meter.call("connection_setup", "SELECT 1") == [[1]]
                report["connection_setup"] = {"calls": meter.calls, "position": "before measured paths; no FTS query"}
                report["measured_started_ms"] = (time.monotonic() - started) * 1000
                for trial in report["trials"]:
                    for item in trial["runs"]:
                        run = fts.measured(active[0], data, CASE, "candidate", deadline, candidate_statement=sqls[item["shape"]])
                        item["measurement"] = run
                        item["status"] = "NOT_RUN" if run["outcome"] == "OK" else "FAIL"
                        save(report, output)
                        assert run["outcome"] == "OK", run.get("error", "candidate execution failed")
                report["measured_finished_ms"] = (time.monotonic() - started) * 1000
                report["audit_started_ms"] = (time.monotonic() - started) * 1000
                report["audit"] = {}
                audit(active[0], data, sqls, deadline, report["audit"])
                report["audit_finished_ms"] = (time.monotonic() - started) * 1000
                check_audit(report["audit"], data)
                expected = fts.reference_hits(data, report["audit"])[("dense", 100)]
                for trial in report["trials"]:
                    for item in trial["runs"]:
                        run = item["measurement"]
                        fts.assert_hits(run["hits"], expected)
                        run["rank_status"], run["recall"] = "PASS", 1.0
                        fts.check_measurement(run, CASE, report["audit"], data, expected)
                        item["status"] = "PASS"
                runs = [item["measurement"] for trial in report["trials"] for item in trial["runs"]]
                report["request_counts"] = {"fixture_schema_index": fixture_requests,
                    "measured": sum(run["native_calls"] for run in runs),
                    "measured_queries": sum(run["native_queries"] for run in runs),
                    "measured_transaction_executes": sum(run["native_executes"] for run in runs),
                    "post_measurement_audit": len(report["audit"]["calls"]), "phase_connection_setup": 1,
                    "measurement_process_total": active[0].request_count,
                    "all_bridge_requests": fixture_requests + active[0].request_count,
                    "implicit_connection_pragmas": 2}
                active[0].close()
                active[0] = None
                report["completed"], report["status"] = True, "PASS"
            finally:
                if active[0] is not None:
                    active[0].kill()
                    active[0] = None
                path = directory / "bridge.stderr"
                if path.exists():
                    with path.open("rb") as stream:
                        retained = stream.read(1024 * 1024)
                    (output / "bridge.stderr").write_bytes(retained)
                    report["stderr_bytes"] = {"source": path.stat().st_size, "retained": len(retained)}
    except Exception as error:
        report["status"] = "FAIL"
        report["errors"].append(str(error) or type(error).__name__)
    finally:
        guard.__exit__()
        report["driver_wall_seconds"] = time.monotonic() - started
        save(report, output)
    errors = report_errors(report)
    (output / "verification.json").write_text(json.dumps({"accepted": not errors, "errors": errors}, indent=2) + "\n")
    print(fts.encoded({"status": report["status"], "posthoc": True, "accepted": not errors, "errors": errors}), flush=True)
    return int(bool(errors))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if args.report:
        assert args.binary is None and args.output is None
        errors = report_errors(json.loads(args.report.read_text()))
        print(fts.encoded({"accepted": not errors, "errors": errors}))
        return int(bool(errors))
    if args.binary is None:
        assert args.output is None
        print(json.dumps({"policy": POLICY, "native_status": "NOT_RUN", "measured_paths": 4}, indent=2))
        return 0
    assert args.output is not None
    return execute(args.binary.resolve(strict=True), args.output)


if __name__ == "__main__":
    raise SystemExit(main())
