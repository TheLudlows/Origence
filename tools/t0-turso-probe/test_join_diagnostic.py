"""Harness tests for one post-hoc point, with no native performance claims."""
import copy
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import time
import unittest

from test_fts_cost import TimedSQLiteFixture, fake_audit, fake_run

SPEC = importlib.util.spec_from_file_location("t0_join_diagnostic", Path(__file__).with_name("join_diagnostic.py"))
diag = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(diag)


def fake_report():
    data = diag.fts.fixture(10000)
    report = diag.initial_report()
    observed = fake_audit(data, "all")
    observed["position"] = diag.POLICY["audit_order"]
    observed["eligible_metadata"] = observed["eligible_metadata"]["dense"]
    observed["plans"] = {
        "direct": [[1, 0, 0, "UNIT TEST STUB: QUERY INDEX METHOD fts"]],
        diag.SHAPES[0]: [[1, 0, 0, "QUERY INDEX METHOD fts"], [2, 0, 0, "SEARCH c USING INDEX"], [3, 0, 0, "SCAN scored AS s"]],
        diag.SHAPES[1]: [[1, 0, 0, "QUERY INDEX METHOD fts"], [2, 0, 0, "SCAN scored AS s"], [3, 0, 0, "SEARCH c USING INDEX"]],
    }
    observed["calls"] = [{"label": label, "ok": True, "engine_elapsed_ms": 0.1} for label in ["audit_direct_scores", "audit_dense_eligibility", "audit_physical_index", "plan_direct", "plan_" + diag.SHAPES[0], "plan_" + diag.SHAPES[1]]]
    reference = diag.fts.reference_hits(data, observed)[("dense", 100)]
    report.update({"fixture_sha256": diag.fts.digest(data), "completed": True, "status": "PASS",
                   "driver_wall_seconds": 10.0, "audit": observed,
                   "measured_started_ms": 1, "measured_finished_ms": 2, "audit_started_ms": 3, "audit_finished_ms": 4,
                   "sql": {shape: sql for shape, (sql, _params) in diag.statements(data).items()},
                   "connection_setup": {"calls": [{"label": "connection_setup", "rows": 1}]},
                   "request_counts": {"fixture_schema_index": 192, "measured": 32, "measured_queries": 16,
                                      "measured_transaction_executes": 16, "post_measurement_audit": 6,
                                      "phase_connection_setup": 1, "implicit_connection_pragmas": 2,
                                      "measurement_process_total": 39, "all_bridge_requests": 231}})
    report["provenance"] = diag.fts.provenance()
    report["provenance"].update({"binary_sha256": "0" * 64, "join_diagnostic_sha256": "1" * 64})
    for trial in report["trials"]:
        for item in trial["runs"]:
            item.update({"status": "PASS", "measurement": fake_run(data, diag.CASE, observed, "candidate", reference)})
    return report


class JoinDiagnosticTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.valid = fake_report()

    def assert_mutation_rejected(self, change):
        report = copy.deepcopy(self.valid)
        change(report)
        self.assertTrue(diag.report_errors(report))

    def test_exact_single_point_no_ladder(self):
        self.assertEqual(diag.CASE, {"case": "posthoc-n10000-all-dense-k100", "corpus_rows": 10000, "query": "all", "bucket": "dense", "k": 100, "mode": "unbudgeted"})
        self.assertEqual(len(diag.ORDERS), 2)
        self.assertEqual(diag.ORDERS[0], list(reversed(diag.ORDERS[1])))
        self.assertEqual(len(diag.fts.planned_cases()), 66)
        self.assertTrue(diag.POLICY["initial_matrix_is_not_replaced"])

    def test_shape_preserves_materialization_scope_and_score_bind(self):
        data = diag.fts.fixture(10000)
        sqls = diag.statements(data)
        self.assertEqual(sqls[diag.SHAPES[0]], diag.fts.candidate_sql(data, "dense", 100))
        forced, params = sqls[diag.SHAPES[1]]
        inner, outer = forced.split(") SELECT ", 1)
        self.assertIn("AS MATERIALIZED", inner)
        self.assertEqual(inner.count("?1"), 2)
        self.assertNotIn("LIMIT", inner)
        self.assertIn("FROM scored s CROSS JOIN oc_chunks c ON s.tenant_id=c.tenant_id AND s.workspace_id=c.workspace_id AND s.id=c.id", outer)
        self.assertEqual(outer.count(" CROSS JOIN "), 4)
        self.assertIn(diag.fts.SUPPORT_SQL, outer)
        self.assertTrue(outer.endswith("ORDER BY s.score DESC,c.tenant_id,c.workspace_id,c.id LIMIT 100"))
        self.assertEqual(params, [diag.fts.T, diag.fts.W])

    def test_optional_injection_default_and_cross_match_in_sqlite_fixture(self):
        data = diag.fts.fixture(1000)
        sqls = diag.statements(data)
        with tempfile.TemporaryDirectory() as tmp:
            db = TimedSQLiteFixture(Path(tmp) / "fixture.db", data)
            baseline = diag.fts.measured(db, data, diag.CASE, "candidate", time.monotonic() + 20)
            current = diag.fts.measured(db, data, diag.CASE, "candidate", time.monotonic() + 20, candidate_statement=sqls[diag.SHAPES[0]])
            forced = diag.fts.measured(db, data, diag.CASE, "candidate", time.monotonic() + 20, candidate_statement=sqls[diag.SHAPES[1]])
            for result in [baseline, current, forced]:
                self.assertEqual(result["outcome"], "OK", result)
                self.assertEqual(result["native_calls"], 8)
            self.assertEqual(baseline["hits"], current["hits"])
            self.assertEqual(current["hits"], forced["hits"])
            db.close()

    def test_complete_diagnostic_report(self):
        self.assertEqual(diag.report_errors(self.valid), [])
        self.assertTrue(diag.report_errors(diag.initial_report()))

    def test_missing_reordered_paths_and_changed_scope_rejected(self):
        self.assert_mutation_rejected(lambda report: report["trials"].pop())
        self.assert_mutation_rejected(lambda report: report["trials"][0]["runs"].pop())
        self.assert_mutation_rejected(lambda report: report["trials"][1].__setitem__("order", diag.ORDERS[0]))
        self.assert_mutation_rejected(lambda report: report.__setitem__("point", dict(diag.CASE, k=10)))
        self.assert_mutation_rejected(lambda report: report["trials"][0]["runs"][0].__setitem__("status", "NOT_RUN"))

    def test_bad_rank_score_authority_metadata_rejected(self):
        def mutate_hit(report, column, value):
            report["trials"][0]["runs"][1]["measurement"]["hits"][0][column] = value
        for column, value in [(0, diag.fts.compat.blob(999)), (4, 2), (5, diag.fts.compat.blob(999)), (6, 0.0)]:
            with self.subTest(column=column):
                self.assert_mutation_rejected(lambda report: mutate_hit(report, column, value))
        self.assert_mutation_rejected(lambda report: report["trials"][0]["runs"][1]["measurement"].__setitem__("final_verified_metadata", []))

    def test_cost_count_plan_and_deadline_mutants_rejected(self):
        self.assert_mutation_rejected(lambda report: report.__setitem__("driver_wall_seconds", 181))
        self.assert_mutation_rejected(lambda report: report.__setitem__("audit_started_ms", 0))
        self.assert_mutation_rejected(lambda report: report["request_counts"].__setitem__("measured", 31))
        self.assert_mutation_rejected(lambda report: report["sql"].__setitem__(diag.SHAPES[1], report["sql"][diag.SHAPES[0]]))
        self.assert_mutation_rejected(lambda report: report["audit"]["plans"].__setitem__(diag.SHAPES[1], report["audit"]["plans"][diag.SHAPES[0]]))
        self.assert_mutation_rejected(lambda report: report["audit"]["plans"].__setitem__("direct", [["SCAN oc_chunks"]]))

    def test_failed_report_cannot_be_accepted_as_negative_performance_result(self):
        self.assert_mutation_rejected(lambda report: report.__setitem__("status", "FAIL"))
        self.assert_mutation_rejected(lambda report: report["errors"].append("deadline"))
        self.assert_mutation_rejected(lambda report: report["trials"][0]["runs"][0]["measurement"].__setitem__("outcome", "RESOURCE_EXHAUSTED"))


    def test_diagnostic_deadline_message_uses_180_not_default_900(self):
        class Expired:
            deadline_seconds = 180
            def raw(self, *args, **kwargs):
                raise AssertionError("expired deadline must not issue a request")
        result = diag.fts.measured(Expired(), diag.fts.fixture(1000), diag.CASE, "candidate", time.monotonic() - 1)
        self.assertEqual(result["outcome"], "FAIL")
        self.assertIn("180-second", result["error"])
        self.assertNotIn("900-second", result["error"])


if __name__ == "__main__":
    unittest.main()
