"""Fabricated evidence tests validate the auditor, never stand in for native FTS."""
import contextlib
import copy
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import fts_probe as f


def hit(row, score):
    return {**{k: row[k] for k in f.IDENTITY}, "score": score}


def fresh(inputs, mutation="none"):
    return {"mutation": mutation, "authority_kind": "in_memory_fixture_only", "authority_native_calls": 0,
            "batch_size": f.BATCH, "fixture_batch_calls": (len(inputs)+f.BATCH-1)//f.BATCH,
            "checked_rows": len(inputs), "elapsed_ms": .01,
            "status": "OK" if mutation == "none" else "AUTHORITY_REVOKED" if mutation == "revoked_key" else "SNAPSHOT_CHANGED",
            "hits": copy.deepcopy(inputs) if mutation == "none" else []}


def case(phase, name):
    terms, k = f.EXPECTED[phase, name]
    rows = f.fixture(phase)
    scores = f.reference_bm25(rows, terms)
    oracle = sorted([hit(r, scores[f.identity(r)]) for r in rows if f.scoped(r) and f.identity(r) in scores], key=f.rank_key)
    eligible = {f.identity(r) for r in rows if f.scoped(r) and r["eligible"]}
    prefiltered = name == "authority_prefilter"
    raw = [h for h in oracle if not prefiltered or f.identity(h) in eligible][:k]
    checked = [h for h in raw if f.identity(h) in eligible]
    filtered = f.valid_membership(checked, [h for h in oracle if f.identity(h) in eligible], k)
    return {"kind": "case", "phase": phase, "name": name, "terms": terms, "k": k,
            "scope_filter": f.authority_filter(rows) if prefiltered else f.SCOPE,
            "prefilter_mode": "fixture_full_identity" if prefiltered else "scope_only",
            "enumerated_authority_rows": len(rows) if prefiltered else 0,
            "prefilter": True, "fast_search": False, "operator": "AND", "fuzziness": 0, "wand_factor": 1.,
            "candidate_raw": copy.deepcopy(raw), "oracle_raw": copy.deepcopy(oracle), "oracle_only": True,
            "oracle_complete": True, "expected_scoped_matches": len(oracle), "checked_candidates": copy.deepcopy(checked),
            "fresh_check": fresh(checked), "native_bm25_topk": True, "native_score_descending": True,
            "native_strict_topk": True, "native_filtered_topk": filtered,
            "candidate_status": "EXPECTED_NEGATIVE_CONTROL" if name == "deep_authority" and not filtered else "PASS_BOUNDED_FIXTURE" if filtered else "BLOCKER", "plan": "LanceRead\n  MatchQuery: column=text",
            "calls": {"candidate": 1, "oracle": 1, "plan": 1, "authority_native": 0,
                      "authority_fixture_batches": (len(raw)+f.BATCH-1)//f.BATCH},
            "timing_ms": {"eligibility_fixture_enumeration": .01, "candidate_native": 1., "candidate_authority_fixture": .01,
                          "candidate_total": 2., "oracle_native": 1., "plan": .1}}


def evidence():
    records = [{"kind": "header", "schema": f.SCHEMA_V1, "lancedb": "0.40.0", "arrow": "58", "max_rows": f.CAP,
                "deadline_seconds": f.DEADLINE, "tokenizer": f.TOKENIZER, "not_run": list(f.NOT_RUN),
                "authority_kind": "in_memory_fixture_only", "full_enumeration": "diagnostic_oracle_only",
                "serving_strategy": "native_scoped_topk_with_fixture_authority_prefilter_arm"}]
    records.append({"kind": "setup", "native_calls": 3, "index_parameters": {"base_tokenizer": "whitespace", "lower_case": False, "stem": False, "remove_stop_words": False, "ascii_folding": False, "max_token_length": None}})
    records.append({"kind": "reopen", "native_calls": 2, "mode": "new_connection_same_process"})
    for phase in f.PHASES:
        count = len(f.fixture(phase))
        fresh_count = 1 if phase == "fresh_before_refresh" else 0
        records.append({"kind": "snapshot", "phase": phase, "fixture": f.fixture(phase), "native_calls": 4,
                        "indices": [{"name": "t0_chunk_terms", "type": "FTS", "columns": ["text"]}],
                        "stats": {"type": "FTS", "indexed_rows": count-fresh_count, "unindexed_rows": fresh_count},
                        "layout": {"rows": count, "fragments": 1, "bytes": 100}, "table_version": min(f.PHASES.index(phase)+1, 5)})
        for (p, name) in f.EXPECTED:
            if phase == p:
                records.append(case(phase, name))
    records.append({"kind": "empty_terms", "phase": "indexed", "name": "empty", "native_calls": 0, "hits": []})
    rank = case("indexed", "rank")["checked_candidates"]
    for mutation in ("version", "generation", "source", "withdrawn", "revoked_key"):
        records.append({"kind": "fresh_authority", "phase": "indexed", "input_hits": rank, "result": fresh(rank, mutation)})
    scores = f.reference_bm25(f.fixture(), ["quartz"])
    records.append({"kind": "unscoped_control", "terms": ["quartz"], "oracle_only": True, "native_calls": 1,
                    "hits": [hit(r, scores[f.identity(r)]) for r in f.fixture() if f.identity(r) in scores]})
    records.append({"kind": "ordered_api", "native_calls": 1, "status": "UNSUPPORTED", "error": "Column _score not found"})
    for action in ("insert", "update", "delete", "OptimizeAction::Index", "OptimizeAction::Compact"):
        records.append({"kind": "mutation", "action": action, "native_calls": 1, "prune": False})
    records.append({"kind": "terminal", "status": "COMPLETED_NATIVE_MATRIX", "elapsed_ms": 100})
    return records


def application(inputs):
    # Test-data builder only; the explicit tests below independently assert the
    # expected order and duplicate/conflict outcomes against hand-written values.
    prepared = f.prepare_rank_inputs(inputs)
    return {"schema": "origence-t0-pre-rrf-v1", "stage": "after_fresh_authority_before_rrf",
            "input": "fresh_check.hits", "input_count": len(inputs), "output_count": len(prepared),
            "deduplicated_count": len(inputs)-len(prepared), "rank_inputs": copy.deepcopy(prepared),
            "rank_positions": list(range(1, len(prepared)+1)), "native_calls": 0,
            "elapsed_ms": .02, "rrf_executed": False}


def as_v2_case(event):
    e = copy.deepcopy(event)
    e["application_stage"] = application(e["checked_candidates"])
    e["timing_ms"]["application_prepare"] = .02
    if e["name"] != "deep_authority":
        e["candidate_status"] = "PASS_BOUNDED_FIXTURE"
    return e


def evidence_v2():
    records = evidence()
    records[0].update(schema=f.SCHEMA_V2, contract=copy.deepcopy(f.CONTRACT_V2), not_run=sorted(f.NOT_RUN_V2))
    return [as_v2_case(e) if e["kind"] == "case" else e for e in records]


def tied_v2_case():
    e = case("indexed", "ties")
    e["candidate_raw"] = list(reversed(e["oracle_raw"][-10:]))
    e["checked_candidates"] = copy.deepcopy(e["candidate_raw"])
    e["fresh_check"] = fresh(e["checked_candidates"])
    e.update(native_strict_topk=False, native_filtered_topk=True, candidate_status="BLOCKER")
    return as_v2_case(e)


class AuditorTests(unittest.TestCase):
    def test_valid_fabricated_evidence_retains_blocker(self):
        report = f.audit(evidence())
        self.assertEqual(report["errors"], [])
        self.assertEqual(report["status"], "BLOCKER")
        self.assertFalse(report["production_acceptance"])
        self.assertFalse(report["oracle_is_serving_strategy"])

    def test_initial_reference_tf_length_downranking(self):
        scores = f.reference_bm25(f.fixture(), ["quartz"])
        rows = {r["id"]: r for r in f.fixture() if f.scoped(r)}
        self.assertGreater(scores[f.identity(rows["rank-strong"])], scores[f.identity(rows["rank-weak"])])

    def test_raw_order_before_any_sort(self):
        e = case("indexed", "statistics_rank")
        e["oracle_raw"].reverse()
        self.assertTrue(any("raw native score" in x for x in f.audit_case(e, f.fixture())["errors"]))

    def test_downranked_topk_not_accepted(self):
        e = case("indexed", "rank")
        e["candidate_raw"] = e["oracle_raw"][-1:]
        self.assertTrue(any("invalid BM25 TopK" in x for x in f.audit_case(e, f.fixture())["errors"]))

    def test_tie_equivalent_topk_is_not_bm25_error(self):
        e = case("indexed", "ties")
        e["candidate_raw"] = list(reversed(e["oracle_raw"][-10:]))
        e["checked_candidates"] = copy.deepcopy(e["candidate_raw"])
        e["fresh_check"] = fresh(e["checked_candidates"])
        e["native_strict_topk"] = False
        e["native_filtered_topk"] = True
        e["candidate_status"] = "BLOCKER"
        report = f.audit_case(e, f.fixture())
        self.assertEqual(report["errors"], [])
        self.assertTrue(report["bm25_valid_topk"])
        self.assertFalse(report["strict_id_topk"])
        self.assertGreater(report["cutoff_tie_count"], e["k"])

    def test_sorted_truncated_ties_still_fail_global_boundary(self):
        e = case("indexed", "ties")
        e["candidate_raw"] = e["oracle_raw"][-10:]
        e["checked_candidates"] = copy.deepcopy(e["candidate_raw"])
        e["fresh_check"] = fresh(e["checked_candidates"])
        e.update(native_strict_topk=False, native_filtered_topk=True, candidate_status="BLOCKER")
        self.assertFalse(f.audit_case(e, f.fixture())["strict_id_topk"])

    def test_initial_bm25_forged_consistently_in_both_arms(self):
        e = case("indexed", "rank")
        for name in ("candidate_raw", "oracle_raw"):
            for h in e[name]: h["score"] *= 2
        self.assertTrue(any("reference mismatch" in x for x in f.audit_case(e, f.fixture())["errors"]))

    def test_truncated_oracle_fails_even_if_claimed_complete(self):
        e = case("indexed", "ties")
        e["oracle_raw"] = e["oracle_raw"][:10]
        self.assertTrue(any("oracle missing/truncated" in x for x in f.audit_case(e, f.fixture())["errors"]))

    def test_scope_widening_and_metadata_tampering(self):
        for field, value in (("tenant", "tenant-b"), ("workspace", "workspace-b"), ("generation", 9), ("version", 3), ("source", "wrong")):
            e = case("indexed", "rank")
            e["candidate_raw"][0][field] = value
            self.assertTrue(f.audit_case(e, f.fixture())["errors"], field)

    def test_predicate_is_full_identity_and_never_overwrites_scope(self):
        e = case("indexed", "authority_prefilter")
        self.assertEqual(f.audit_case(e, f.fixture())["errors"], [])
        self.assertTrue(f.audit_case(e, f.fixture())["exact_filtered_topk"])
        for fragment in ("tenant = 'tenant-a'", "workspace = 'workspace-a'", "source_id", "version = 2", "generation = 7"):
            self.assertIn(fragment, e["scope_filter"])
        e["scope_filter"] = "id = 'authority-ok-00'"
        self.assertTrue(f.audit_case(e, f.fixture())["errors"])

    def test_scope_only_authority_shortage_is_blocker(self):
        result = f.audit_case(case("indexed", "deep_authority"), f.fixture())
        self.assertEqual(result["errors"], [])
        self.assertFalse(result["exact_filtered_topk"])
        self.assertEqual(result["blockers"], [])
        self.assertTrue(result["negative_control"])

    def test_mutated_fresh_authority_retained_evidence_rejected(self):
        records = evidence()
        event = next(r for r in records if r["kind"] == "fresh_authority")
        event["result"]["hits"] = event["input_hits"]
        self.assertTrue(any("fail closed" in x for x in f.audit(records)["errors"]))

    def test_n_plus_one_authority_and_false_turso_claim_rejected(self):
        e = case("indexed", "ties")
        e["fresh_check"]["fixture_batch_calls"] = len(e["candidate_raw"])
        e["fresh_check"]["authority_native_calls"] = 10
        self.assertTrue(f.audit_case(e, f.fixture())["errors"])

    def test_invalid_and_or_fuzzy_fast_search_rejected(self):
        for field, value in (("operator", "OR"), ("fuzziness", 1), ("wand_factor", 1.1), ("fast_search", True), ("prefilter", False)):
            e = case("indexed", "and")
            e[field] = value
            self.assertTrue(f.audit_case(e, f.fixture())["errors"], field)

    def test_missing_snapshot_compaction_index_plan_terminal(self):
        for kind in ("snapshot", "mutation", "terminal"):
            records = evidence()
            records.remove(next(r for r in records if r["kind"] == kind))
            self.assertTrue(f.audit(records)["errors"])
        e = case("indexed", "rank")
        e["plan"] = "TableScan"
        self.assertTrue(f.audit_case(e, f.fixture())["errors"])

    def test_duplicate_nonfinite_and_zero_scores_rejected(self):
        for score in (0, float("nan"), float("inf"), True):
            e = case("indexed", "rank")
            e["candidate_raw"][0]["score"] = score
            self.assertTrue(f.audit_case(e, f.fixture())["errors"])
        e = case("indexed", "ties")
        e["candidate_raw"][1] = e["candidate_raw"][0]
        self.assertTrue(f.audit_case(e, f.fixture())["errors"])

    def test_refresh_score_changes_reported_not_hidden(self):
        records = evidence()
        after = next(r for r in records if r.get("phase") == "after_refresh" and r.get("name") == "statistics_rank")
        for name in ("candidate_raw", "oracle_raw", "checked_candidates"):
            for h in after[name]: h["score"] *= 1.1
        after["fresh_check"] = fresh(after["checked_candidates"])
        report = f.audit(records)
        self.assertEqual(report["errors"], [])
        self.assertFalse(report["refresh_observation"]["stability_required"])
        self.assertTrue(any(r["delta"] != 0 for r in report["refresh_observation"]["score_deltas"]))

    def test_deadline_and_false_summary_acceptance_rejected(self):
        records = evidence()
        records[-1]["elapsed_ms"] = 180001
        records[0]["not_run"].remove("summary_placement")
        self.assertGreaterEqual(len(f.audit(records)["errors"]), 2)

    def test_altered_canonical_fixture_rejected(self):
        records = evidence()
        next(r for r in records if r["kind"] == "snapshot")["fixture"][0]["text"] = "forged"
        self.assertTrue(any("canonical fixture" in x for x in f.audit(records)["errors"]))

    def test_flat_match_plan_is_not_index_proof(self):
        e = case("indexed", "rank")
        e["plan"] = "LanceRead\n  FlatMatchQuery: column=text"
        self.assertTrue(any("plan missing" in x for x in f.audit_case(e, f.fixture())["errors"]))

    def test_authority_prefilter_cannot_return_ineligible_high_scores(self):
        e = case("indexed", "authority_prefilter")
        e["candidate_raw"] = e["oracle_raw"][:10]
        e["checked_candidates"] = []
        e["fresh_check"] = fresh([])
        e.update(native_bm25_topk=False, native_strict_topk=False, native_filtered_topk=False, candidate_status="BLOCKER")
        result = f.audit_case(e, f.fixture())
        self.assertFalse(result["bm25_valid_topk"])
        self.assertTrue(result["errors"])

    def test_native_metadata_and_tokenizer_tampering_rejected(self):
        for field, value in (("table_version", 0), ("stats", {"type": "FTS", "indexed_rows": 0, "unindexed_rows": 0})):
            records = evidence()
            next(r for r in records if r["kind"] == "snapshot")[field] = value
            self.assertTrue(f.audit(records)["errors"])
        records = evidence()
        next(r for r in records if r["kind"] == "setup")["index_parameters"]["stem"] = True
        self.assertTrue(f.audit(records)["errors"])

    def test_executed_order_api_is_audited_instead_of_assumed_blocker(self):
        records = evidence()
        e = next(r for r in records if r["kind"] == "ordered_api")
        e.update(status="EXECUTED", hits=case("indexed", "ties")["oracle_raw"][:10])
        report = f.audit(records)
        self.assertEqual(report["errors"], [])
        self.assertEqual(report["blockers"], [])


class V2ContractTests(unittest.TestCase):
    def test_v2_accepts_legal_tied_members_and_keeps_strict_observation(self):
        e = tied_v2_case()
        report = f.audit_case(e, f.fixture(), f.SCHEMA_V2)
        self.assertEqual(report["errors"], [])
        self.assertEqual(report["blockers"], [])
        self.assertTrue(report["bm25_valid_topk"])
        self.assertTrue(report["application_pre_rrf_valid"])
        self.assertFalse(report["strict_id_topk"])
        self.assertTrue(report["observations"])
        self.assertEqual([h["id"] for h in e["application_stage"]["rank_inputs"]], [f"tie-{n:03}" for n in range(86, 96)])

    def test_v2_full_report_is_bounded_pass_with_known_api_observation(self):
        records = evidence_v2()
        index = next(i for i, e in enumerate(records) if e.get("name") == "ties")
        records[index] = tied_v2_case()
        report = f.audit(records)
        self.assertEqual(report["errors"], [])
        self.assertEqual(report["blockers"], [])
        self.assertEqual(report["status"], "PASS_BOUNDED_DIAGNOSTIC")
        self.assertEqual(report["schema"], f.SCHEMA_V2+"-audit")
        self.assertTrue(any("unsupported" in x for x in report["observations"]))
        self.assertIn("production_rrf_integration", report["not_run"])
        self.assertFalse(report["production_acceptance"])
        self.assertEqual(len(report["cases"]), 42)

    def test_v1_stays_strict_and_unknown_or_relabelled_header_fails(self):
        old = evidence()
        report = f.audit(old)
        self.assertEqual(report["status"], "BLOCKER")
        self.assertEqual(report["source_schema"], f.SCHEMA_V1)
        self.assertEqual(report["schema"], f.SCHEMA_V1+"-audit")
        old[0]["schema"] = f.SCHEMA_V2
        self.assertTrue(f.audit(old)["errors"])
        old[0]["schema"] = "unknown-future-contract"
        self.assertTrue(f.audit(old)["errors"])
        old = evidence()
        old[0]["contract"] = f.CONTRACT_V2
        self.assertTrue(f.audit(old)["errors"])

    def test_preparation_deduplicates_and_orders_before_rank_assignment(self):
        a, z = {**case("indexed", "rank")["candidate_raw"][0], "id": "a"}, {**case("indexed", "rank")["candidate_raw"][0], "id": "z"}
        inputs = [z, a, z, a]
        prepared = f.prepare_rank_inputs(inputs)
        self.assertEqual([h["id"] for h in prepared], ["a", "z"])
        self.assertEqual(list(enumerate([h["id"] for h in prepared], 1)), [(1, "a"), (2, "z")])
        stage = application(inputs)
        self.assertEqual(stage["deduplicated_count"], 2)
        errors = []
        self.assertTrue(f.audit_application(stage, inputs, errors, "synthetic duplicate unit"))
        self.assertEqual(errors, [])
        stage["rank_inputs"] = [z, a]
        self.assertFalse(f.audit_application(stage, inputs, [], "unsorted before rank"))

    def test_preparation_conflicting_source_version_generation_score_fail_closed(self):
        original = case("indexed", "rank")["candidate_raw"][0]
        for field, value in (("source", "wrong"), ("version", 3), ("generation", 8), ("score", original["score"]+.1)):
            conflict = {**original, field: value}
            with self.assertRaises(ValueError):
                f.prepare_rank_inputs([original, conflict])
            errors = []
            self.assertFalse(f.audit_application(application([original]), [original, conflict], errors, "conflict"))
            self.assertTrue(errors)

    def test_preparation_uses_full_scoped_key_not_id_alone(self):
        a = case("indexed", "rank")["candidate_raw"][0]
        b = {**a, "workspace": "workspace-b"}
        self.assertEqual(len(f.prepare_rank_inputs([a, b])), 2)
        self.assertEqual(f.prepare_rank_inputs([]), [])

    def test_v2_application_missing_changed_dropped_added_or_unsorted_rejected(self):
        for mutate in (
            lambda s: s.update(rank_inputs=s["rank_inputs"][1:]),
            lambda s: s["rank_inputs"].reverse(),
            lambda s: s["rank_inputs"].append(s["rank_inputs"][0]),
            lambda s: s["rank_inputs"][0].update(score=123),
            lambda s: s["rank_inputs"][0].update(version=99),
            lambda s: s.update(rank_positions=list(reversed(s["rank_positions"]))),
            lambda s: s.update(stage="after_rrf"),
            lambda s: s.update(rrf_executed=True),
            lambda s: s.update(deduplicated_count=10),
        ):
            e = tied_v2_case()
            mutate(e["application_stage"])
            self.assertTrue(f.audit_case(e, f.fixture(), f.SCHEMA_V2)["errors"])
        e = tied_v2_case()
        del e["application_stage"]
        self.assertTrue(f.audit_case(e, f.fixture(), f.SCHEMA_V2)["errors"])

    def test_v2_does_not_hide_invalid_raw_scores_by_sorting_application_output(self):
        e = as_v2_case(case("indexed", "statistics_rank"))
        e["candidate_raw"].reverse()
        e["checked_candidates"] = copy.deepcopy(e["candidate_raw"])
        e["fresh_check"] = fresh(e["checked_candidates"])
        e["application_stage"] = application(e["checked_candidates"])
        self.assertTrue(any("raw native score stream" in x for x in f.audit_case(e, f.fixture(), f.SCHEMA_V2)["errors"]))

    def test_v2_does_not_relax_native_duplicates_or_worse_topk(self):
        e = tied_v2_case()
        e["candidate_raw"][1] = copy.deepcopy(e["candidate_raw"][0])
        e["checked_candidates"] = copy.deepcopy(e["candidate_raw"])
        e["fresh_check"] = fresh(e["checked_candidates"])
        e["application_stage"] = application(e["checked_candidates"])
        self.assertTrue(any("duplicate scoped identity" in x for x in f.audit_case(e, f.fixture(), f.SCHEMA_V2)["errors"]))
        e = as_v2_case(case("indexed", "rank"))
        e["candidate_raw"] = e["oracle_raw"][-1:]
        self.assertTrue(any("invalid BM25 TopK" in x for x in f.audit_case(e, f.fixture(), f.SCHEMA_V2)["errors"]))

    def test_v2_keeps_authority_safety_and_exact_filtered_topk_gates(self):
        e = as_v2_case(case("indexed", "authority_prefilter"))
        e["candidate_raw"] = e["oracle_raw"][:10]
        e["checked_candidates"] = []
        e["fresh_check"] = fresh([])
        e["application_stage"] = application([])
        result = f.audit_case(e, f.fixture(), f.SCHEMA_V2)
        self.assertTrue(result["errors"])
        self.assertFalse(result["exact_filtered_topk"])
        self.assertTrue(result["blockers"])
        e = as_v2_case(case("indexed", "rank"))
        e["fresh_check"].update(status="SNAPSHOT_CHANGED", hits=[])
        self.assertTrue(f.audit_case(e, f.fixture(), f.SCHEMA_V2)["errors"])

    def test_v2_application_cost_is_inside_measured_candidate(self):
        e = tied_v2_case()
        e["application_stage"]["elapsed_ms"] = 5
        e["timing_ms"]["application_prepare"] = 5
        self.assertTrue(any("wall excludes" in x for x in f.audit_case(e, f.fixture(), f.SCHEMA_V2)["errors"]))
        e = tied_v2_case()
        e["application_stage"]["elapsed_ms"] = 0
        self.assertTrue(any("timing disagrees" in x for x in f.audit_case(e, f.fixture(), f.SCHEMA_V2)["errors"]))

    def test_unexpected_api_error_is_not_the_known_unsupported_observation(self):
        records = evidence_v2()
        next(e for e in records if e["kind"] == "ordered_api")["error"] = "IO failure"
        self.assertTrue(any("unexpected order API failure" in x for x in f.audit(records)["errors"]))


class LauncherTests(unittest.TestCase):
    def test_missing_binary_does_not_launch(self):
        with mock.patch("subprocess.Popen") as popen, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                f.main(["--binary", "/no/such/native/binary", "--output", "/unused"])
        popen.assert_not_called()

    def test_timeout_kills_single_native_process_without_new_session(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root/"fake-native"
            binary.write_text("fixture only")
            binary.chmod(0o700)
            process = mock.Mock()
            process.wait.side_effect = [subprocess.TimeoutExpired("fixture", f.DEADLINE), -9]
            with mock.patch("subprocess.Popen", return_value=process) as popen, mock.patch.object(f, "lock_errors", return_value=[]), mock.patch.object(f, "baseline_evidence", return_value={"matches": True}), mock.patch("platform.platform", return_value="test"), contextlib.redirect_stdout(io.StringIO()):
                rc = f.main(["--binary", str(binary), "--output", str(root/"out")])
            self.assertEqual(rc, 1)
            process.kill.assert_called_once_with()
            self.assertEqual(process.wait.call_count, 2)
            self.assertNotIn("start_new_session", popen.call_args.kwargs)
            self.assertEqual(popen.call_args.args[0][1], "fts-v2")
            report = json.loads((root/"out/report.json").read_text())
            self.assertTrue(report["execution"]["native_process_killed"])
            self.assertTrue(report["execution"]["outer_timeout_shares_process_group"])
            self.assertNotEqual(report["status"], "PASS_BOUNDED_DIAGNOSTIC")

    def test_partial_and_malformed_report_not_accepted(self):
        self.assertTrue(f.audit(evidence()[:5])["errors"])
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp)/"partial.jsonl"
            p.write_text('{"kind":"header"}\n{"kind":')
            with self.assertRaises(json.JSONDecodeError): f.read_records(p)


if __name__ == "__main__":
    unittest.main()
