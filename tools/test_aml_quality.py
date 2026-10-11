"""Metric and provenance regression tests for the AML quality evaluator."""
import json
from pathlib import Path
import unittest
import tempfile
from unittest.mock import patch
import aml_quality
from aml_drill import DrillError
from aml_quality import decode_hits, score


class QualityTests(unittest.TestCase):
    def test_partial_multihop_and_duplicate_candidates(self):
        result = score([("s", 0), ("s", 1)], [("s", 9), ("s", 0), ("s", 0)], 5)
        self.assertEqual(result["recall"], .5)
        self.assertFalse(result["all_evidence"])
        self.assertEqual(result["reciprocal_rank"], .5)
        self.assertTrue(score([("s", 0), ("s", 1)], [("s", 1), ("s", 0)], 5)["all_evidence"])
        self.assertEqual(score([("s", 0)], [("s", 9), ("s", 0)], 1)["recall"], 0)

    def test_no_answer_is_separate_from_recall(self):
        self.assertIsNone(score([], [("s", 0)], 5)["recall"])
        self.assertTrue(score([], [("s", 0)], 5)["nonempty_no_answer"])
        self.assertFalse(score([], [], 5)["nonempty_no_answer"])

    def test_failed_run_preserves_partial_evidence_without_success_summary(self):
        with tempfile.TemporaryDirectory() as folder:
            report = Path(folder) / "report.json"
            def fail(args, result):
                result["queries"] = [{"id": "completed-before-failure"}]
                raise RuntimeError("private provider diagnostic")
            with patch("sys.argv", ["aml_quality", "--binary", "unused", "--report", str(report)]), patch.object(aml_quality, "evaluate", side_effect=fail), patch("builtins.print"):
                self.assertEqual(aml_quality.main(), 1)
                artifact = json.loads(report.read_text(encoding="utf-8"))
                self.assertEqual(artifact["status"], "failed")
                self.assertEqual(artifact["error"], "evaluation_failed")
                self.assertEqual(len(artifact["queries"]), 1)
                self.assertNotIn("summary", artifact)
                original = report.read_bytes()
                with self.assertRaises(FileExistsError):
                    aml_quality.main()
                self.assertEqual(report.read_bytes(), original)

    def test_corpus_labels_and_response_provenance(self):
        corpus = json.loads((Path(__file__).resolve().parents[1] / "evals/aml/corpus.json").read_text(encoding="utf-8"))
        batches = corpus["batches"]
        known = {(b["user_id"], b["session_id"], i) for b in batches for i in range(len(b["messages"]))}
        self.assertEqual(len({q["id"] for q in corpus["questions"]}), len(corpus["questions"]))
        for q in corpus["questions"]:
            self.assertEqual(not q["required"], q["category"] == "no_answer")
            for gold in q["required"]:
                self.assertIn((q["user_id"], gold["session_id"], gold["message_index"]), known)
        batch = batches[0]
        message = batch["messages"][0]
        metadata = {"session_id": batch["session_id"], "message_index": 0,
                    "timestamp": message["timestamp"], "role": message["role"]}
        hit = {"id": "a", "content": "Source metadata: " + json.dumps(metadata) + "\n" + message["content"]}
        self.assertEqual(decode_hits([hit], batches, batch["user_id"]), [(batch["session_id"], 0)])
        for data, user in [([hit], "other"), ([hit, hit], batch["user_id"]),
                           ([{**hit, "content": hit["content"] + "corruption"}], batch["user_id"])]:
            with self.assertRaises(DrillError):
                decode_hits(data, batches, user)


if __name__ == "__main__":
    unittest.main()
