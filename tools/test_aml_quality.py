"""Metric and provenance regression tests for the AML quality evaluator."""
import json
import hashlib
from pathlib import Path
import unittest
import tempfile
from unittest.mock import patch
from aml_report import write_report
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
            def fail(result):
                result["queries"] = [{"id": "completed-before-failure"}]
                raise RuntimeError("private provider diagnostic")
            with patch("urllib.request.urlopen") as request:
                self.assertEqual(write_report(report, {"status": "running"}, fail), 1)
                request.assert_not_called()
                artifact = json.loads(report.read_text(encoding="utf-8"))
                self.assertEqual(artifact["status"], "failed")
                self.assertEqual(artifact["error"], "evaluation_failed")
                self.assertEqual(len(artifact["queries"]), 1)
                self.assertNotIn("summary", artifact)
                original = report.read_bytes()
                with self.assertRaises(FileExistsError):
                    write_report(report, {"status": "running"}, fail)
                self.assertEqual(report.read_bytes(), original)

    def test_v2_frozen_source_disjoint_splits_and_labels(self):
        folder = Path(__file__).resolve().parents[1] / "evals/aml/v2"
        data = (folder / "corpus.json").read_bytes()
        corpus = json.loads(data)
        freeze = json.loads((folder / "freeze.json").read_text(encoding="utf-8"))
        self.assertEqual(hashlib.sha256(data).hexdigest(), freeze["corpus_sha256"])
        self.assertEqual(sum(len(b["messages"]) for b in corpus["batches"]), 512)
        self.assertEqual(len(corpus["questions"]), 30)
        sources = {split: set() for split in freeze["splits"]}
        known = {(b["user_id"], b["session_id"], i): m for b in corpus["batches"] for i, m in enumerate(b["messages"])}
        for q in corpus["questions"]:
            self.assertEqual(q["user_id"], freeze["splits"][q["split"]])
            self.assertEqual(not q["required"], q["category"] == "no_answer")
            for gold in q["required"]:
                key = (q["user_id"], gold["session_id"], gold["message_index"])
                self.assertIn(key, known)
                sources[q["split"]].add(key)
        self.assertFalse(sources["development"] & sources["holdout"])
        dev_text = {m["content"] for (u, _, _), m in known.items() if u == "workshop"}
        hold_text = {m["content"] for (u, _, _), m in known.items() if u == "gallery"}
        self.assertFalse(dev_text & hold_text)
        self.assertEqual(len({q["id"] for q in corpus["questions"]}), 30)

    def test_same_session_batches_fail_closed_for_legacy_gold_keys(self):
        message = {"role": "user", "content": "Atlas approval", "timestamp": None}
        batches = [{"user_id": "u", "session_id": "same", "request_id": request,
                    "messages": [message]} for request in ["one", "two"]]
        metadata = {"session_id": "same", "external_batch_id": "one", "message_index": 0,
                    "source_event_id": "00112233-4455-6677-8899-aabbccddeeff",
                    "parser": "message-batch-ranges-v1", "parser_version": 1, "chunking_version": 1,
                    "message_count": 1, "role": "user", "timestamp": None,
                    "source_path": "/messages/0/content", "byte_basis": "message_content_utf8",
                    "byte_start": 0, "byte_end": len(message["content"].encode())}
        hit = {"id": "synthetic", "content": "Source metadata: " + json.dumps(metadata) + "\n" + message["content"]}
        self.assertEqual(decode_hits([hit], batches[:1], "u"), [("same", 0)])
        historical_metadata = {key: metadata[key] for key in ("session_id", "message_index", "role", "timestamp")}
        historical_hit = {**hit, "content": "Source metadata: " + json.dumps(historical_metadata) + "\n" + message["content"]}
        with self.assertRaises(DrillError):
            decode_hits([historical_hit], batches[:1], "u")
        self.assertEqual(decode_hits([historical_hit], batches[:1], "u", allow_legacy=True), [("same", 0)])
        with self.assertRaisesRegex(DrillError, "ambiguous_quality_source"):
            decode_hits([historical_hit], batches, "u", allow_legacy=True)
        with self.assertRaisesRegex(DrillError, "ambiguous_quality_source"):
            decode_hits([hit], batches, "u")
        for field, value in [("external_batch_id", "two"), ("source_event_id", "invalid"), ("parser", "future")]:
            altered = {**metadata, field: value}
            bad = {**hit, "content": "Source metadata: " + json.dumps(altered) + "\n" + message["content"]}
            with self.assertRaises(DrillError):
                decode_hits([bad], batches[:1], "u")

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
                    "timestamp": message["timestamp"], "role": message["role"],
                    "external_batch_id": batch["request_id"],
                    "source_event_id": "00112233-4455-6677-8899-aabbccddeeff",
                    "parser": "message-batch-ranges-v1", "parser_version": 1, "chunking_version": 1,
                    "message_count": len(batch["messages"]), "source_path": "/messages/0/content",
                    "byte_basis": "message_content_utf8", "byte_start": 0,
                    "byte_end": len(message["content"].encode("utf-8"))}
        hit = {"id": "a", "content": "Source metadata: " + json.dumps(metadata) + "\n" + message["content"]}
        self.assertEqual(decode_hits([hit], batches, batch["user_id"]), [(batch["session_id"], 0)])
        for data, user in [([hit], "other"), ([hit, hit], batch["user_id"]),
                           ([{**hit, "content": hit["content"] + "corruption"}], batch["user_id"])]:
            with self.assertRaises(DrillError):
                decode_hits(data, batches, user)


if __name__ == "__main__":
    unittest.main()
