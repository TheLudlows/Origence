"""Harness correctness only. SQLite functions below are NOT native FTS evidence."""
import copy
import importlib.util
import contextlib
import io
import json
from pathlib import Path
import tempfile
import time
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("staged_fts", Path(__file__).with_name("staged_fts.py"))
s = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(s)


class TimedSQLite:
    def __init__(self, path, data, equal_scores=False):
        self.db = s.fts.compat.SQLite(path)
        s.fts.populate(self.db, data, fts=False)
        self.equal_scores = equal_scores

    def raw(self, sql, params=(), connection="default", **kwargs):
        # Ensure each newly named test connection has the fake scalar functions.
        self.db.raw("SELECT 1", connection=connection)
        conn = self.db.connections[connection]
        conn.create_function("fts_match", 2, lambda text, query: int(query.strip('"') in text.split()))
        conn.create_function("fts_score", 2, lambda text, query: 1.0 if self.equal_scores else 1.0 + text.split().count(query.strip('"')) / max(len(text.split()), 1))
        result = self.db.raw(sql, params, connection=connection, **kwargs)
        result["engine_elapsed_ms"] = 0.0
        return result

    query = s.fts.compat.Bridge.query
    execute = s.fts.compat.Bridge.execute

    def close(self):
        self.db.close()


class DriverSQLite(TimedSQLite):
    """Driver integration stub: fabricated plans/index metadata, NEVER native proof."""
    def __init__(self, _binary, path, create=True):
        self.db = s.fts.compat.SQLite(path)
        self.equal_scores = False
        self.request_count = self.query_count = self.execute_count = 0
        self.connection_names = set()

    def raw(self, sql, params=(), connection="default", **kwargs):
        self.request_count += 1
        self.connection_names.add(connection)
        if kwargs.get("op", "query") == "query":
            self.query_count += 1
        else:
            self.execute_count += 1
        if sql.startswith("CREATE INDEX t0_cost_chunks_fts"):
            sql = "CREATE INDEX t0_cost_chunks_fts ON oc_chunks(search_terms)"
        elif sql.startswith("EXPLAIN QUERY PLAN"):
            return {"ok": True, "engine_elapsed_ms": 0.0, "result": {"rows": [[0, 0, 0, "UNIT STUB QUERY INDEX METHOD fts"]]}}
        elif sql == "SELECT name,sql FROM sqlite_master WHERE name='t0_cost_chunks_fts'":
            return {"ok": True, "engine_elapsed_ms": 0.0, "result": {"rows": [["t0_cost_chunks_fts", "UNIT STUB USING fts"]]}}
        return super().raw(sql, params, connection=connection, **kwargs)

    counts = s.CountedBridge.counts
    kill = TimedSQLite.close


class StagedTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.data = s.fts.fixture(1000)
        self.db = TimedSQLite(Path(self.temp.name) / "fixture.db", self.data)
        self.case = next(c for c in s.planned_cases() if c["corpus_rows"] == 1000 and c["query"] == "all" and c["bucket"] == "dense" and c["k"] == 100)

    def tearDown(self):
        self.db.close()
        self.temp.cleanup()

    def run_case(self, case=None, arm="staged", **kwargs):
        return s.measured(self.db, self.data, case or self.case, arm, time.monotonic() + 60, **kwargs)

    def reference(self, case=None):
        case = case or self.case
        scores = self.db.query(s.fts.SCORE_SQL, [s.fts.compat.literal_fts_query(s.fts.QUERIES[case["query"]])])
        return s.fts.reference_hits(self.data, {"scores": scores})[(case["bucket"], case["k"])]

    def test_frozen_matrix_and_sql_shape(self):
        self.assertEqual(len(s.planned_cases()), 60)
        self.assertEqual(s.POLICY["corpora"], [1000, 10000])
        for take in (1, 100, 256):
            sql = s.page_sql(take)
            self.assertEqual(sql.count("?1"), 2)
            self.assertIn("tenant_id=?2 AND workspace_id=?3", sql)
            self.assertNotIn("JOIN", sql)
            self.assertTrue(sql.endswith(f"LIMIT {take+1} OFFSET ?4"))
        with self.assertRaises(AssertionError):
            s.page_sql(257)
        self.assertIn("summary_union_and_ranking", s.POLICY["not_run"])

    def test_both_arms_all_buckets_against_independent_fixture_oracle(self):
        for bucket in s.fts.BUCKETS:
            for k in (10, 100):
                case = dict(self.case, bucket=bucket, k=k)
                reference = self.reference(case)
                for arm in ("staged", "full_scan"):
                    with self.subTest(bucket=bucket, k=k, arm=arm):
                        run = self.run_case(case, arm)
                        self.assertEqual(run["outcome"], "OK", run)
                        s.check_run(run, reference)
                        self.assertTrue(all("JOIN" not in c["sql"] for c in run["calls"]))
                        self.assertTrue(all(len(c["params"]) <= 514 for c in run["calls"]))

    def test_ties_across_pages_exact_order_and_refill(self):
        self.db.equal_scores = True
        case = dict(self.case, bucket="source_512")
        run = self.run_case(case, page_size=8)
        self.assertEqual(run["outcome"], "OK", run)
        self.assertGreater(len(run["pages"]), 12)
        self.assertGreater(run["examined"], 100)
        s.check_run(run, self.reference(case))
        self.assertEqual(len({s.key(r) for r in run["hits"]}), 100)

    def test_zero_and_fewer_than_k_prove_exhaustion(self):
        # The low-density/source_1 fixture has one eligible row, never padded.
        case = dict(self.case, query="low", bucket="source_1")
        run = self.run_case(case)
        self.assertEqual(run["outcome"], "OK", run)
        self.assertEqual(len(run["hits"]), 1)
        self.assertFalse(run["pages"][-1]["more"])
        self.db.execute("UPDATE oc_chunks SET search_terms='absent'")
        run = self.run_case(case)
        self.assertEqual(run["outcome"], "OK", run)
        self.assertEqual(run["hits"], [])
        self.assertEqual(run["examined"], 0)

    def test_exact_candidate_cap_512_and_513_has_eof_proof(self):
        self.db.equal_scores = True
        ids = self.db.query("SELECT id FROM oc_chunks WHERE tenant_id=? AND workspace_id=? ORDER BY id", [s.fts.T, s.fts.W])
        self.db.execute("UPDATE oc_chunks SET search_terms='absent'")
        for count in (512, 513):
            self.db.execute("UPDATE oc_chunks SET search_terms='absent'")
            for batch in s.chunks(ids[:count]):
                self.db.execute("UPDATE oc_chunks SET search_terms='commontoken' WHERE tenant_id=? AND workspace_id=? AND id IN (" + ",".join("?" for _ in batch) + ")", [s.fts.T, s.fts.W] + [r[0] for r in batch])
            case = dict(self.case, bucket="source_1")  # Fewer than K, must prove EOS.
            run = self.run_case(case, limits={**s.LIMITS, "candidates": 512})
            self.assertEqual(run["outcome"], "OK" if count == 512 else "RESOURCE_EXHAUSTED", run)
            self.assertEqual(run["examined"], 512)
            if count == 513:
                self.assertEqual(run["hits"], [])
                self.assertEqual(run["rank_status"], "NOT_RUN")
                self.assertIsNone(run["recall"])

    def test_row_and_call_caps_explicit_and_rollback(self):
        for limits in ({**s.LIMITS, "authority_rows": 0}, {**s.LIMITS, "native_calls": 3}):
            run = self.run_case(limits=limits)
            self.assertEqual(run["outcome"], "RESOURCE_EXHAUSTED", run)
            self.assertEqual(run["hits"], [])
            self.assertEqual(run["calls"][-1]["label"], "rollback")
            self.assertLessEqual(run["native_calls"], limits["native_calls"])
        self.assertEqual(self.run_case()["outcome"], "OK")

    def test_json_each_compatibility_including_null_object_scalar(self):
        source = s.blobkey(self.data["groups"][0]["source"])
        samples = [({}, []), ({"support_sources": None}, [None]), ({"support_sources": []}, []),
                   ({"support_sources": {}}, []), ({"support_sources": source}, [source]),
                   ({"support_sources": {"member": source}}, [source]),
                   ({"support_sources": [source, True, None, [source]]}, [source, None, None, None])]
        for fields, expected in samples:
            locator = json.dumps({"parser": "memory-semantics:v1", **fields})
            self.assertEqual(s.support_ids(locator), expected)
            values = self.db.query("SELECT value FROM json_each(?, '$.support_sources')", [locator])
            self.assertEqual(len(values), len(expected))
        with self.assertRaises(json.JSONDecodeError):
            s.support_ids("{broken")

    def test_duplicate_json_members_preserve_sqlite_first_path_and_all_values(self):
        active = s.blobkey(self.data["groups"][0]["source"])
        missing = "f" * 32
        values = [
            ('{"parser":"memory-semantics:v1","parser":"other","support_sources":["' + missing + '"]}', [missing]),
            ('{"parser":"other","parser":"memory-semantics:v1","support_sources":["' + missing + '"]}', []),
            ('{"parser":"memory-semantics:v1","support_sources":["' + missing + '"],"support_sources":[]}', [missing]),
            ('{"parser":"memory-semantics:v1","support_sources":{"a":"' + missing + '","a":"' + active + '"}}', [missing, active]),
        ]
        for locator, expected in values:
            self.assertEqual(s.support_ids(locator), expected)
            parser = self.db.query("SELECT json_extract(?, '$.parser')", [locator])[0][0]
            rows = self.db.query("SELECT value FROM json_each(?, '$.support_sources')", [locator]) if parser == "memory-semantics:v1" else []
            self.assertEqual([r[0] for r in rows], expected)

    def test_unmatched_sibling_invalidates_whole_version(self):
        group = self.data["groups"][1]
        sibling = next(r for r in self.data["rows"] if r["group"] == 1 and r["ordinal"] == 7)
        self.db.execute("UPDATE oc_chunks SET search_terms='nonmatching',locator=? WHERE tenant_id=? AND workspace_id=? AND id=?",
                        [json.dumps({"parser": "memory-semantics:v1", "support_sources": ["bad"]})] + sibling["meta"][:3])
        case = dict(self.case, bucket="source_8")
        run = self.run_case(case)
        self.assertEqual(run["outcome"], "OK", run)
        self.assertEqual(run["hits"], [])
        self.assertTrue(any(c["label"] == "s2_supports" for c in run["calls"]))
        self.db.execute("UPDATE oc_chunks SET locator='{broken' WHERE tenant_id=? AND workspace_id=? AND id=?", sibling["meta"][:3])
        run = self.run_case(case)
        self.assertEqual(run["outcome"], "FAIL", run)
        self.assertEqual(run["hits"], [])

    def test_fresh_key_and_root_revocations_return_no_evidence(self):
        def revoke_key(db, _result, _hits):
            db.execute("UPDATE oc_api_keys SET revoked=1 WHERE id=?", [s.fts.KEY], connection="writer")
        run = self.run_case(hooks={"before_verify": revoke_key})
        self.assertEqual(run["outcome"], "AUTHORITY_CHANGED", run)
        self.assertEqual(run["hits"], [])
        self.db.execute("UPDATE oc_api_keys SET revoked=0 WHERE id=?", [s.fts.KEY])
        def revoke_root(db, _result, hits):
            db.execute("UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?", [s.fts.T, s.fts.W, hits[0][5]], connection="writer")
        run = self.run_case(hooks={"before_verify": revoke_root})
        self.assertEqual(run["outcome"], "SNAPSHOT_CHANGED", run)
        self.assertEqual(run["hits"], [])
        self.assertEqual(run["final_verified_metadata"], [])

    def test_two_connection_snapshot_scores_and_order_stable(self):
        expected = self.reference()
        def update_after_page(db, number, _result):
            if number == 1:
                db.execute("UPDATE oc_chunks SET search_terms='commontoken commontoken' WHERE tenant_id=? AND workspace_id=?", [s.fts.T, s.fts.W], connection="writer")
        run = self.run_case(page_size=8, hooks={"after_page": update_after_page})
        self.assertEqual(run["outcome"], "OK", run)
        s.fts.assert_hits(run["hits"], expected)
        self.assertNotEqual(run["hits"], self.reference())

    def test_native_focused_harness_runs_with_sqlite_stub_only(self):
        results = s.focused_cases(self.db, self.data, time.monotonic() + 60)
        self.assertEqual(len(results), 8)
        s.validate_focused(results)
        self.assertTrue(all(r["status"] == "PASS" for r in results))

    def test_focused_report_mutations_rejected(self):
        original = s.focused_cases(self.db, self.data, time.monotonic() + 60)
        s.validate_focused(original)
        changes = [lambda r: r.pop(), lambda r: r[0].pop("run"),
                   lambda r: r[-1]["run"].update(outcome="OK"),
                   lambda r: r[3].update(old_snapshot_rows=[[["bad"]]]),
                   lambda r: r[3].update(new_snapshot_rows=[]),
                   lambda r: r[3].update(writer_calls=[]),
                   lambda r: r[3].update(before_expected=[])]
        for change in changes:
            altered = copy.deepcopy(original)
            change(altered)
            with self.assertRaises((AssertionError, KeyError)):
                s.validate_focused(altered)

    def test_bad_order_score_scope_and_report_rejected(self):
        row = [s.fts.T, s.fts.W, s.fts.compat.blob(1), 1.0]
        for bad in (row[:3] + [0.0], row[:3] + [float("nan")], [s.fts.compat.blob(999)] + row[1:]):
            with self.assertRaises(AssertionError):
                s.validate_page([bad])
        with self.assertRaises(AssertionError):
            s.validate_page([row, row])
        self.assertTrue(s.report_errors(s.new_report()))
        run = self.run_case()
        expected = self.reference()
        s.check_run(run, expected)
        for mutation in (lambda r: r.update(native_calls=0), lambda r: r.update(final_verified_metadata=[]),
                         lambda r: r["hits"].reverse(), lambda r: r["calls"][0].update(sql="SELECT * FROM a JOIN b")):
            changed = copy.deepcopy(run)
            mutation(changed)
            with self.assertRaises(AssertionError):
                s.check_run(changed, expected)

    def test_complete_driver_stub_report_and_mutation_rejection(self):
        saved = []
        def retain(report, _output):
            if not saved:
                saved.append(report)
        with mock.patch.object(s, "CountedBridge", DriverSQLite), mock.patch.object(s, "save", retain), contextlib.redirect_stdout(io.StringIO()):
            code = s.execute(Path(__file__), Path(self.temp.name) / "stub-output")
        self.assertEqual(code, 0, saved[0].get("fatal_errors"))
        report = saved[0]
        self.assertEqual(s.report_errors(report), [])
        mutations = [lambda r: r["phases"][0]["audit"].update(plans={}),
                     lambda r: r["phases"][0].update(query="unplanned"),
                     lambda r: r["phases"][0]["counts"].update(bridge_requests=0),
                     lambda r: r["focused"][3].pop("writer_calls"),
                     lambda r: r["cases"][0]["trials"].pop(),
                     lambda r: r.update(completed=False)]
        for change in mutations:
            modified = copy.deepcopy(report)
            change(modified)
            self.assertTrue(s.report_errors(modified))

    def test_driver_marks_actual_failure_without_promoting_untouched_work(self):
        saved = []
        class BrokenScores(DriverSQLite):
            def raw(self, sql, params=(), **kwargs):
                result = super().raw(sql, params, **kwargs)
                if sql.startswith(s.SCOPED_SCORE):
                    for row in result["result"]["rows"]:
                        row[3] = 0.0
                return result
        with mock.patch.object(s, "CountedBridge", BrokenScores), mock.patch.object(s, "save", lambda report, _output: saved.append(report) if not saved else None), contextlib.redirect_stdout(io.StringIO()):
            code = s.execute(Path(__file__), Path(self.temp.name) / "failed-stub-output")
        self.assertEqual(code, 1)
        self.assertEqual(saved[0]["cases"][0]["status"], "FAIL")
        self.assertEqual(saved[0]["cases"][0]["trials"][0]["runs"][0]["pending_page"]["rows"][0][3], 0.0)
        self.assertEqual(saved[0]["cases"][1]["status"], "NOT_RUN")
        self.assertEqual(saved[0]["phases"][0]["status"], "FAIL")
        self.assertFalse(saved[0]["completed"])

    def test_failed_audit_retains_native_score_rows(self):
        class BadScores:
            def raw(self, *_args, **_kwargs):
                return {"ok": True, "engine_elapsed_ms": 0.0, "result": {"rows": [[s.fts.T, s.fts.W, s.fts.compat.blob(1), 0.0]]}}
        result = {}
        with self.assertRaises(AssertionError):
            s.audit(BadScores(), self.data, "all", time.monotonic() + 10, result)
        self.assertEqual(len(result["calls"]), 1)
        self.assertEqual(result["scores"][0][3], 0.0)

    def test_deadline_is_failure_not_successful_budget_rejection(self):
        run = s.measured(self.db, self.data, self.case, "staged", time.monotonic() - 1)
        self.assertEqual(run["outcome"], "FAIL")
        self.assertEqual(run["native_calls"], 0)
        self.assertEqual(run["hits"], [])


if __name__ == "__main__":
    unittest.main()
