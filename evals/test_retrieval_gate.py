import json
from pathlib import Path
import tempfile
import unittest

import retrieval_gate as gate


def row(case_id, split, score, answerable):
    return {
        "id": case_id, "split": split, "query": case_id,
        "status": "ok", "resolve_status": "ok",
        "relevant_source_ids": ["x"] if answerable else [],
        "response": {"hits": [{"score": score}]},
        "context": {"sources": ["x"]}, "elapsed_ms": 1, "resolve_elapsed_ms": 2,
        "recall": 1 if answerable else None, "evidence_recall": 1 if answerable else None,
        "coverage": 1 if answerable else None, "false_positive": None if answerable else True,
    }


class RetrievalGate(unittest.TestCase):
    def test_failed_request_is_not_an_abstention(self):
        response = row("negative", "development", 0.5, False)
        for changes, vector_failed in [
            ({"status": "error"}, False), ({"resolve_status": "error"}, False), ({}, True),
        ]:
            failed = response | changes
            self.assertEqual(gate.gate(failed, None, 0.6, vector_failed), (failed, "request_failed"))
        after, state = gate.gate(response, 0.5, 0.6)
        self.assertEqual(state, "insufficient_evidence")
        self.assertFalse(after["false_positive"])

    def test_locks_from_development_and_does_not_retune_on_heldout(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            rows = [
                row("negative", "development", 0.5, False),
                row("positive", "development", 0.8, True),
                row("h-negative", "heldout", 0.9, False),
                row("h-positive", "heldout", 0.4, True),
            ]
            for mode in gate.MODES:
                (root / mode).mkdir()
                path = root / mode / "results.jsonl"
                path.write_text("".join(json.dumps(item) + "\n" for item in rows), encoding="utf-8")
            locked = gate.lock(root)
            self.assertTrue(locked["meets_development_guardrail"])
            self.assertGreater(locked["threshold"], 0.5)
            results = gate.report(root, locked)["splits"]
            self.assertEqual(results["development"]["summary"]["vector"]["after"]["no_answer_false_positives"], 0)
            self.assertEqual(results["heldout"]["summary"]["vector"]["after"]["no_answer_false_positives"], 1)
            self.assertEqual(results["heldout"]["summary"]["vector"]["after"]["document_recall_at_k"], 0)
            path.write_bytes(path.read_bytes() + b"\n")
            with self.assertRaisesRegex(ValueError, "does not match"):
                gate.report(root, locked)
