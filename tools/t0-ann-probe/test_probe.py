"""Test the report gate without installing or pretending to execute LanceDB."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("t0_probe", Path(__file__).with_name("probe.py"))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


def complete_report():
    config = {"rows":10000,"dimension":128,"queries_per_bucket":1,"index_kind":"ivf_flat"}
    checks = dict.fromkeys(("visibility_violations", "source_violations", "score_distance_violations", "duplicate_ids"), 0)
    def row(phase, bucket, snapshot):
        eligible = {"empty":0,"one":1,"lt_10":7,"lt_100":37,"source_version":8,"fresh_unindexed":16}.get(bucket,513)
        hits = [{"id":f"artifact-{i:09}","source":f"source-{i:07}","tenant":"tenant-a","workspace":"workspace-a",
                 "version":2,"generation":1,"distance":i/100.0,"score":1.0/(1.0+i/100.0)} for i in range(min(100,eligible))]
        return {"exact_hits":copy.deepcopy(hits),"ann_hits":copy.deepcopy(hits),"verified_ann_hits":copy.deepcopy(hits),
                "fallback_hits":copy.deepcopy(hits),"exact_plan":"LanceScan: exact","ann_plan":"ANNIVFPartition: synthetic test-only plan",
                "phase":phase,"bucket":bucket,"query":0,"snapshot":snapshot,"passed":True,"candidate_limit":100,
                "eligible_count":eligible,"exact_count":min(100,eligible),"ann_count":min(100,eligible),
                "recall_at_10":1.0 if eligible else None,"recall_at_100":1.0 if eligible else None,
                "fallback_exact_match":True,"native_ann_proved":True,"exact_bypass_proved":True,
                "exact_invariants":checks.copy(),"ann_invariants":checks.copy(),"fallback_invariants":checks.copy(),"exact_native_ms":1.2,"ann_native_ms":.7,
                "batched_512":{"exact_hits":copy.deepcopy(hits),"ann_hits":copy.deepcopy(hits),"verified_ann_hits":copy.deepcopy(hits),"native_calls_per_mode":(eligible+511)//512,"passed":True,"exact_matches_one_query":True,
                               "recall_at_10":1.0 if eligible else None,"recall_at_100":1.0 if eligible else None,"invariants":checks.copy(),"raw_ann_invariants":checks.copy(),"raw_exact_invariants":checks.copy()},
                "synthetic_authority_adversaries":{"wrong_source_version_generation_scope_rejected":True,
                                                   "withdrawal_between_native_and_final_rejected":True}}
    queries = []
    for i, phase in enumerate(probe.PHASES):
        for bucket in probe.BUCKETS + (() if i == 0 else ("fresh_unindexed",)):
            queries.append(row(phase,bucket,i+1))
    report = {"schema":probe.SCHEMA,"status":"PASS_SYNTHETIC_T0","config":config,"lancedb":"0.40.0",
              "candidate_limit":100,"queries":queries,"reopened_fresh_query":row("reopened","fresh_unindexed",3),
              "temporary_database_removed":True,"fixture_indexed_sha256":"a"*64,"fixture_final_sha256":"b"*64}
    for key, indexed, unindexed in (("indexed_evidence",9984,0),("fresh_evidence_before_refresh",9984,16),
                                     ("fresh_evidence_after_refresh",10000,0),("reopened_evidence",10000,0)):
        report[key] = {"list_indices":[{"name":"t0_vector_l2","type":"IVF_FLAT","columns":["vector"]}],"index_stats":{"indexed_rows":indexed,"unindexed_rows":unindexed,"type":"IVF_FLAT","distance_type":"L2","loss":None,"loss_status":probe.LOSS_STATUS}}
    return report


class ReportGateTests(unittest.TestCase):
    def test_complete_synthetic_report(self):
        self.assertEqual(probe.validate_report(complete_report()), [])

    def test_previous_dependency_pass_is_not_current_evidence(self):
        report = complete_report()
        report["lancedb"] = "0.23.1"
        self.assertIn("dependency or candidate-limit mismatch", probe.validate_report(report))

    def test_removed_training_loss_is_explicit_not_fabricated(self):
        for field, value in (("loss", 0.0), ("loss_status", "measured")):
            report = complete_report()
            report["indexed_evidence"]["index_stats"][field] = value
            self.assertTrue(any("training loss" in error for error in probe.validate_report(report)))
        report = complete_report()
        del report["indexed_evidence"]["index_stats"]["loss"]
        self.assertTrue(any("training loss" in error for error in probe.validate_report(report)))

    def test_lock_version_guard_requires_current_complete_dependency_identity(self):
        current = {"lancedb":"0.40.0", "lance":"13.0.0", "arrow-array":"58.0.0", "arrow-schema":"58.0.0"}
        with tempfile.TemporaryDirectory() as directory:
            lock = Path(directory) / "Cargo.lock"
            self.assertTrue(probe.lock_errors(lock))
            def write(versions):
                lock.write_text("\n".join(f'[[package]]\nname = "{name}"\nversion = "{version}"' for name, version in versions.items()))
            write(current)
            self.assertEqual(probe.lock_errors(lock), [])
            for name, version in (("lancedb","0.23.1"),("lance","1.0.1"),("arrow-array","56.2.0"),("arrow-schema","58.1.0")):
                write({**current,name:version})
                self.assertTrue(any(name in error for error in probe.lock_errors(lock)))
            write(current)
            with lock.open("a") as stream:
                stream.write('\n[[package]]\nname = "lancedb"\nversion = "0.23.1"\n')
            self.assertTrue(probe.lock_errors(lock))
            lock.write_text("not valid = [")
            self.assertTrue(probe.lock_errors(lock))

    def test_missing_and_duplicate_buckets_fail(self):
        report = complete_report()
        report["queries"].pop()
        self.assertTrue(probe.validate_report(report))
        report = complete_report()
        report["queries"].append(copy.deepcopy(report["queries"][0]))
        self.assertTrue(probe.validate_report(report))

    def test_tiny_bucket_recall_failure_is_not_hidden_by_average_or_fallback(self):
        report = complete_report()
        report["queries"][1]["recall_at_100"] = .94
        self.assertTrue(any("below 0.95" in e for e in probe.validate_report(report)))

    def test_nan_recall_fails(self):
        report = complete_report()
        report["queries"][1]["recall_at_10"] = float("nan")
        self.assertTrue(probe.validate_report(report))

    def test_nonzero_invariant_fails(self):
        for metric in ("visibility_violations", "source_violations", "score_distance_violations", "duplicate_ids"):
            report = complete_report()
            report["queries"][1]["ann_invariants"][metric] = 1
            self.assertTrue(probe.validate_report(report))

    def test_empty_must_be_empty_and_has_no_recall_percentage(self):
        report = complete_report()
        report["queries"][0]["ann_count"] = 1
        self.assertTrue(probe.validate_report(report))
        report = complete_report()
        report["queries"][0]["recall_at_10"] = 1.
        self.assertTrue(probe.validate_report(report))

    def test_index_presence_without_native_plan_is_not_ann_proof(self):
        report = complete_report()
        report["queries"][1]["native_ann_proved"] = False
        self.assertTrue(probe.validate_report(report))

    def test_fallback_must_match_exact(self):
        report = complete_report()
        report["queries"][1]["fallback_exact_match"] = False
        self.assertTrue(probe.validate_report(report))

    def test_fresh_coverage_and_refresh_are_distinct(self):
        report = complete_report()
        report["fresh_evidence_before_refresh"]["index_stats"]["unindexed_rows"] = 0
        self.assertTrue(probe.validate_report(report))
        report = complete_report()
        report["fresh_evidence_after_refresh"]["index_stats"]["unindexed_rows"] = 16
        self.assertTrue(probe.validate_report(report))

    def test_fixed_snapshot_and_512_batch_counts(self):
        report = complete_report()
        report["queries"][1]["snapshot"] = 1000
        self.assertTrue(probe.validate_report(report))
        report = complete_report()
        report["queries"][1]["batched_512"]["native_calls_per_mode"] = 2
        self.assertTrue(probe.validate_report(report))

    def test_batch_invariants_cannot_be_hidden_by_passed_label(self):
        report = complete_report()
        report["queries"][1]["batched_512"]["invariants"]["source_violations"] = 1
        self.assertTrue(probe.validate_report(report))

    def test_discarded_batched_native_and_fallback_violations_fail(self):
        for mode in ("raw_ann_invariants", "raw_exact_invariants"):
            report = complete_report()
            report["queries"][1]["batched_512"][mode]["visibility_violations"] = 1
            self.assertTrue(probe.validate_report(report))
        report = complete_report()
        report["queries"][1]["fallback_invariants"]["source_violations"] = 1
        self.assertTrue(probe.validate_report(report))

    def test_all_empty_or_wrong_reopen_cannot_pass(self):
        report = complete_report()
        for row in report["queries"] + [report["reopened_fresh_query"]]:
            row.update(eligible_count=0,exact_count=0,ann_count=0,recall_at_10=None,recall_at_100=None,native_ann_proved=False)
            row["batched_512"].update(native_calls_per_mode=0,recall_at_10=None,recall_at_100=None)
        self.assertTrue(probe.validate_report(report))
        report = complete_report()
        report["reopened_fresh_query"] = copy.deepcopy(report["queries"][0])
        self.assertTrue(probe.validate_report(report))

    def test_plan_text_must_agree_with_ann_proof(self):
        report = complete_report()
        report["queries"][1]["ann_plan"] = "LanceScan: exhaustive"
        self.assertTrue(probe.validate_report(report))

    def test_retained_hits_recompute_recall_and_fallback_scope(self):
        report = complete_report()
        report["queries"][1]["ann_hits"][0]["id"] = "missing-neighbor"
        report["queries"][1]["verified_ann_hits"][0]["id"] = "missing-neighbor"
        self.assertTrue(any("retained-hit recall" in e for e in probe.validate_report(report)))
        report = complete_report()
        report["queries"][1]["fallback_hits"][0]["tenant"] = "foreign"
        self.assertTrue(any("fallback differs" in e for e in probe.validate_report(report)))

    def test_index_list_and_fixture_digest_required(self):
        report = complete_report()
        del report["indexed_evidence"]["list_indices"]
        self.assertTrue(probe.validate_report(report))
        report = complete_report()
        del report["fixture_final_sha256"]
        self.assertTrue(probe.validate_report(report))

    def test_status_cannot_hide_runtime_failure(self):
        report = complete_report()
        report["status"] = "FAILED_RUNTIME"
        self.assertTrue(probe.validate_report(report))

    def test_summarize_keeps_limits_and_per_bucket_minima(self):
        text = probe.summarize(complete_report(), [])
        self.assertIn("not a production SLO", text)
        self.assertIn("minimum Recall@10/@100", text)
        self.assertIn("Native ANN failures remain failures", text)

    def test_missing_authority_adversary_fails_closed(self):
        report = complete_report()
        report["queries"][1]["synthetic_authority_adversaries"] = {"wrong_source_version_generation_scope_rejected": True}
        self.assertTrue(probe.validate_report(report))

    def test_unbounded_configuration_fails(self):
        report = complete_report()
        report["config"]["rows"] = 1000001
        self.assertTrue(probe.validate_report(report))

    def test_default_is_plan_only_and_requires_explicit_scale(self):
        command = [sys.executable, str(probe.HERE / "probe.py")]
        output = subprocess.run(command, text=True, capture_output=True, check=True)
        self.assertEqual(json.loads(output.stdout)["status"], "PLANNED_NOT_RUN")
        self.assertEqual(json.loads(output.stdout)["config"]["rows"], 10000)
        scale = subprocess.run(command + ["--rows","100000"], text=True, capture_output=True)
        self.assertNotEqual(scale.returncode, 0)
        self.assertIn("--allow-scale", scale.stderr)

    def test_baseline_hashes_match(self):
        self.assertTrue(probe.baseline_evidence()["matches"])


if __name__ == "__main__":
    unittest.main()
