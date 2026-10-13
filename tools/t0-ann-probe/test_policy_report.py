"""Mutation tests only: synthetic report objects do not pretend to run LanceDB."""
import copy
from functools import lru_cache
import hashlib
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("policy_report", Path(__file__).with_name("policy_report.py"))
policy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(policy)


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


@lru_cache(maxsize=1)
def _template():
    """Well-formed validator input, not native or exhaustive-neighbor evidence.

Hits have genuine fixture identities and distances. Retained test oracle lists
are made from a bounded subset, so these tests validate report checks only.
"""
    report = {"schema": policy.SCHEMA, "policy_id": policy.POLICY_ID,
              "config": policy.CONFIG.copy(), "contract_sha256": policy.CONTRACT_SHA256,
              "status": "PASS_QUALITY", "lancedb": "0.23.1", "candidate_limit": 100,
              "fixture_indexed_sha256": policy.fixture_sha256(9984),
              "fixture_final_sha256": policy.fixture_sha256(10000),
              "temporary_database_removed": True, "setup_sdk_calls": policy.SETUP_SDK_CALLS.copy(),
              "call_count_scope": policy.CALL_COUNT_SCOPE,
              "timing_protocol": "pair_then_oracle_audit_then_bucket_plans", "cases": [], "queries": [], "plan_pool": {}}
    phases = {"indexed": 3, "fresh_before_refresh": 4, "fresh_after_refresh": 5, "reopened": 5}
    checks = dict.fromkeys(policy.INVARIANTS, 0)
    for case_id, expected in policy.expected_cases().items():
        case = {key: copy.deepcopy(value) for key, value in expected.items() if not key.startswith("_")}
        case["snapshot"] = phases[case["phase"]]
        case["plan_pass"] = True
        for arm, route in (("selected", case["selected_route"]),
                           ("exact", "empty" if not case["eligible_count"] else "exact")):
            plan = (policy.EMPTY_PLAN if route == "empty" else
                    "ANNIVF: uuid=12345678-1234-4234-9234-123456789abc, minimum_nprobes=32, maximum_nprobes=Some(32), synthetic test-only plan" if route == "ann" else
                    "KNNVectorDistance: exhaustive, synthetic test-only plan")
            report["plan_pool"][digest(plan)] = plan
            case[arm + "_plan_sha256"] = digest(plan)
        case["slot_plan_hashes"] = [{"slot": slot, "selected": case["selected_plan_sha256"],
                                    "exact": case["exact_plan_sha256"]} for slot in range(8)]
        report["cases"].append(case)
        for slot in range(8):
            vector = list(policy.expected_query(case_id, slot))
            hits = []
            for index in expected["_indices"][:100]:
                hit = {key: policy.fixture_row(index)[key] for key in policy.IDENTITY}
                hit["distance"] = policy.f32(policy._distance(case_id, slot, index))
                hit["score"] = policy.f32(1.0 / policy.f32(1.0 + hit["distance"]))
                hits.append(hit)
            hits.sort(key=lambda hit: (-hit["score"], hit["id"]))
            def arm(route, wall):
                return {"route": route, "wall_ms": wall, "native_calls": int(bool(hits)),
                        "eligible_count": case["eligible_count"], "raw_hits": hits, "verified_hits": hits,
                        "components_ms": dict.fromkeys(policy.COMPONENTS, 0.01)}
            report["queries"].append({"case_id": case_id, "phase": case["phase"], "bucket": case["bucket"],
                "slot": slot, "query_kind": "near" if slot < 4 else "bridge", "query_vector": vector,
                "query_sha256": policy.vector_sha256(vector), "selected_first": slot % 2 == 0,
                "selected": arm(case["selected_route"], 0.7), "exact": arm("exact" if hits else "empty", 1.0),
                "oracle_hits": hits, "selected_raw_invariants": checks, "selected_verified_invariants": checks,
                "exact_raw_invariants": checks, "exact_verified_invariants": checks, "oracle_invariants": checks,
                "recall_at_10": 1.0 if hits else None, "recall_at_100": 1.0 if hits else None,
                "exact_matches_oracle": True,
                "selected_exact_matches_oracle": None if case["selected_route"] == "ann" else True,
                "quality_pass": True})
    report["call_counts"] = {"selected_native": 552, "exact_native": 552, "oracle_native": 552, "explain_plans": 1104}
    for key, indexed, unindexed in (("indexed_evidence", 9984, 0), ("fresh_evidence_before_refresh", 9984, 16),
                                     ("fresh_evidence_after_refresh", 10000, 0), ("reopened_evidence", 10000, 0)):
        report[key] = {"list_indices": [{"name": "t0_vector_l2", "type": "IVF_FLAT", "columns": ["vector"]}],
                       "index_stats": {"indexed_rows": indexed, "unindexed_rows": unindexed,
                                       "type": "IVF_FLAT", "distance_type": "L2"}}
    for key, version, count, fragments in (("layout_before_compact", 1, 9984, 39),
            ("layout_after_compact", 2, 9984, 3), ("layout_after_index", 3, 9984, 3),
            ("layout_before_index_refresh", 4, 10000, 4), ("layout_after_index_refresh", 5, 10000, 4)):
        report[key] = {"version": version, "rows": count, "fragments": fragments, "bytes": 1000000}
    return report


