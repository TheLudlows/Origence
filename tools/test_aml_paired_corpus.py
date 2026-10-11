"""Frozen paired data integrity and hard-case construction checks."""
from collections import Counter
import hashlib
import json
from pathlib import Path
import unittest
from aml_diagnose import oracle_hits
from aml_quality import decode_hits

ROOT = Path(__file__).resolve().parents[1] / "evals/aml/v4"


class PairedCorpusTests(unittest.TestCase):
    def test_frozen_hashes_and_labels(self):
        freeze = json.loads((ROOT / "freeze.json").read_text(encoding="utf-8"))
        for field, name in [("corpus_sha256", "corpus.json"), ("rubrics_sha256", "rubrics.json")]:
            self.assertEqual(hashlib.sha256((ROOT / name).read_bytes()).hexdigest(), freeze[field])
        corpus = json.loads((ROOT / "corpus.json").read_text(encoding="utf-8"))
        refs = json.loads((ROOT / "rubrics.json").read_text(encoding="utf-8"))["questions"]
        self.assertEqual(len(corpus["questions"]), 32)
        self.assertEqual(len({q["user_id"] for q in corpus["questions"]}), 32)
        self.assertEqual(set(refs), {q["id"] for q in corpus["questions"]})
        for q in corpus["questions"]:
            self.assertEqual(bool(q["required"]), refs[q["id"]]["answerable"])
            decode_hits(oracle_hits(q, corpus), corpus["batches"], q["user_id"])

    def test_pair_differs_by_exactly_one_required_fact(self):
        corpus = json.loads((ROOT / "corpus.json").read_text(encoding="utf-8"))
        pairs = {}
        for q in corpus["questions"]:
            pairs.setdefault(q["pair_id"], []).append(q)
        for pair in pairs.values():
            self.assertEqual(len(pair), 2)
            positive = next(q for q in pair if q["required"])
            negative = next(q for q in pair if not q["required"])
            self.assertEqual(positive["query"], negative["query"])
            self.assertEqual(positive["split"], negative["split"])
            messages = lambda q: Counter(m["content"] for b in corpus["batches"] if b["user_id"] == q["user_id"] for m in b["messages"])
            added, removed = messages(positive) - messages(negative), messages(negative) - messages(positive)
            self.assertEqual(sum(added.values()), 1)
            self.assertFalse(removed)
            evidence = [h["content"].partition("\n")[2] for h in oracle_hits(positive, corpus)]
            self.assertIn(next(iter(added)), evidence)

    def test_backfilled_history_is_later_than_current_fact(self):
        corpus = json.loads((ROOT / "corpus.json").read_text(encoding="utf-8"))
        for q in corpus["questions"]:
            if q["category"] != "event_time" or not q["required"]:
                continue
            messages = [m for b in corpus["batches"] if b["user_id"] == q["user_id"] for m in b["messages"]]
            backfill = next(m for m in messages if "补录历史" in m["content"] or "补登记" in m["content"])
            current = oracle_hits(q, corpus)[0]
            timestamp = json.loads(current["content"].partition("\n")[0].removeprefix("Source metadata: "))["timestamp"]
            self.assertGreater(backfill["timestamp"], timestamp)


if __name__ == "__main__":
    unittest.main()
