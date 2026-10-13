"""Harness/mutation tests only. Fabricated scores/plans never prove native FTS."""
import copy
import importlib.util
import math
from pathlib import Path
import tempfile
import time
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("t0_fts_cost", Path(__file__).with_name("fts_cost.py"))
fts = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fts)


def fake_audit(data, query):
    """Arbitrary positive unit-test values, explicitly not BM25 evidence."""
    scores = [row["meta"][:3] + [1.0 + row["ordinal"] % 7 / 10]
              for row in data["rows"] if fts.QUERIES[query] in row["search_terms"].split()]
    plan = [[0, 0, 0, "UNIT TEST STUB: QUERY INDEX METHOD fts"]]
    return {"corpus_rows": data["size"], "query": query, "scores": scores,
            "matching_rows": len(scores), "actual_match_density": len(scores) / data["size"],
            "eligible_metadata": {bucket: [row["meta"] for row in fts.bucket_rows(data, bucket)] for bucket in fts.BUCKETS},
            "plans": {name: plan for name in ["direct", "guard"] + [f"{bucket}-k{k}" for bucket in fts.BUCKETS for k in fts.KS]},
            "physical_index": [["t0_cost_chunks_fts", "CREATE INDEX t0_cost_chunks_fts ON oc_chunks USING fts(search_terms)"]],
            "position": "after all measured pairs in this phase", "wall_ms": 1.0, "calls": []}


def fake_run(data, case, audit, arm, reference=None):
    guarded = arm == "candidate" and case["mode"] == "guard_512"
    rejected = guarded and audit["matching_rows"] > fts.BUDGET
    expected = [] if rejected else (reference if reference is not None else fts.oracle_hits(data, audit, case["bucket"], case["k"]))
    calls = []
    for label in fts.expected_labels(arm, rejected, guarded):
        op = "execute" if label.startswith(("begin_", "commit_")) else "query"
        rows = {"authority": 1, "fresh_authority": 1,
                "guard": min(audit["matching_rows"], fts.BUDGET + 1),
                "candidate_cte": len(expected), "fresh_visibility": len(expected),
                "eligibility": len(fts.bucket_rows(data, case["bucket"])),
                "direct_full_index_scores": audit["matching_rows"]}.get(label, 0)
        calls.append({"label": label, "op": op, "request_bytes": 100, "response_json_bytes": 100,
                      "rows": rows, "wall_ms": 0.1, "engine_elapsed_ms": 0.05, "ok": True})
    run = {"arm": arm, "outcome": "RESOURCE_EXHAUSTED" if rejected else "OK",
           "rank_status": "NOT_RUN" if rejected else "PASS", "recall": None if rejected or not expected else 1.0,
           "hits": copy.deepcopy(expected), "final_verified_metadata": [row[:6] for row in expected],
           "calls": calls, "wall_ms": 2.0, "native_calls": len(calls),
           "native_queries": sum(call["op"] == "query" for call in calls),
           "native_executes": sum(call["op"] == "execute" for call in calls),
           "returned_sql_rows": sum(call["rows"] for call in calls),
           "native_call_wall_sum_ms": sum(call["wall_ms"] for call in calls)}
    if guarded:
        run["guard_rows"] = min(audit["matching_rows"], fts.BUDGET + 1)
    if arm == "exact":
        run["enumerated_eligible_rows"] = len(fts.bucket_rows(data, case["bucket"]))
        run["full_index_score_rows"] = audit["matching_rows"]
        run["python_selection_ms"] = 0.1
    return run


