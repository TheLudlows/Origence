"""Diagnostic integrity: stale reports, missing hops, and oracle isolation."""
import copy
import gzip
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from aml_diagnose import (ARMS, check_artifact, classify, diagnose, diagnose_question,
                          indexed, load_dataset, oracle_hits, payload_digest)
from aml_drill import DrillError
from aml_experiment import evidence_payload
from aml_quality import decode_hits, stream_digest


class DiagnosisTests(unittest.TestCase):
    def test_first_loss_and_answer_outcome_are_separate(self):
        metrics = {"abstained": True, "supported_proxy_success": False, "reference_match": False}
        gold = [("s", 0), ("s", 1)]
        for seed, pool, selected, expected in [
            (gold[:1], gold[:1], gold[:1], "retrieval_top100"),
            (gold, gold[:1], gold[:1], "candidate_cutoff"),
            (gold, gold, gold[:1], "selection_or_budget"),
            (gold, gold, gold, None),
        ]:
            result = classify(gold, seed, pool, selected, metrics)
            self.assertEqual(result["first_evidence_loss"], expected)
            self.assertEqual(result["answer_outcome"], "abstained")
            self.assertEqual(result["reader_proxy_failure_with_complete_evidence"], expected is None)
        self.assertEqual(classify([], [], [], [], {**metrics, "abstained": False})["answer_outcome"], "unsupported_answer")

    def test_artifacts_must_be_complete_and_bound_to_input(self):
        valid = {"status": "completed", "corpus_sha256": "c", "input_sha256": "i", "references_sha256": "r"}
        check_artifact(valid, "c", "i", "r")
        for field, value in [("status", "failed"), ("corpus_sha256", "old"),
                             ("input_sha256", "other"), ("references_sha256", "changed")]:
            with self.assertRaises(DrillError):
                check_artifact({**valid, field: value}, "c", "i", "r")
        for rows in [[{"id": "a"}, {"id": "a"}], [{"id": "b"}], []]:
            with self.assertRaises(DrillError):
                indexed(rows, {"a"})

    def test_streaming_hash_preserves_large_input_bytes(self):
        raw = bytes(range(256)) * 8193
        class BoundedStream(io.BytesIO):
            def read(self, size=-1):
                if not 0 < size <= 1024 * 1024:
                    raise AssertionError("unbounded hash read")
                return super().read(size)
        self.assertEqual(stream_digest(BoundedStream(raw)), hashlib.sha256(raw).hexdigest())

    def test_lossless_archive_preserves_input_link(self):
        raw = b'{"content":"original"}\n'
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json.gz"
            with gzip.open(path, "wb") as stream:
                stream.write(raw)
            self.assertEqual(payload_digest(path), hashlib.sha256(raw).hexdigest())

    def test_oracle_uses_exact_gold_sources_without_reference_labels(self):
        corpus, questions, refs, _, _, _ = load_dataset("development")
        q = next(q for q in questions if len(q["required"]) > 1)
        hits = oracle_hits(q, corpus)
        self.assertEqual(len(decode_hits(hits, corpus["batches"], q["user_id"])), len(q["required"]))
        self.assertTrue(all(h["id"].startswith("diagnostic-only:") for h in hits))
        payload = {"question": q["query"], "evidence": evidence_payload(hits)}
        self.assertEqual(set(payload), {"question", "evidence"})
        self.assertTrue(all(set(h) == {"index", "content"} for h in payload["evidence"]))
        foreign = {**q, "user_id": "different-user"}
        with self.assertRaises(DrillError):
            oracle_hits(foreign, corpus)
        no_answer = next(q for q in questions if not q["required"])
        self.assertEqual(oracle_hits(no_answer, corpus), [])

    def test_saved_answer_tampering_is_not_silently_scored(self):
        corpus, questions, refs, rows, answers, _ = load_dataset("development")
        q = questions[0]
        corrupted = copy.deepcopy(answers[q["id"]])
        corrupted["vector"]["cited_ids"] = ["foreign-id"]
        with self.assertRaisesRegex(DrillError, "answer_citation_mismatch"):
            diagnose_question(q, corpus, refs, rows[q["id"]], corrupted)
        corrupted = copy.deepcopy(answers[q["id"]])
        corrupted["vector"]["metrics"]["supported_proxy_success"] = False
        with self.assertRaisesRegex(DrillError, "saved_answer_metrics_mismatch"):
            diagnose_question(q, corpus, refs, rows[q["id"]], corrupted)

    def test_both_frozen_reports_remain_consistent(self):
        for dataset, expected in [("development", 15), ("new_topics", 20)]:
            report = diagnose(dataset)
            self.assertEqual(len(report["queries"]), expected)
            self.assertEqual(set(report["summary"]), set(ARMS))
            for summary in report["summary"].values():
                self.assertEqual(sum(summary["answer_outcomes"].values()), expected)


if __name__ == "__main__":
    unittest.main()
