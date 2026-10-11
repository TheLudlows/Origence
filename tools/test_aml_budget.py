import unittest
from aml_budget import Budget, checked_order
from aml_budget_eval import question_payload
from aml_drill import DrillError
from aml_public import adapt, timestamp


class BudgetTests(unittest.TestCase):
    def test_no_partial_evidence_or_budget_overrun(self):
        budget = object.__new__(Budget)
        budget.size = lambda hits: {"reference_tokens": sum(h["size"] for h in hits), "utf8_bytes": sum(h["size"] for h in hits)}
        hits = [{"size": 3}, {"size": 8}, {"size": 1}]
        selected, size = budget.pack(hits, 5, 10, 10)
        self.assertEqual(selected, hits[:1])  # deterministic prefix, no silent slicing or skipping
        self.assertEqual(size["reference_tokens"], 3)
        self.assertEqual(budget.pack(hits, 5, 2, 2)[0], [])

    def test_ranking_validation(self):
        self.assertEqual(checked_order([1, 0], 2), [1, 0])
        for indices in ([0, 0], [True, 0], [0], [2, 0]):
            with self.assertRaises(DrillError): checked_order(indices, 2)

    def test_public_labels_scope_and_shared_history(self):
        records = []
        for qid in ("a", "b_abs"):
            records.append({"question_id": qid, "question_type": "multi-session", "question": "When?",
                "question_date": "2023/05/30 (Tue) 23:40", "answer": "GOLD_SECRET", "answer_session_ids": ["s"],
                "haystack_session_ids": ["s"], "haystack_dates": ["2023/05/20 (Sat) 02:21"],
                "haystack_sessions": [[{"role": "user", "content": "原文🙂" * 3000}]]})
        corpus, labels = adapt(records, ["a", "b_abs"])
        self.assertNotEqual(corpus["questions"][0]["user_id"], corpus["questions"][1]["user_id"])
        self.assertEqual(labels["a"]["cluster"], labels["b_abs"]["cluster"])
        self.assertFalse(labels["b_abs"]["answerable"])
        self.assertEqual(labels["b_abs"]["required_sessions"], [])
        self.assertEqual(corpus["batches"][0]["messages"][0]["content"], records[0]["haystack_sessions"][0][0]["content"])
        for q in corpus["questions"]:
            self.assertEqual(set(question_payload(q)), {"question", "question_date"})
            self.assertNotIn("GOLD_SECRET", str(question_payload(q)))
        self.assertLess(timestamp(records[0]["haystack_dates"][0]), timestamp(records[0]["question_date"]))
        with self.assertRaises(DrillError): timestamp("2023-01-01")
        with self.assertRaises(DrillError): adapt(records, ["a", "a"])


if __name__ == "__main__":
    unittest.main()