def fake_report():
    report = fts.new_report()
    report["completed"] = True
    report["driver_wall_seconds"] = 10.0
    report["provenance"] = fts.provenance()
    report["provenance"]["binary_sha256"] = "0" * 64
    fixtures = {n: fts.fixture(n) for n in fts.CORPORA}
    report["corpora"] = [{"rows": n, "status": "PASS", "fixture_sha256": fts.digest(data),
                           "original_schema_files": 6, "original_schema_statements": 47,
                           "eligible_rows": n * 9 // 10} for n, data in fixtures.items()]
    for phase in report["phases"]:
        data = fixtures[phase["corpus_rows"]]
        phase.update({"status": "PASS", "audit": fake_audit(data, phase["query"]),
                      "measured_started_ms": 1, "measured_finished_ms": 2,
                      "audit_started_ms": 3, "audit_finished_ms": 4,
                      "connection_setup": {"calls": [{"label": "connection_setup", "ok": True, "rows": 1}],
                                           "bridge_requests": 1, "implicit_foreign_keys_pragma_calls": 1,
                                           "position": "before measured pairs; no FTS query"}})
    audits = {(phase["corpus_rows"], phase["query"]): phase["audit"] for phase in report["phases"]}
    references = {key: fts.reference_hits(fixtures[key[0]], audit) for key, audit in audits.items()}
    for case in report["cases"]:
        data = fixtures[case["corpus_rows"]]
        audit = audits[(case["corpus_rows"], case["query"])]
        expected = references[(case["corpus_rows"], case["query"])][(case["bucket"], case["k"])]
        for order in fts.POLICY["orders"]:
            case["trials"].append({"order": order, "runs": [fake_run(data, case, audit, arm, expected) for arm in order]})
        fts.adjudicate_case(case, audit, data, expected)
    report["summary"] = {status: sum(case["status"] == status for case in report["cases"])
                         for status in ("PASS", "FAIL", "NOT_RUN", "RESOURCE_EXHAUSTED")}
    return report


class TimedSQLiteFixture:
    """Actual SQLite authority SQL with fake scalar scores, never native evidence."""
    def __init__(self, path, data):
        self.db = fts.compat.SQLite(path)
        fts.populate(self.db, data, fts=False)
        conn = self.db.connections["default"]
        conn.create_function("fts_match", 2, lambda text, query: int(query.strip('"') in text.split()))
        conn.create_function("fts_score", 2, lambda text, query: float(text.count(query.strip('"')) + 1))

    def raw(self, *args, **kwargs):
        response = self.db.raw(*args, **kwargs)
        response["engine_elapsed_ms"] = 0.0
        return response

    def close(self):
        self.db.close()


class FtsCostTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.valid = fake_report()

    def mutate_rejected(self, change):
        report = copy.deepcopy(self.valid)
        change(report)
        self.assertTrue(fts.report_errors(report))

    def test_frozen_matrix_and_exact_cardinalities(self):
        cases = fts.planned_cases()
        self.assertEqual(len(cases), 66)
        self.assertEqual(sum(case["mode"] == "guard_512" for case in cases), 6)
        self.assertTrue(all(case["bucket"] == "dense" and case["k"] == 100
                            for case in cases if case["mode"] == "guard_512"))
        for size in fts.CORPORA:
            data = fts.fixture(size)
            self.assertEqual([len(fts.bucket_rows(data, bucket)) for bucket in fts.BUCKETS], [1, 8, 64, 512, size * 9 // 10])
            eligible_ids = {row["meta"][2]["blob"] for row in data["rows"] if row["state"] == "eligible"}
            wrong_scope = [row for row in data["rows"] if row["state"] in {"wrong_tenant", "wrong_workspace"}]
            self.assertTrue(any(row["meta"][2]["blob"] in eligible_ids for row in wrong_scope))
        with self.assertRaises(AssertionError):
            fts.fixture(100000)

    def test_original_ddl_and_visibility_fixture(self):
        for size in fts.CORPORA:
            data = fts.fixture(size)
            with tempfile.TemporaryDirectory() as tmp:
                db = fts.compat.SQLite(Path(tmp) / "fixture.db")
                fts.populate(db, data, fts=False)
                self.assertEqual(db.query(fts.AUTH_SQL, [fts.TOKEN_HASH]), [[fts.T, fts.W, fts.KEY, "reader"]])
                for bucket in fts.BUCKETS:
                    sql, params = fts.visibility_sql(source=fts.bucket_source(data, bucket))
                    actual = db.query("SELECT " + fts.META_COLUMNS + " " + sql, params)
                    self.assertEqual(sorted(map(fts.metadata_key, actual)), sorted(fts.metadata_key(row["meta"]) for row in fts.bucket_rows(data, bucket)))
                db.close()

    def test_sql_shape_reuses_bind_and_no_inner_topk(self):
        sql, params = fts.candidate_sql(fts.fixture(1000), "source_512", 100)
        inner, outer = sql.split(") SELECT ", 1)
        self.assertIn("AS MATERIALIZED", inner)
        self.assertEqual(inner.count("?1"), 2)
        self.assertNotIn("LIMIT", inner)
        self.assertIn(fts.SUPPORT_SQL, outer)
        self.assertIn("v.source_event_id=?4", outer)
        self.assertTrue(outer.endswith("LIMIT 100"))
        self.assertEqual(len(params), 3)
        self.assertTrue(fts.GUARD_SQL.endswith("LIMIT 513"))

    def test_actual_measured_calls_and_guard_rejection(self):
        data = fts.fixture(1000)
        with tempfile.TemporaryDirectory() as tmp:
            db = TimedSQLiteFixture(Path(tmp) / "fixture.db", data)
            case = next(case for case in fts.planned_cases() if case["corpus_rows"] == 1000 and case["query"] == "all" and case["bucket"] == "source_512" and case["k"] == 100)
            exact = fts.measured(db, data, case, "exact", time.monotonic() + 20)
            candidate = fts.measured(db, data, case, "candidate", time.monotonic() + 20)
            self.assertEqual(exact["outcome"], "OK", exact)
            self.assertEqual(candidate["outcome"], "OK", candidate)
            self.assertEqual(exact["native_calls"], 9)
            self.assertEqual(candidate["native_calls"], 8)
            fts.assert_hits(candidate["hits"], exact["hits"])
            guarded = dict(case, mode="guard_512")
            rejected = fts.measured(db, data, guarded, "candidate", time.monotonic() + 20)
            self.assertEqual(rejected["outcome"], "RESOURCE_EXHAUSTED")
            self.assertEqual(rejected["native_calls"], 4)
            self.assertEqual(rejected["guard_rows"], 513)
            self.assertEqual(rejected["rank_status"], "NOT_RUN")
            self.assertIsNone(rejected["recall"])
            self.assertFalse(rejected["hits"])
            db.close()

    def test_valid_unit_report_and_native_call_totals(self):
        self.assertEqual(fts.report_errors(self.valid), [])
        self.assertEqual(self.valid["summary"], {"PASS": 63, "FAIL": 0, "NOT_RUN": 0, "RESOURCE_EXHAUSTED": 3})
        runs = [run for case in self.valid["cases"] for trial in case["trials"] for run in trial["runs"]]
        self.assertEqual(len(runs), 264)
        self.assertEqual(sum(run["native_calls"] for run in runs), 2226)
        self.assertEqual(sum(run["native_queries"] for run in runs), 1182)

    def test_rank_and_score_mutants_rejected(self):
        def rank(report):
            case = next(case for case in report["cases"] if len(case["trials"][0]["runs"][0]["hits"]) > 1)
            hits = case["trials"][0]["runs"][0]["hits"]
            hits[0], hits[1] = hits[1], hits[0]
        self.mutate_rejected(rank)
        for score in [0.0, -1.0, float("nan"), float("inf"), 999.0]:
            with self.subTest(score=score):
                self.mutate_rejected(lambda report, score=score: report["cases"][0]["trials"][0]["runs"][0]["hits"][0].__setitem__(6, score))

    def test_full_scope_metadata_and_fresh_verification_mutants_rejected(self):
        for column in [0, 1, 3, 4, 5]:
            with self.subTest(column=column):
                self.mutate_rejected(lambda report, column=column: report["cases"][0]["trials"][0]["runs"][0]["hits"][0].__setitem__(column, 2 if column == 4 else fts.compat.blob(999999)))
        self.mutate_rejected(lambda report: report["cases"][0]["trials"][0]["runs"][0].__setitem__("final_verified_metadata", []))

    def test_missing_case_phase_trial_and_unexecuted_rejected(self):
        self.mutate_rejected(lambda report: report["cases"].pop())
        self.mutate_rejected(lambda report: report["phases"].pop())
        self.mutate_rejected(lambda report: report["cases"][0]["trials"].pop())
        self.mutate_rejected(lambda report: report["cases"][0].__setitem__("status", "NOT_RUN"))
        self.mutate_rejected(lambda report: report["cases"][0]["trials"][0].__setitem__("order", ["exact", "candidate"]))
        self.mutate_rejected(lambda report: report["phases"][0].__setitem__("audit_started_ms", 0))
        self.assertTrue(fts.report_errors(fts.new_report()))
        self.mutate_rejected(lambda report: report.__setitem__("driver_wall_seconds", 901))
        self.mutate_rejected(lambda report: report["provenance"].pop("binary_sha256"))
        self.mutate_rejected(lambda report: report["corpora"].pop())

    def test_budget_rejection_cannot_be_empty_success_or_recall_pass(self):
        def change(report, field, value, case_level=False):
            case = next(case for case in report["cases"] if case["status"] == "RESOURCE_EXHAUSTED")
            target = case if case_level else case["trials"][0]["runs"][0]
            target[field] = value
        for field, value, case_level in [("status", "PASS", True), ("ranking", "PASS", True),
                                          ("outcome", "OK", False), ("rank_status", "PASS", False),
                                          ("recall", 1.0, False), ("guard_rows", 512, False)]:
            with self.subTest(field=field):
                self.mutate_rejected(lambda report: change(report, field, value, case_level))

    def test_query_count_timing_and_plan_mutants_rejected(self):
        for field in ["native_calls", "native_queries", "native_executes", "returned_sql_rows"]:
            with self.subTest(field=field):
                self.mutate_rejected(lambda report, field=field: report["cases"][0]["trials"][0]["runs"][0].__setitem__(field, 0))
        self.mutate_rejected(lambda report: report["cases"][0]["trials"][0]["runs"][0]["calls"].pop())
        self.mutate_rejected(lambda report: report["cases"][0]["trials"][0]["runs"][0]["calls"][0].pop("engine_elapsed_ms"))
        self.mutate_rejected(lambda report: report["cases"][0]["trials"][0]["runs"][0].__setitem__("wall_ms", 0.0))
        self.mutate_rejected(lambda report: report["phases"][0]["audit"]["plans"].__setitem__("direct", [["SCAN oc_chunks"]]))

    def test_bridge_without_native_timing_fails_closed(self):
        class MissingTiming:
            def raw(self, *args, **kwargs):
                return {"ok": True, "result": {"rows": []}}
        meter = fts.Meter(MissingTiming(), time.monotonic() + 1)
        with self.assertRaises(KeyError):
            meter.call("authority", "SELECT 1")
        self.assertFalse(meter.calls[0]["ok"])

    def test_driver_deadline_is_failure_before_next_query(self):
        class Uncalled:
            def raw(self, *args, **kwargs):
                raise AssertionError("deadline must prevent request")
        result = fts.measured(Uncalled(), fts.fixture(1000), fts.planned_cases()[0], "candidate", time.monotonic() - 1)
        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(result["native_calls"], 0)
        self.assertIn("deadline", result["error"])


    def test_signal_deadline_interrupts_startup_before_populate(self):
        active = [None]
        populated = []
        with self.assertRaises(TimeoutError), fts.OverallDeadline(0.02, active):
            time.sleep(0.2)  # Simulate Bridge constructor waiting for ready.
            populated.append(True)
        self.assertEqual(populated, [])

    def test_signal_deadline_kills_active_setup(self):
        class Active:
            killed = False
            def kill(self):
                self.killed = True
        db = Active()
        with self.assertRaises(TimeoutError), fts.OverallDeadline(0.02, [db]):
            time.sleep(0.2)  # Simulate populate blocked on a request.
        self.assertTrue(db.killed)

    def test_guard_exact_512_513_boundary(self):
        data = fts.fixture(1000)
        case = next(case for case in fts.planned_cases() if case["corpus_rows"] == 1000 and case["query"] == "all" and case["mode"] == "guard_512")
        with tempfile.TemporaryDirectory() as tmp:
            db = TimedSQLiteFixture(Path(tmp) / "fixture.db", data)
            original = db.raw
            for count in [512, 513]:
                def raw(sql, params=(), **kwargs):
                    if sql == fts.GUARD_SQL:
                        return {"ok": True, "engine_elapsed_ms": 0.0,
                                "result": {"rows": [[fts.T, fts.W, fts.compat.blob(i)] for i in range(count)]}}
                    return original(sql, params, **kwargs)
                with mock.patch.object(db, "raw", raw):
                    result = fts.measured(db, data, case, "candidate", time.monotonic() + 20)
                self.assertEqual(result["outcome"], "OK" if count == 512 else "RESOURCE_EXHAUSTED")
                self.assertEqual(result["native_calls"], 9 if count == 512 else 4)
            db.close()


    def test_failed_audit_retains_returned_score_evidence(self):
        class BadScores:
            def raw(self, *args, **kwargs):
                return {"ok": True, "engine_elapsed_ms": 0.0,
                        "result": {"rows": [[fts.T, fts.W, fts.compat.blob(50000), 0.0]]}}
        result = {}
        with self.assertRaises(AssertionError):
            fts.collect_audit(BadScores(), fts.fixture(1000), "all", time.monotonic() + 2, result)
        self.assertEqual(result["scores"][0][3], 0.0)
        self.assertEqual(len(result["calls"]), 1)


if __name__ == "__main__":
    unittest.main()
