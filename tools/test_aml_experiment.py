"""Boundaries of model-generated queries and evidence selection."""
import unittest
from unittest.mock import patch
from aml_drill import DrillError
from aml_experiment import Chat, PLAN, balanced_pool, evidence_payload, fuse, parse_queries, select


class ExperimentTests(unittest.TestCase):
    def test_expansion_limits_and_duplicate_queries(self):
        self.assertEqual(parse_queries({"queries": [" x ", "x", "original"]}, "original"), ["x"])
        for queries in [None, ["a"] * 4, [""], ["x" * 1001], [True], ["a\0b"]]:
            with self.assertRaises(DrillError):
                parse_queries({"queries": queries}, "original")

    def test_reranker_can_only_select_distinct_original_evidence(self):
        candidates = [{"id": "a", "content": "原文"}, {"id": "b", "content": "evidence"}]
        self.assertIs(select({"indices": [1]}, candidates)[0], candidates[1])
        self.assertEqual(select({"indices": []}, candidates), [])
        self.assertEqual(select([1], candidates), [candidates[1]])
        self.assertEqual(select([{"index": 1}], candidates), [candidates[1]])
        self.assertEqual(select([{ "index": 1, "content": "evidence"}], candidates), [candidates[1]])
        with self.assertRaisesRegex(DrillError, "modified_selection_evidence"):
            select([{ "index": 1, "content": "rewritten"}], candidates)
        for indices in [[1, 1], [-1], [2], [True], ["a"], None, [0] * 6]:
            with self.assertRaises(DrillError):
                select({"indices": indices}, candidates)
        self.assertEqual(evidence_payload(candidates), [{"index": 0, "content": "原文"}, {"index": 1, "content": "evidence"}])

    def test_fusion_deduplicates_and_rejects_changed_source(self):
        a, b = {"id": "a", "content": "a"}, {"id": "b", "content": "b"}
        self.assertEqual(fuse([[a, a, b], [b]]), [b, a])
        with self.assertRaises(DrillError):
            fuse([[a], [{"id": "a", "content": "changed"}]])

    def test_balanced_candidates_preserve_each_query_and_original_objects(self):
        a, b, c = ({"id": str(i), "content": str(i)} for i in range(3))
        self.assertEqual(balanced_pool([[a, b], [c, a]], 2), [a, c])
        self.assertEqual(balanced_pool([[a, b], [a, c]], 3), [a, b, c])
        with self.assertRaises(DrillError):
            balanced_pool([[a], [{"id": "0", "content": "changed"}]])

    def test_chat_budgets_and_invalid_output_fail_closed(self):
        env = {"AML_CHAT_BASE_URL": "https://example.invalid/v1", "AML_CHAT_API_KEY": "secret", "AML_CHAT_MODEL": "fixture"}
        report = {}
        chat = Chat(env, report)
        self.assertNotIn("secret", str(report))
        with patch("aml_experiment.request") as request:
            with self.assertRaisesRegex(DrillError, "chat_budget_exceeded"):
                chat.call("plan", "q", PLAN, {"question": "x" * 40000})
            request.assert_not_called()
        for result in [{"choices": [{"finish_reason": "length", "message": {"content": "{}"}}]},
                       {"choices": [{"finish_reason": "stop", "message": {"content": "private malformed text"}}]}]:
            with patch("aml_experiment.request", return_value=result):
                with self.assertRaises(DrillError) as raised:
                    chat.call("plan", "q", PLAN, {"question": "test"})
                self.assertNotIn("private", str(raised.exception))
        with patch("aml_experiment.request", return_value={"choices": [{"finish_reason": "stop", "message": {"content": '[{"indices":[0]}]'}}]}):
            self.assertEqual(chat.call("rank", "q", PLAN, {}), {"indices": [0]})
        chat.calls[:] = [{}] * 60
        with patch("aml_experiment.request") as request:
            with self.assertRaisesRegex(DrillError, "chat_budget_exceeded"):
                chat.call("plan", "q", PLAN, {})
            request.assert_not_called()

    def test_rejects_chat_credentials_in_url(self):
        for base in ["http://external.invalid", "https://u:p@example.invalid", "https://example.invalid?key=secret"]:
            with self.assertRaises(DrillError):
                Chat({"AML_CHAT_BASE_URL": base, "AML_CHAT_API_KEY": "secret", "AML_CHAT_MODEL": "fixture"}, {})


if __name__ == "__main__":
    unittest.main()
