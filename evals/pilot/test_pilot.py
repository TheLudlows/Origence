import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from pilot import build_case, cases, generate, input_digest, observations, score, score_file


class PilotTests(unittest.TestCase):
    def setUp(self):
        self.tasks, self.oracle = cases("test")
        self.data = observations(self.tasks, self.oracle)
        self.excluded = {input_digest(task['input']) for task in self.tasks}

    def test_reproducible_and_seed_changes_instances(self):
        self.assertEqual(cases("test"), cases("test"))
        self.assertNotEqual(cases("test"), cases("other"))
        self.assertEqual(len(self.tasks), 16)

    def test_zero_observations_never_certify(self):
        result = score(self.data, self.tasks)
        self.assertEqual(result["acceptance"], "NOT_CERTIFIED")
        for group in result["groups"].values():
            self.assertEqual((group["tasks"], group["required"], group["task_success_rate"], group["verification_coverage"]), (8, 32, 0, 0))

    def test_success_and_coverage_are_separate(self):
        row = self.data["cases"][0]
        row.update(outcome="COMPLETED", reviewer="independent-test-reviewer", evidence=["synthetic-evidence:1"], verified_checks=row["required_checks"][:2])
        group = score(self.data, self.tasks)["groups"]["database_wait/baseline"]
        self.assertEqual(group["task_success_rate"], 0)
        self.assertEqual(group["verification_coverage"], 2 / 32)

    def test_missing_duplicate_and_changed_denominator_rejected(self):
        for mutation in (lambda d: d["cases"].pop(), lambda d: d["cases"].append(d["cases"][0]), lambda d: d["cases"][0]["required_checks"].pop()):
            data = copy.deepcopy(self.data)
            mutation(data)
            with self.assertRaises(ValueError):
                score(data, self.tasks)

    def test_unsubstantiated_success_rejected(self):
        self.data["cases"][0]["independent_success"] = True
        with self.assertRaises(ValueError):
            score(self.data, self.tasks)

    def test_even_all_success_does_not_certify_pilot(self):
        for row in self.data["cases"]:
            row.update(outcome="COMPLETED", independent_success=True, reviewer="synthetic-reviewer", evidence=["fixture-only"], verified_checks=row["required_checks"][:])
        self.assertEqual(score(self.data, self.tasks)["acceptance"], "NOT_CERTIFIED")

    def test_duplicate_or_unknown_checks_rejected(self):
        for checks in (["snapshot_support", "snapshot_support"], ["unknown"]):
            data = copy.deepcopy(self.data)
            data["cases"][0].update(outcome="COMPLETED", reviewer="reviewer", evidence=["ref"], verified_checks=checks)
            with self.assertRaises(ValueError):
                score(data, self.tasks)

    def test_timeout_stays_in_denominator(self):
        self.data["cases"][0]["outcome"] = "TIMEOUT"
        self.assertEqual(score(self.data, self.tasks)["groups"]["database_wait/baseline"]["tasks"], 8)

    def test_expense_oracle_matches_frozen_rule(self):
        for task, oracle in zip(self.tasks[8:], self.oracle[8:]):
            value = task["input"]
            expected = 0 if value["duplicate"] or not value["receipt_present"] else min(value["claim_minor"], value["policy_v2_cap_minor"])
            self.assertEqual(oracle["expected"]["payable_minor"], expected)

    def test_every_seed_has_distinct_expense_categories_and_v2_cap(self):
        # These are public test seeds, not the evaluator's sealed holdout seed.
        for seed, split in [(seed, split) for seed in ["public-development-1", "synthetic-holdout-test"] + [str(i) for i in range(100)] for split in ("prepare", "held-out")]:
            with self.subTest(seed=seed, split=split):
                tasks, oracle = cases(seed, split, self.excluded)
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
            tasks, oracle = cases(seed, split, self.excluded)
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
            self.assertEqual(cases("seed-a", split, self.excluded), cases("seed-a", split, self.excluded))
            for scenario in ("database_wait", "expense_review"):
                self.assertNotEqual(mapping("seed-a", split, scenario), mapping("seed-b", split, scenario))
        for scenario in ("database_wait", "expense_review"):
            self.assertNotEqual(mapping("seed-a", "prepare", scenario), mapping("seed-a", "held-out", scenario))

    def test_generated_agent_input_excludes_oracle(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "new"
            prep = Path(tmp) / "prepare"
            generate("test", prep)
            generate("synthetic-holdout-test", output, "held-out", [prep / "tasks.json"])
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

    def test_held_out_excludes_exposed_inputs_regardless_of_ids_or_key_order(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            previous = []
            exposed = set()
            for seed in ("public-development-1", "additional-public-batch"):
                batch, _ = cases(seed)
                for item in batch:
                    exposed.add(input_digest(item["input"]))
                    item["id"] = "renumbered-" + item["id"]
                    item["input"] = dict(reversed(list(item["input"].items())))
                path = root / (seed + ".json")
                path.write_text(json.dumps(batch), encoding="utf-8")
                previous.append(path)
            for label, paths in (("a", previous), ("b", reversed(previous))):
                generate("public-review-demo-0", root / label, "held-out", paths)
            raw = (root / "a" / "tasks.json").read_bytes()
            self.assertEqual(raw, (root / "b" / "tasks.json").read_bytes())
            held = {input_digest(item["input"]) for item in json.loads(raw)}
            self.assertEqual(len(held), 16)
            self.assertTrue(held.isdisjoint(exposed))
            manifest = json.loads((root / "a" / "manifest.json").read_bytes())
            self.assertEqual(set(manifest["excluded_tasks_sha256"]),
                             {hashlib.sha256(path.read_bytes()).hexdigest() for path in previous})

    def test_missing_or_exhausted_exclusions_do_not_publish_a_batch(self):
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "new"
            with self.assertRaisesRegex(ValueError, "requires a preparation"):
                generate("seed", target, "held-out")
            excluded = Path(tmp) / "exposed.json"
            excluded.write_text(json.dumps([{"input": build_case("database_wait", 0, i)[0]}
                                            for i in range(100)]), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "no unused database_wait"):
                generate("seed", target, "held-out", [excluded])
            self.assertFalse(target.exists())

    def test_score_checks_the_bundle_and_each_observation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            a, b = root / "a", root / "b"
            generate("public-development-1", a)
            generate("public-review-demo-0", b, "held-out", [a / "tasks.json"])
            sheet = a / "observations.json"
            original = json.loads(sheet.read_bytes())
            peer = json.loads((b / sheet.name).read_bytes())
            expected = hashlib.sha256((a / "tasks.json").read_bytes()).hexdigest()
            self.assertEqual(score_file(sheet)["tasks_sha256"], expected)
            sheet.write_text(json.dumps(peer), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "different task set"):
                score_file(sheet)
            mixed = copy.deepcopy(original)
            mixed["cases"][0] = peer["cases"][0]
            sheet.write_text(json.dumps(mixed), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "row belongs to a different task set"):
                score_file(sheet)
            mixed["cases"][0]["tasks_sha256"] = expected
            sheet.write_text(json.dumps(mixed), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "task input"):
                score_file(sheet)
            sheet.write_text(json.dumps(dict(original, kind="pilot-observations-v1")), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "v2 observation"):
                score_file(sheet)
            sheet.write_text(json.dumps(original), encoding="utf-8")
            (a / "tasks.json").write_bytes((b / "tasks.json").read_bytes())
            with self.assertRaisesRegex(ValueError, "manifest"):
                score_file(sheet)

    def test_held_out_requires_independent_frozen_tasks_sha256(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            prep, first, other = (root / name for name in ("prep", "first", "other"))
            generate("public-verify-92", prep)
            for batch in (first, other):
                generate("public-verify-92-" + batch.name, batch, "held-out", [prep / "tasks.json"])
            frozen = root / "public-test-freeze.json"
            data = (first / "tasks.json").read_bytes()
            digest = hashlib.sha256(data).hexdigest()
            frozen.write_text(json.dumps({"data": {"tasks_sha256": digest}}), encoding="utf-8")
            sheet = first / "observations.json"
            with self.assertRaisesRegex(ValueError, "requires --freeze"):
                score_file(sheet)
            with self.assertRaisesRegex(ValueError, "requires a held-out batch"):
                score_file(prep / sheet.name, freeze_path=frozen)
            with self.assertRaisesRegex(ValueError, "freeze.data.tasks_sha256"):
                score_file(other / sheet.name, freeze_path=frozen)
            result = score_file(sheet, freeze_path=frozen)
            self.assertEqual((result["split"], result["score_mode"], result["tasks_sha256"]),
                             ("held-out", "FROZEN_BATCH", digest))
            self.assertEqual((result["acceptance"], result["evidence_authenticity"]),
                             ("NOT_CERTIFIED", "NOT_CHECKED"))

            frozen.write_text('{"data":{"tasks_sha256":null}}', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "freeze.data.tasks_sha256"):
                score_file(sheet, freeze_path=frozen)
            frozen.write_text(json.dumps({"data": {"tasks_sha256": digest}}), encoding="utf-8")
            (first / "tasks.json").write_bytes(data + b"\n")
            manifest = first / "manifest.json"
            modified = json.loads(manifest.read_bytes())
            modified["sha256"]["tasks.json"] = hashlib.sha256(data + b"\n").hexdigest()
            manifest.write_text(json.dumps(modified), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "freeze.data.tasks_sha256"):
                score_file(sheet, freeze_path=frozen)

    def test_preparation_metrics_are_repeatable_and_do_not_score_public_handoff(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "prepare"
            generate("public-verify-92", root)
            sheet = root / "observations.json"
            data = json.loads(sheet.read_bytes())
            done, failed = (next(row for row in data["cases"] if row["id"] == name and row["arm"] == "baseline")
                            for name in ("database_wait-01", "database_wait-02"))
            done.update(outcome="COMPLETED", independent_success=True, reviewer="public-fixture",
                        evidence=["fixture-success"], verified_checks=done["required_checks"][:])
            failed.update(outcome="COMPLETED", reviewer="public-fixture", evidence=["fixture-failure"],
                          verified_checks=failed["required_checks"][:2])
            sheet.write_text(json.dumps(data), encoding="utf-8")
            raw = sheet.read_bytes()
            result = score_file(sheet)
            self.assertEqual(result, score_file(sheet))
            self.assertEqual(raw, sheet.read_bytes())
            self.assertEqual((result["split"], result["score_mode"]), ("prepare", "PREPARATION_ONLY"))
            group = result["groups"]["database_wait/baseline"]
            self.assertEqual((group["tasks"], group["required"]), (8, 32))
            self.assertEqual((group["task_success_rate"], group["verification_coverage"]), (1 / 8, 6 / 32))
            sheet.write_bytes(Path(__file__).with_name("public-handoff.windows.json").read_bytes())
            with self.assertRaisesRegex(ValueError, "split mismatch"):
                score_file(sheet)


if __name__ == "__main__":
    unittest.main()
