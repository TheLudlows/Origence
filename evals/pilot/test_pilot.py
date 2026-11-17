import copy
import json
from pathlib import Path
import tempfile
import unittest

from pilot import cases, generate, observations, score


class PilotTests(unittest.TestCase):
    def setUp(self):
        self.tasks, self.oracle = cases("test")
        self.data = observations(self.tasks, self.oracle)

    def test_reproducible_and_seed_changes_instances(self):
        self.assertEqual(cases("test"), cases("test"))
        self.assertNotEqual(cases("test"), cases("other"))
        self.assertEqual(len(self.tasks), 16)

    def test_zero_observations_never_certify(self):
        result = score(self.data)
        self.assertEqual(result["acceptance"], "NOT_CERTIFIED")
        for group in result["groups"].values():
            self.assertEqual((group["tasks"], group["required"], group["task_success_rate"], group["verification_coverage"]), (8, 32, 0, 0))

    def test_success_and_coverage_are_separate(self):
        row = self.data["cases"][0]
        row.update(outcome="COMPLETED", reviewer="independent-test-reviewer", evidence=["synthetic-evidence:1"], verified_checks=row["required_checks"][:2])
        group = score(self.data)["groups"]["database_wait/baseline"]
        self.assertEqual(group["task_success_rate"], 0)
        self.assertEqual(group["verification_coverage"], 2 / 32)

    def test_missing_duplicate_and_changed_denominator_rejected(self):
        for mutation in (lambda d: d["cases"].pop(), lambda d: d["cases"].append(d["cases"][0]), lambda d: d["cases"][0]["required_checks"].pop()):
            data = copy.deepcopy(self.data)
            mutation(data)
            with self.assertRaises(ValueError):
                score(data)

    def test_unsubstantiated_success_rejected(self):
        self.data["cases"][0]["independent_success"] = True
        with self.assertRaises(ValueError):
            score(self.data)

    def test_even_all_success_does_not_certify_pilot(self):
        for row in self.data["cases"]:
            row.update(outcome="COMPLETED", independent_success=True, reviewer="synthetic-reviewer", evidence=["fixture-only"], verified_checks=row["required_checks"][:])
        self.assertEqual(score(self.data)["acceptance"], "NOT_CERTIFIED")

    def test_duplicate_or_unknown_checks_rejected(self):
        for checks in (["snapshot_support", "snapshot_support"], ["unknown"]):
            data = copy.deepcopy(self.data)
            data["cases"][0].update(outcome="COMPLETED", reviewer="reviewer", evidence=["ref"], verified_checks=checks)
            with self.assertRaises(ValueError):
                score(data)

    def test_timeout_stays_in_denominator(self):
        self.data["cases"][0]["outcome"] = "TIMEOUT"
        self.assertEqual(score(self.data)["groups"]["database_wait/baseline"]["tasks"], 8)

    def test_expense_oracle_matches_frozen_rule(self):
        for task, oracle in zip(self.tasks[8:], self.oracle[8:]):
            value = task["input"]
            expected = 0 if value["duplicate"] or not value["receipt_present"] else min(value["claim_minor"], value["policy_v2_cap_minor"])
            self.assertEqual(oracle["expected"]["payable_minor"], expected)

    def test_every_seed_has_distinct_expense_categories_and_v2_cap(self):
        # These are public test seeds, not the evaluator's sealed holdout seed.
        for seed, split in [(seed, split) for seed in ["public-development-1", "synthetic-holdout-test"] + [str(i) for i in range(100)] for split in ("prepare", "held-out")]:
            with self.subTest(seed=seed, split=split):
                tasks, oracle = cases(seed, split)
                self.assertEqual(len(tasks), 16)
                categories = {name: [] for name in ("normal", "capped", "duplicate", "missing")}
                for task, answer in zip(tasks[8:], oracle[8:]):
                    value, expected = task["input"], answer["expected"]
                    if value["duplicate"]:
                        category = "duplicate"
                        self.assertTrue(value["receipt_present"])
                        self.assertEqual(expected["decision"], "hold")
                        self.assertEqual(expected["payable_minor"], 0)
                    elif not value["receipt_present"]:
                        category = "missing"
                        self.assertEqual(expected["decision"], "hold")
                        self.assertEqual(expected["payable_minor"], 0)
                    elif value["claim_minor"] > value["policy_v2_cap_minor"]:
                        category = "capped"
                        self.assertEqual(expected["decision"], "recommend")
                        self.assertEqual(expected["policy_version"], 2)
                        self.assertEqual(expected["payable_minor"], value["policy_v2_cap_minor"])
                        self.assertLess(expected["payable_minor"], value["claim_minor"])
                        # Using v1 would reimburse the full claim: must differ.
                        self.assertLess(value["claim_minor"], value["policy_v1_cap_minor"])
                        self.assertNotEqual(expected["payable_minor"], min(value["claim_minor"], value["policy_v1_cap_minor"]))
                    else:
                        category = "normal"
                        self.assertEqual(expected["decision"], "recommend")
                        self.assertEqual(expected["payable_minor"], value["claim_minor"])
                    categories[category].append(value)
                for values in categories.values():
                    self.assertEqual(len(values), 2)
                    self.assertNotEqual(values[0], values[1])

    def test_shuffle_reproducible_and_not_fixed_to_case_number(self):
        def mapping(seed, split, scenario):
            tasks, oracle = cases(seed, split)
            result = []
            for task, answer in zip(tasks, oracle):
                if task["scenario"] != scenario:
                    continue
                self.assertEqual(task["id"], answer["id"])
                value = task["input"]
                if scenario == "database_wait":
                    result.append(answer["expected"]["classification"])
                else:
                    result.append("duplicate" if value["duplicate"] else "missing" if not value["receipt_present"] else "capped" if value["claim_minor"] > value["policy_v2_cap_minor"] else "normal")
            return result
        for split in ("prepare", "held-out"):
            self.assertEqual(cases("seed-a", split), cases("seed-a", split))
            for scenario in ("database_wait", "expense_review"):
                self.assertNotEqual(mapping("seed-a", split, scenario), mapping("seed-b", split, scenario))
        for scenario in ("database_wait", "expense_review"):
            self.assertNotEqual(mapping("seed-a", "prepare", scenario), mapping("seed-a", "held-out", scenario))

    def test_generated_agent_input_excludes_oracle(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "new"
            generate("synthetic-holdout-test", output, "held-out")
            tasks = json.loads((output / "tasks.json").read_text())
            self.assertNotIn("synthetic-holdout-test", json.dumps(tasks))
            self.assertNotIn("held-out", json.dumps(tasks))
            for task in tasks:
                self.assertEqual(set(task), {"id", "scenario", "synthetic", "input"})
                self.assertNotIn("expected", task["input"])
                self.assertNotIn("payable_minor", task["input"])
            self.assertTrue((output / "oracle.json").exists())

    def test_output_never_overwrites(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "new"
            generate("test", output)
            self.assertEqual(json.loads((output / "manifest.json").read_text())["product_execution"], "NOT_RUN")
            with self.assertRaises(FileExistsError):
                generate("different", output)


if __name__ == "__main__":
    unittest.main()
