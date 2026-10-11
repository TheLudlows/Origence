import copy
import unittest
from aml_drill import DrillError
from aml_temporal import from_millis, payload, support_result


class TemporalTests(unittest.TestCase):
    def hit(self, timestamp):
        import json
        return {"id": "fixed", "content": "Source metadata: " + json.dumps({"timestamp": timestamp}) + "\n今天采摘。🙂"}

    def test_sidecar_preserves_evidence_and_signed_calendar_difference(self):
        hits = [self.hit(1681582260000), self.hit(1681840800000), self.hit(None)]
        before = copy.deepcopy(hits)
        result = payload("How many days?", "2023/04/18 (Tue) 01:48", hits, "offsets")
        self.assertEqual(hits, before)
        self.assertEqual([x["content"] for x in result["evidence"]], [x["content"] for x in hits])
        self.assertEqual([x["days"] for x in result["date_context"]["question_minus_message_calendar_days"]], [3, 0, None])
        self.assertEqual(result["date_context"]["message_pair_calendar_days"], [{"from_index": 0, "to_index": 1, "days": 3}])
        future = payload("q", "2023/04/14 (Fri) 00:00", hits[:1], "offsets")
        self.assertEqual(future["date_context"]["question_minus_message_calendar_days"][0]["days"], -1)
        self.assertNotIn("date_context", payload("q", "unchanged", hits, "raw"))
        self.assertNotIn("message_pair_calendar_days", payload("q", "2023/04/18 (Tue) 01:48", hits, "dates")["date_context"])

    def test_epoch_precision_leap_year_and_invalid_values(self):
        self.assertEqual(from_millis(-1).isoformat(), "1969-12-31T23:59:59.999000+00:00")
        self.assertEqual(from_millis(1709164800000).date().isoformat(), "2024-02-29")
        for value in [True, "1681582260000", 1.2, 2**63-1]:
            with self.assertRaises(DrillError):
                from_millis(value)
        with self.assertRaises(DrillError):
            payload("q", "bad-date", [], "dates")
        with self.assertRaises(DrillError):
            payload("q", "2023/04/18 (Tue) 01:48", [{"content": "not metadata"}], "dates")

    def test_rejects_invalid_judge_output(self):
        self.assertEqual(support_result({"state": "insufficient", "reason": "missing date"}), "insufficient")
        for output in [[], {}, {"state": "true", "reason": "x"}, {"state": "supported", "reason": None}]:
            with self.assertRaises(DrillError):
                support_result(output)


if __name__ == "__main__":
    unittest.main()
