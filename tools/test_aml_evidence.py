import copy
import json
import unittest
from aml_drill import DrillError
from aml_evidence import source_index, decode_fragments, complete_messages, coverage


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.message = {"role": "user", "timestamp": 123, "content": "中🙂é\n" * 800}
        self.batches = [{"user_id": "u", "session_id": "s", "messages": [self.message]}]
        self.known = source_index(self.batches, "u")
        self.hits = []
        raw = self.message["content"].encode()
        pos = 0
        while pos < len(raw):
            end = min(pos + 2400, len(raw))
            while True:
                try:
                    content = raw[pos:end].decode()
                    break
                except UnicodeDecodeError:
                    end -= 1
            meta = {"role": "user", "timestamp": 123, "session_id": "s", "message_index": 0,
                    "message_count": 1, "source_path": "/messages/0/content", "parser": "aml-message-ranges-v1",
                    "byte_basis": "message_content_utf8", "byte_start": pos, "byte_end": end}
            self.hits.append({"id": str(pos), "content": "Source metadata: " + json.dumps(meta) + "\n" + content})
            pos = end

    def test_multilingual_ranges_and_partial_coverage(self):
        parts = decode_fragments(self.hits, self.known)
        self.assertEqual(complete_messages(parts), {("s", 0)})
        self.assertEqual(complete_messages(parts[:1]), set())
        self.assertFalse(coverage(parts[:1], required_messages=[("s", 0)])["all_required_messages"])
        self.assertEqual(coverage(parts[:1], required_sessions=["s"])["session_recall"], 1)
        self.assertIsNone(coverage(parts, required_sessions=["s"])["complete_evidence_recall"])

    def test_tampering_scope_parser_and_utf8_boundaries(self):
        for key, value in [("byte_start", 1), ("byte_end", True), ("message_index", True),
                           ("parser", "future-v2"), ("session_id", "foreign"), ("source_path", "/other"),
                           ("message_count", 2), ("timestamp", 456), ("role", "assistant")]:
            hits = copy.deepcopy(self.hits)
            header, content = hits[0]["content"].split("\n", 1)
            meta = json.loads(header.removeprefix("Source metadata: "))
            meta[key] = value
            hits[0]["content"] = "Source metadata: " + json.dumps(meta) + "\n" + content
            with self.assertRaises(DrillError): decode_fragments(hits, self.known)
        with self.assertRaises(DrillError): decode_fragments(self.hits, source_index(self.batches, "other"))
        with self.assertRaises(DrillError): decode_fragments(self.hits + self.hits[:1], self.known)
        with self.assertRaises(DrillError): source_index(self.batches * 2, "u")

    def test_missing_middle_never_counts_complete(self):
        parts = decode_fragments(self.hits, self.known)
        self.assertGreaterEqual(len(parts), 3)
        self.assertFalse(complete_messages(parts[:1] + parts[-1:]))


if __name__ == "__main__":
    unittest.main()