def complete_report():
    return copy.deepcopy(_template())


def query(report, case_id="indexed/dense_current", slot=0):
    return next(row for row in report["queries"] if row["case_id"] == case_id and row["slot"] == slot)


def case(report, case_id="indexed/dense_current"):
    return next(row for row in report["cases"] if row["case_id"] == case_id)


def replace_plan(report, target, arm, text, slots=None):
    """Update referenced text honestly, leaving validation to reject bad proof."""
    slots = set(range(8)) if slots is None else set(slots)
    reference = digest(text)
    report["plan_pool"][reference] = text
    for entry in target["slot_plan_hashes"]:
        if entry["slot"] in slots:
            entry[arm] = reference
    if 0 in slots:
        target[arm + "_plan_sha256"] = reference
    used = {entry[key] for row in report["cases"] for entry in row["slot_plan_hashes"]
            for key in ("selected", "exact")}
    report["plan_pool"] = {key: text for key, text in report["plan_pool"].items() if key in used}


class PolicyReportTests(unittest.TestCase):
    def assert_rejected(self, report, fragment=None):
        errors = policy.validate_report(report)
        self.assertTrue(errors)
        if fragment:
            self.assertTrue(any(fragment in error for error in errors), errors[:20])

    def test_complete_report_quality_and_exploratory_benefit(self):
        report = complete_report()
        self.assertEqual(policy.validate_report(report), [])
        summary = policy.benefit_summary(report)
        self.assertTrue(summary["quality_passed"])
        self.assertTrue(summary["performance_passed"])
        self.assertAlmostEqual(summary["workload_sum_time_ratio"], .7)
        self.assertEqual(summary["ann_median_paired_ratio"], .7)
        self.assertEqual(len(summary["per_bucket_median_paired_ratio"]), 72)

    def test_fixture_regeneration_matches_retained_native_smoke_digests(self):
        self.assertEqual(policy.fixture_sha256(9984), "104b42b105d709b9743df93b2160be89e92ada29b4d954482e501d32c332ba07")
        self.assertEqual(policy.fixture_sha256(10000), "a15ff3af30877988a2ed85e0dee321ee88a099e46eab302d626804d9364c892b")

    def test_routing_boundaries_and_fraction(self):
        self.assertEqual(policy.selected_route(0, 10000), "empty")
        self.assertEqual(policy.selected_route(4095, 10000), "exact")
        self.assertEqual(policy.selected_route(4096, 10000), "ann")
        self.assertEqual(policy.selected_route(4097, 10000), "ann")
        self.assertEqual(policy.selected_route(4096, 20000), "exact")

    def test_matrix_distinguishes_count_rotated_and_subsample_cases(self):
        cases = policy.expected_cases()
        self.assertEqual(len(cases), 72)
        self.assertEqual(cases["indexed/count_4095"]["eligible_count"], 4095)
        self.assertEqual(cases["indexed/count_4096"]["selected_route"], "ann")
        self.assertNotEqual(cases["indexed/rotated_4096"]["eligible_ids"], cases["indexed/count_4096"]["eligible_ids"])
        self.assertEqual(cases["indexed/subsample_2"]["eligible_count"], 2170)
        self.assertEqual(cases["indexed/historical_generation"]["scope"]["version"], 1)
        self.assertEqual(cases["reopened/fresh_unindexed"]["eligible_count"], 16)

    def test_config_and_contract_mutations_fail(self):
        for key, value in (("rows", 100000), ("dimension", 384), ("queries_per_bucket", 7),
                           ("partitions", 32), ("nprobes", 64), ("seed", 53), ("index_kind", "ivf_pq")):
            report = complete_report()
            report["config"][key] = value
            self.assert_rejected(report, "contract")
        report = complete_report()
        report["contract_sha256"] = "0" * 64
        self.assert_rejected(report, "contract")

    def test_frozen_fixture_hash_cannot_be_replaced_by_any_digest(self):
        report = complete_report()
        report["fixture_final_sha256"] = "a" * 64
        self.assert_rejected(report, "canonical fixture digest")

    def test_missing_and_duplicated_cases_or_queries_fail(self):
        for collection in ("cases", "queries"):
            report = complete_report()
            report[collection].pop()
            self.assert_rejected(report, "coverage")
            report = complete_report()
            report[collection][-1] = copy.deepcopy(report[collection][0])
            self.assert_rejected(report, "coverage")

    def test_empty_all_pairs_cannot_pass(self):
        report = complete_report()
        report["cases"] = []
        report["queries"] = []
        report["call_counts"] = dict.fromkeys(report["call_counts"], 0)
        self.assert_rejected(report, "coverage")
        benefit = policy.benefit_summary(report)
        self.assertFalse(benefit["performance_passed"])
        self.assertEqual(benefit["recommendation"], "exact")

    def test_eligible_count_ids_order_hash_and_scope_mutations(self):
        for key, value in (("eligible_count", 4095), ("selected_route", "exact"),
                           ("rotation", 1), ("case_stride", 2), ("snapshot_rows", 20000),
                           ("eligible_ids_sha256", "f" * 64)):
            report = complete_report()
            case(report)[key] = value
            self.assert_rejected(report, "frozen eligibility/shape")
        report = complete_report()
        target = case(report)
        target["eligible_ids"].reverse()
        target["eligible_ids_sha256"] = policy.ids_sha256(target["eligible_ids"])
        self.assert_rejected(report, "eligible_ids")
        report = complete_report()
        case(report)["scope"]["generation"] = True
        self.assert_rejected(report, "scope")

    def test_wrong_arm_route_and_bool_counts_fail(self):
        for arm, key, value in (("selected", "route", "exact"), ("exact", "route", "ann"),
                                ("selected", "native_calls", True), ("exact", "eligible_count", 4096)):
            report = complete_report()
            query(report)[arm][key] = value
            self.assert_rejected(report, "mismatch")

    def test_balance_and_near_bridge_identity_are_frozen(self):
        for key, value in (("selected_first", False), ("query_kind", "bridge"), ("slot", True), ("phase", "reopened")):
            report = complete_report()
            query(report)[key] = value
            self.assert_rejected(report)

    def test_query_vector_and_hash_both_mutated_still_fail(self):
        report = complete_report()
        target = query(report)
        target["query_vector"][0] += 0.01
        target["query_sha256"] = policy.vector_sha256(target["query_vector"])
        self.assert_rejected(report, "frozen query vector/hash")
        report = complete_report()
        query(report)["query_sha256"] = "a" * 64
        self.assert_rejected(report, "frozen query vector/hash")

    def test_empty_queries_all_have_constant_vectors_and_zero_calls(self):
        report = complete_report()
        for slot in range(8):
            row = query(report, "indexed/empty", slot)
            self.assertEqual(row["query_vector"], [.25] * 128)
            self.assertEqual(row["selected"]["native_calls"], 0)
            self.assertIsNone(row["recall_at_100"])
        query(report, "indexed/empty")["selected"]["native_calls"] = 1
        self.assert_rejected(report, "native_calls")
        report = complete_report()
        query(report, "indexed/empty")["recall_at_10"] = 1.0
        self.assert_rejected(report, "recall")

    def test_plan_marker_hash_probe_bounds_and_native_claim(self):
        mutations = ("LanceScan: exact", "ANNIVF: minimum_nprobes=64, maximum_nprobes=Some(64)", "")
        for plan in mutations:
            report = complete_report()
            target = case(report)
            replace_plan(report, target, "selected", plan)
            self.assert_rejected(report, "plan")
        report = complete_report()
        case(report)["selected_plan_sha256"] = "a" * 64
        self.assert_rejected(report, "plan hash")
        report = complete_report()
        case(report)["plan_pass"] = False
        self.assert_rejected(report, "plan claim")

    def test_exhaustive_operator_index_uuid_and_setup_counts_are_required(self):
        report = complete_report()
        target = case(report)
        replace_plan(report, target, "exact", "LanceScan: no demonstrated KNN execution")
        self.assert_rejected(report, "KNNVectorDistance")
        report = complete_report()
        target = case(report)
        replace_plan(report, target, "selected", "ANNIVF: uuid=not-a-uuid, minimum_nprobes=32, maximum_nprobes=Some(32)")
        self.assert_rejected(report, "index UUID")
        report = complete_report()
        report["setup_sdk_calls"]["add"] = 1
        self.assert_rejected(report, "setup_sdk_calls")
        report = complete_report()
        del report["call_count_scope"]
        self.assert_rejected(report, "call_count_scope")

    def test_exact_and_empty_plan_cannot_be_ann(self):
        for case_id, arm in (("indexed/one", "selected"), ("indexed/dense_current", "exact"), ("indexed/empty", "selected")):
            report = complete_report()
            target = case(report, case_id)
            replace_plan(report, target, arm, "ANNIVF: unexpected")
            self.assert_rejected(report, "plan contradicts")

    def test_per_vector_plan_proof_rejects_bad_later_slot(self):
        report = complete_report()
        target = case(report)
        first_plan = target["selected_plan_sha256"]
        replace_plan(report, target, "selected", "KNNVectorDistance: exact later slot", slots=[7])
        self.assertEqual(target["selected_plan_sha256"], first_plan)
        self.assert_rejected(report, "indexed/dense_current/7/selected: plan contradicts")

    def test_per_vector_plan_coverage_must_be_exactly_eight_distinct_slots(self):
        for replacement in ([], [{"slot": 0, "selected": "a", "exact": "b"}] * 8, None):
            report = complete_report()
            case(report)["slot_plan_hashes"] = replacement
            self.assert_rejected(report, "per-vector plan coverage")
        report = complete_report()
        case(report)["slot_plan_hashes"][7]["slot"] = True
        self.assert_rejected(report, "per-vector plan coverage")

    def test_plan_pool_rejects_hash_tampering_missing_and_unused_entries(self):
        report = complete_report()
        reference = case(report)["selected_plan_sha256"]
        report["plan_pool"][reference] += " changed"
        self.assert_rejected(report, "content hash mismatch")
        report = complete_report()
        del report["plan_pool"][case(report)["selected_plan_sha256"]]
        self.assert_rejected(report, "actual plan missing from pool")
        report = complete_report()
        report["plan_pool"][digest("unused")] = "unused"
        self.assert_rejected(report, "unused or missing")
        report = complete_report()
        del report["plan_pool"]
        self.assert_rejected(report, "plan pool missing")

    def test_obsolete_single_vector_proof_and_old_explain_count_fail(self):
        report = complete_report()
        case(report)["selected_plan"] = "ANNIVF: obsolete evidence"
        self.assert_rejected(report, "obsolete single-vector")
        report = complete_report()
        report["call_counts"]["explain_plans"] = 138
        self.assert_rejected(report, "call counts")

    def test_discarded_raw_violation_cannot_be_hidden_by_verified_hits(self):
        report = complete_report()
        target = query(report)
        target["selected"]["raw_hits"] = copy.deepcopy(target["selected"]["raw_hits"])
        target["selected"]["raw_hits"][0]["tenant"] = "foreign"
        self.assert_rejected(report, "scope violation")
        self.assert_rejected(report, "final verification changed")

    def test_forged_identity_or_ineligible_row_fails_independent_of_counters(self):
        for field, value in (("source", "forged"), ("version", 1), ("generation", 0),
                             ("tenant", "tenant-b"), ("workspace", "workspace-b"), ("id", "artifact-000000000")):
            report = complete_report()
            query(report)["selected"]["raw_hits"][0][field] = value
            self.assert_rejected(report, "violation")

    def test_forged_consistent_distance_and_score_still_fails_full_vector_check(self):
        report = complete_report()
        hit = query(report, "indexed/one")["oracle_hits"][0]
        hit["distance"] = 99.0
        hit["score"] = 0.01
        self.assert_rejected(report, "full-vector distance/score")

    def test_nonfinite_distance_and_score_and_incomplete_hits_fail(self):
        for field, value in (("distance", float("nan")), ("score", float("inf")), ("version", True)):
            report = complete_report()
            query(report)["oracle_hits"][0][field] = value
            self.assert_rejected(report, "incomplete or nonfinite")
        report = complete_report()
        del query(report)["oracle_hits"][0]["source"]
        self.assert_rejected(report, "incomplete or nonfinite")

    def test_duplicate_overlimit_and_unsorted_hits_fail(self):
        report = complete_report()
        target = query(report)
        target["oracle_hits"][1] = target["oracle_hits"][0]
        self.assert_rejected(report, "duplicate")
        report = complete_report()
        query(report)["oracle_hits"].append(query(report)["oracle_hits"][0])
        self.assert_rejected(report, "candidate limit")
        report = complete_report()
        query(report)["oracle_hits"].reverse()
        self.assert_rejected(report, "ordering")

    def test_raw_and_verified_cannot_be_different_even_if_both_authorized(self):
        report = complete_report()
        row = query(report)
        row["selected"]["raw_hits"] = row["selected"]["raw_hits"][:-1]
        self.assert_rejected(report, "final verification changed")

    def test_recall_is_recomputed_per_pair_and_quality_failure_is_retained(self):
        report = complete_report()
        row = query(report)
        row["selected"]["raw_hits"] = row["selected"]["raw_hits"][:94]
        row["selected"]["verified_hits"] = row["selected"]["raw_hits"]
        self.assert_rejected(report, "contradicts scalar")
        row["recall_at_100"] = .94
        row["quality_pass"] = False
        report["status"] = "FAIL_QUALITY"
        self.assert_rejected(report, "per-pair recall@100 below 0.95")
        self.assertEqual(policy.benefit_summary(report)["recommendation"], "exact")

    def test_small_exact_route_must_match_oracle_at_100_percent(self):
        report = complete_report()
        row = query(report, "indexed/lt_100")
        row["selected"]["raw_hits"] = row["selected"]["raw_hits"][:-1]
        row["selected"]["verified_hits"] = row["selected"]["raw_hits"]
        row["recall_at_100"] = 36 / 37
        self.assert_rejected(report, "selected exact does not match")

    def test_exact_baseline_not_just_selected_route_must_match_oracle(self):
        report = complete_report()
        row = query(report)
        row["exact"]["raw_hits"] = row["exact"]["raw_hits"][:-1]
        row["exact"]["verified_hits"] = row["exact"]["raw_hits"]
        self.assert_rejected(report, "exact does not match retained oracle")

    def test_nan_recall_and_native_false_pass_cannot_hide(self):
        report = complete_report()
        query(report)["recall_at_100"] = float("nan")
        self.assert_rejected(report, "recall")
        report = complete_report()
        query(report)["quality_pass"] = False
        self.assert_rejected(report, "quality claim")
        report = complete_report()
        report["status"] = "FAILED_RUNTIME"
        self.assert_rejected(report, "native status")

    def test_every_invariant_counter_must_be_present_integer_and_zero(self):
        for key in ("selected_raw_invariants", "selected_verified_invariants", "exact_raw_invariants",
                    "exact_verified_invariants", "oracle_invariants"):
            report = complete_report()
            query(report)[key] = dict.fromkeys(policy.INVARIANTS, 0)
            query(report)[key]["source_violations"] = 1
            self.assert_rejected(report, key)
        report = complete_report()
        query(report)["oracle_invariants"] = dict.fromkeys(policy.INVARIANTS, False)
        self.assert_rejected(report, "oracle_invariants")

    def test_snapshot_lifecycle_index_coverage_and_cleanup_fail_closed(self):
        for key in ("indexed_evidence", "fresh_evidence_before_refresh", "fresh_evidence_after_refresh", "reopened_evidence"):
            report = complete_report()
            report[key]["index_stats"]["indexed_rows"] = 0
            self.assert_rejected(report, "coverage mismatch")
        report = complete_report()
        case(report, "reopened/fresh_unindexed")["snapshot"] = 4
        self.assert_rejected(report, "fixed snapshot")
        report = complete_report()
        report["layout_after_index_refresh"]["fragments"] += 1
        self.assert_rejected(report, "fragment layout")
        report = complete_report()
        report["temporary_database_removed"] = False
        self.assert_rejected(report, "cleanup")

    def test_native_and_explain_call_counts_are_recomputed(self):
        for key in ("selected_native", "exact_native", "oracle_native", "explain_plans"):
            report = complete_report()
            report["call_counts"][key] -= 1
            self.assert_rejected(report, "call counts")

    def test_timing_components_and_positive_walls_are_required(self):
        for value in (0, -1, float("nan"), float("inf"), True):
            report = complete_report()
            query(report)["selected"]["wall_ms"] = value
            self.assert_rejected(report, "wall time")
        report = complete_report()
        query(report)["selected"]["components_ms"]["authority_map"] = 10
        self.assert_rejected(report, "component sum")
        report = complete_report()
        del query(report)["exact"]["components_ms"]["verification"]
        self.assert_rejected(report, "timing components")

    def test_performance_failure_does_not_erase_quality_or_tune_policy(self):
        report = complete_report()
        for row in report["queries"]:
            row["selected"]["wall_ms"] = 1.0
        self.assertEqual(policy.validate_report(report), [])
        summary = policy.benefit_summary(report)
        self.assertTrue(summary["quality_passed"])
        self.assertFalse(summary["performance_passed"])
        self.assertEqual(summary["recommendation"], "exact")
        self.assertTrue(any("0.90" in error for error in summary["performance_errors"]))

    def test_ann_only_slowdown_cannot_be_hidden_by_workload_mean(self):
        report = complete_report()
        for row in report["queries"]:
            row["selected"]["wall_ms"] = 1.0 if row["selected"]["route"] == "ann" else .1
        summary = policy.benefit_summary(report)
        self.assertLess(summary["workload_sum_time_ratio"], .90)
        self.assertFalse(summary["performance_passed"])
        self.assertTrue(any("ANN-selected" in error for error in summary["performance_errors"]))

    def test_one_phase_bucket_slowdown_cannot_hide_in_pooled_median(self):
        report = complete_report()
        for row in report["queries"]:
            if row["case_id"] == "fresh_before_refresh/lt_100":
                row["selected"]["wall_ms"] = 1.11
        summary = policy.benefit_summary(report)
        self.assertLess(summary["workload_sum_time_ratio"], .90)
        self.assertFalse(summary["performance_passed"])
        self.assertTrue(any("fresh_before_refresh/lt_100" in error for error in summary["performance_errors"]))

    def test_eight_ratios_use_arithmetic_median_not_p50_nearest_rank(self):
        report = complete_report()
        for row in report["queries"]:
            if row["case_id"] == "indexed/one":
                row["selected"]["wall_ms"] = 1.0 if row["slot"] < 4 else 1.3
        summary = policy.benefit_summary(report)
        self.assertEqual(summary["per_bucket_median_paired_ratio"]["indexed/one"], 1.15)
        self.assertFalse(summary["performance_passed"])

    def test_duplicate_timing_slot_cannot_satisfy_performance_coverage(self):
        report = complete_report()
        query(report, "indexed/one", 1)["slot"] = 0
        summary = policy.benefit_summary(report)
        self.assertFalse(summary["performance_passed"])
        self.assertTrue(any("coverage" in error for error in summary["performance_errors"]))

    def test_walls_not_components_determine_performance(self):
        report = complete_report()
        for row in report["queries"]:
            row["selected"]["wall_ms"] = 1.2
            row["selected"]["components_ms"] = dict.fromkeys(policy.COMPONENTS, .000001)
        self.assertFalse(policy.benefit_summary(report)["performance_passed"])

    def test_two_rebuilds_must_be_evaluated_separately(self):
        good, bad = complete_report(), complete_report()
        for row in bad["queries"]:
            row["selected"]["wall_ms"] = 1.0
        summaries = [policy.benefit_summary(report) for report in (good, bad)]
        self.assertTrue(summaries[0]["performance_passed"])
        self.assertFalse(summaries[1]["performance_passed"])
        self.assertLess((summaries[0]["workload_sum_time_ratio"] + summaries[1]["workload_sum_time_ratio"]) / 2, .90)
        self.assertFalse(all(summary["performance_passed"] for summary in summaries))

    def test_malformed_shapes_return_errors_instead_of_crashing(self):
        for report in (None, [], {}, {"queries": 7}, {"cases": [None], "queries": [None]},
                       {"cases": [{"case_id": []}], "queries": [{"case_id": {}, "slot": []}]}):
            self.assert_rejected(report)
            self.assertFalse(policy.benefit_summary(report)["performance_passed"])

    def test_summary_discloses_limits_and_keeps_failures(self):
        report = complete_report()
        query(report)["selected"]["wall_ms"] = 10000
        text = policy.summarize(report)
        self.assertIn("Exploratory performance: FAIL", text)
        self.assertIn("Recommendation: exact", text)
        self.assertIn("both fresh rebuilds must pass separately", text)
        self.assertIn("does not independently execute an exhaustive", text)
        self.assertIn("No threshold/probe tuning", text)


if __name__ == "__main__":
    unittest.main()
