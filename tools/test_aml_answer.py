"""Answer proxy tests: no silent guessing or citation-free success."""
import json
import hashlib
from pathlib import Path
import unittest
from aml_answer import assess, validate_answer
from aml_drill import DrillError


class AnswerTests(unittest.TestCase):
    def test_partial_reference_or_missing_citation_cannot_succeed(self):
        output = {"answerable": True, "answer": "南区二号楼", "indices": [0]}
        self.assertFalse(assess(output, [["南区"], ["二号楼"]], [("s", 0), ("s", 1)], [("s", 0)])["supported_proxy_success"])
        self.assertFalse(assess(output, [["南区"], ["三号楼"]], [("s", 0)], [("s", 0)])["supported_proxy_success"])
        self.assertTrue(assess(output, [["南区"], ["二号楼"]], [("s", 0)], [("s", 0)])["supported_proxy_success"])

    def test_abstention_and_citation_contract(self):
        candidates = [{"id": "a", "content": "original"}]
        for output in [{"answerable": True, "answer": "guess", "indices": []},
                       {"answerable": False, "answer": "guess", "indices": []},
                       {"answerable": False, "answer": "", "indices": [0]},
                       {"answerable": True, "answer": "claim", "indices": [1]}]:
            with self.assertRaises(DrillError):
                validate_answer(output, candidates)
        output = {"answerable": False, "answer": "", "indices": []}
        self.assertEqual(validate_answer(output, candidates), [])
        self.assertTrue(assess(output, [], [], [])["supported_proxy_success"])
        self.assertFalse(assess(output, [["known"]], [("s", 0)], [])["supported_proxy_success"])

    def test_v3_freeze_matches_corpus_and_answer_references(self):
        root = Path(__file__).resolve().parents[1]
        freeze = json.loads((root / "evals/aml/v3/freeze.json").read_text(encoding="utf-8"))
        for field, path in [("corpus_sha256", "evals/aml/v3/corpus.json"), ("references_sha256", "evals/aml/experiments/answer-references.json")]:
            self.assertEqual(hashlib.sha256((root / path).read_bytes()).hexdigest(), freeze[field])

    def test_all_frozen_questions_have_reference_groups(self):
        root = Path(__file__).resolve().parents[1]
        refs = json.loads((root / "evals/aml/experiments/answer-references.json").read_text(encoding="utf-8"))
        for dataset, path in [("development", "v2"), ("unseen_test", "v3")]:
            corpus = json.loads((root / f"evals/aml/{path}/corpus.json").read_text(encoding="utf-8"))
            questions = [q for q in corpus["questions"] if q["split"] == dataset]
            self.assertEqual({q["id"] for q in questions}, set(refs[dataset]))
            for q in questions:
                self.assertEqual(bool(q["required"]), bool(refs[dataset][q["id"]]))
                self.assertTrue(all(group and all(alias for alias in group) for group in refs[dataset][q["id"]]))


if __name__ == "__main__":
    unittest.main()
