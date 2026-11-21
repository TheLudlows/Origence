import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from p6_bind import PROVENANCE, binding
from pilot import input_digest

RUN = "00000000-0000-4000-8000-000000000001"
VERIFICATION = "00000000-0000-4000-8000-000000000002"


class P6BindingTests(unittest.TestCase):
    def write_json(self, name, value):
        return self.write(name, json.dumps(value).encode() + b"\n")

    def write(self, name, data):
        (self.root / name).write_bytes(data)
        return hashlib.sha256(data).hexdigest()

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.prefix = {
            "run_id": RUN, "through_sequence": 2, "prefix_digest": "a" * 64,
            "basis_sequences": [1, 2],
        }
        self.member = {
            "case_id": "database_wait-01", "scenario": "database_wait",
            "client": "codex", "arm": "reuse", "run_id": RUN,
        }
        self.input = {"snapshot": {"pool_active": 3}}
        self.task = {"id": self.member["case_id"], "scenario": self.member["scenario"],
                     "input": self.input}
        self.tasks_hash = self.write_json("tasks.json", [self.task])
        self.freeze_hash = self.write_json("freeze.json", {"data": {"tasks_sha256": self.tasks_hash}})
        self.row = {
            "id": self.member["case_id"], "scenario": self.member["scenario"],
            "arm": "reuse", "input_sha256": input_digest(self.input),
            "tasks_sha256": self.tasks_hash, "outcome": "COMPLETED",
            "evidence": ["case-link.json"],
        }
        self.observation = {"kind": "pilot-observations-v2", "split": "held-out",
                            "product_execution": "RECORDED", "tasks_sha256": self.tasks_hash,
                            "cases": [self.row]}
        self.delivery = dict(self.member, input=self.input)
        self.receipt = {
            **PROVENANCE, "run_id": RUN, "withdrawal": None,
            "recorder_principal_id": RUN, "owner_key_id": RUN, "recorded_at_ms": 1,
            "assessment": {**{k: v for k, v in self.prefix.items() if k != "run_id"},
                           "verification_id": VERIFICATION, "conclusion": "failed",
                           "verifier_label": "synthetic test", "assessed_at_ms": 1,
                           "basis_summary": "synthetic basis", "reported_skill_version": None},
        }
        self.link = {
            **{k: v for k, v in self.member.items() if k != "run_id"},
            "pilot_item": "P6", "status": "RECORDED", "run_prefix": self.prefix,
            "independent_original": {
                "status": "RECORDED", "ref": "original.bin",
                "sha256": self.write("original.bin", b"synthetic original bytes"),
            },
            "owner_transcription": {
                "status": "RECORDED", "receipt_ref": "receipt.json",
                "verification_id": VERIFICATION, "withdrawal_status": "active",
            },
        }
        self.prepare()

    def prepare(self):
        delivery_sha = self.write_json("delivery.json", self.delivery)
        index_sha = self.write_json("index.json", {"run_prefix": self.prefix, "files": [{
            "path": "delivery.json", "sha256": delivery_sha,
            "bytes": (self.root / "delivery.json").stat().st_size,
            "type": "delivered_input", "case_id": self.member["case_id"], "pilot_item": "P6",
        }]})
        self.link["evidence_index"] = {"ref": "index.json", "sha256": index_sha}
        self.link["owner_transcription"]["receipt_sha256"] = self.write_json("receipt.json", self.receipt)
        self.row["run_id"] = self.prefix["run_id"]
        self.row["case_link_sha256"] = self.write_json("case-link.json", self.link)
        self.write_json("observations.json", self.observation)

    def check(self, **overrides):
        options = dict(case_link=self.root / "case-link.json", freeze=self.root / "freeze.json",
                       tasks=self.root / "tasks.json", observation=self.root / "observations.json",
                       freeze_sha256=self.freeze_hash, delivery_ref="delivery.json")
        return binding(**dict(options, **overrides))

    def test_matching_failed_inconclusive_and_passed_claims_are_structure_only(self):
        for conclusion in ("failed", "inconclusive", "passed"):
            with self.subTest(conclusion=conclusion):
                self.receipt["assessment"]["conclusion"] = conclusion
                self.prepare()
                result = self.check()
                self.assertEqual(result["result"], "STRUCTURAL_BINDING_ONLY")
                self.assertEqual(result["pilot_acceptance"], "NOT_CERTIFIED")
                self.assertEqual(result["current_run_or_withdrawal"], "NOT_CHECKED")

    def test_cli_returns_only_structure_and_rejects_withdrawal(self):
        command = [
            sys.executable, str(Path(__file__).with_name("p6_bind.py")),
            str(self.root / "case-link.json"),
            "--freeze", str(self.root / "freeze.json"), "--freeze-sha256", self.freeze_hash,
            "--tasks", str(self.root / "tasks.json"),
            "--observation", str(self.root / "observations.json"),
            "--delivery-ref", "delivery.json",
        ]
        result = subprocess.run(command, capture_output=True, check=True, timeout=10)
        self.assertEqual(json.loads(result.stdout)["result"], "STRUCTURAL_BINDING_ONLY")
        self.receipt["withdrawal"] = {"reason": "synthetic test"}
        self.prepare()
        result = subprocess.run(command, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, b"")
        self.assertIn(b"saved receipt is withdrawn", result.stderr)

    def test_freeze_requires_expected_raw_digest(self):
        with self.assertRaisesRegex(ValueError, "freeze SHA-256"):
            self.check(freeze_sha256="0" * 64)

    def test_rejects_blocked_or_contaminated_freeze_even_with_matching_digest(self):
        for extra in ({"status": "BLOCKED"}, {"data": {
            "tasks_sha256": self.tasks_hash, "contaminated": True,
        }}):
            with self.subTest(extra=extra):
                freeze = {"data": {"tasks_sha256": self.tasks_hash}, **extra}
                frozen_hash = self.write_json("freeze.json", freeze)
                with self.assertRaisesRegex(ValueError, "blocked or contaminated"):
                    self.check(freeze_sha256=frozen_hash)

    def test_reuse_client_cannot_change(self):
        self.link["client"] = self.delivery["client"] = "claude_code"
        self.prepare()
        with self.assertRaisesRegex(ValueError, "unexpected client"):
            self.check()

    def test_rejects_changed_task_bytes(self):
        path = self.root / "tasks.json"
        path.write_bytes(path.read_bytes() + b" ")
        with self.assertRaisesRegex(ValueError, "tasks SHA-256"):
            self.check()

    def test_rejects_task_from_other_batch_and_wrong_observation_input(self):
        self.observation["tasks_sha256"] = "3" * 64
        self.prepare()
        with self.assertRaisesRegex(ValueError, "belong to"):
            self.check()
        self.observation["tasks_sha256"] = self.tasks_hash
        self.row["input_sha256"] = "5" * 64
        self.prepare()
        with self.assertRaisesRegex(ValueError, "frozen case input"):
            self.check()

    def test_rejects_duplicate_row_or_missing_case_link(self):
        self.observation["cases"] = [self.row, self.row]
        self.prepare()
        with self.assertRaisesRegex(ValueError, "one matching row"):
            self.check()
        self.observation["cases"] = [self.row]
        self.row["evidence"] = ["other.json"]
        self.prepare()
        with self.assertRaisesRegex(ValueError, "must reference"):
            self.check()

    def test_same_case_retry_cannot_reuse_prior_observation(self):
        previous = (self.root / "observations.json").read_bytes()
        self.prefix["run_id"] = self.delivery["run_id"] = self.receipt["run_id"] = VERIFICATION
        self.prepare()
        self.assertEqual(self.check()["result"], "STRUCTURAL_BINDING_ONLY")
        self.write("observations.json", previous)
        with self.assertRaisesRegex(ValueError, "observation row Run mismatch"):
            self.check()

    def test_exact_case_link_digest_is_required(self):
        path = self.root / "case-link.json"
        path.write_bytes(path.read_bytes() + b" ")
        with self.assertRaisesRegex(ValueError, "case link SHA-256"):
            self.check()

    def test_not_run_does_not_validate(self):
        self.row["outcome"] = "NOT_RUN"
        self.prepare()
        with self.assertRaisesRegex(ValueError, "no recorded execution"):
            self.check()
        self.link["independent_original"]["status"] = "NOT_RUN"
        self.prepare()
        with self.assertRaisesRegex(ValueError, "must be RECORDED"):
            self.check()

    def test_public_example_does_not_validate_as_recorded(self):
        sample = Path(__file__).parent / "examples" / "p6-synthetic" / "case-link.json"
        with self.assertRaisesRegex(ValueError, "not synthetic"):
            self.check(case_link=sample)

    def test_rejects_unindexed_or_mismatched_delivery(self):
        self.write_json("unindexed.json", self.delivery)
        with self.assertRaisesRegex(ValueError, "one matching row"):
            self.check(delivery_ref="unindexed.json")
        for key, value in (("arm", "baseline"), ("run_id", VERIFICATION),
                           ("input", {"snapshot": {}})):
            with self.subTest(key=key):
                old = self.delivery[key]
                self.delivery[key] = value
                self.prepare()
                with self.assertRaisesRegex(ValueError, "delivery record|delivered input"):
                    self.check()
                self.delivery[key] = old

    def test_checks_original_and_receipt_raw_bytes(self):
        receipt = self.root / "receipt.json"
        receipt.write_bytes(receipt.read_bytes() + b" ")
        with self.assertRaisesRegex(ValueError, "receipt SHA-256"):
            self.check()
        self.prepare()
        (self.root / "original.bin").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "original SHA-256"):
            self.check()

    def test_judgement_cannot_change_receipt_run_or_provenance(self):
        self.receipt["assessment"]["basis_sequences"] = [2]
        self.prepare()
        with self.assertRaisesRegex(ValueError, "receipt Run prefix mismatch"):
            self.check()
        self.receipt["assessment"]["basis_sequences"] = [1, 2]
        self.receipt["verifier_authentication"] = "verified"
        self.prepare()
        with self.assertRaisesRegex(ValueError, "receipt provenance"):
            self.check()

    def test_missing_and_withdrawn_receipt_never_validate(self):
        self.receipt["withdrawal"] = {"reason": "synthetic test"}
        self.prepare()
        with self.assertRaisesRegex(ValueError, "is withdrawn"):
            self.check()
        del self.receipt["withdrawal"]
        self.prepare()
        with self.assertRaisesRegex(KeyError, "withdrawal"):
            self.check()

    def test_original_and_receipt_hardlink_alias_is_rejected(self):
        original = self.root / "original.bin"
        original.unlink()
        try:
            os.link(self.root / "receipt.json", original)
        except (NotImplementedError, OSError):
            self.skipTest("test filesystem cannot create a hardlink")
        self.link["independent_original"]["sha256"] = hashlib.sha256(original.read_bytes()).hexdigest()
        self.row["case_link_sha256"] = self.write_json("case-link.json", self.link)
        self.write_json("observations.json", self.observation)
        with self.assertRaisesRegex(ValueError, "must be separate"):
            self.check()

    def test_original_cannot_escape_package(self):
        self.link["independent_original"]["ref"] = "../outside"
        self.prepare()
        with self.assertRaisesRegex(ValueError, "path segment"):
            self.check()


if __name__ == "__main__":
    unittest.main()
