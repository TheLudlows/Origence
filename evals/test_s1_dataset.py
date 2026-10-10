"""Frozen dataset integrity and split separation, not semantic label certification."""
import hashlib
import json
from pathlib import Path
import unittest
from test_run import RUN


class S1Dataset(unittest.TestCase):
    def test_frozen_hashes_provenance_and_source_disjoint_holdout(self):
        root = Path(__file__).parent / 's1' / 'v2'
        manifest = json.loads((root / 'manifest.json').read_text(encoding='utf-8'))
        for name, digest in manifest['sha256'].items():
            self.assertEqual(hashlib.sha256((root / name).read_bytes()).hexdigest(), digest)
        corpus = RUN.read_jsonl(root / 'corpus.jsonl')
        development = RUN.read_jsonl(root / 'development.jsonl')
        heldout = RUN.read_jsonl(root / 'heldout.jsonl')
        all_cases = RUN.read_jsonl(root / 'cases.jsonl')
        self.assertEqual(all_cases, development + heldout)
        self.assertEqual((len(corpus), len(development), len(heldout)), (36, 200, 100))
        RUN.validate_evidence(corpus, all_cases, True)
        dev_sources = {s for c in development for s in c['relevant_source_ids']}
        heldout_sources = {s for c in heldout for s in c['relevant_source_ids']}
        self.assertFalse(dev_sources & heldout_sources)
        self.assertEqual(sum(not c['relevant_source_ids'] for c in all_cases), 12)
        # Preserve the original 100 questions, labels and evidence without edits.
        original = RUN.read_jsonl(root.parent / 'cases.jsonl')
        self.assertEqual([{k: v for k, v in c.items() if k not in ('split','authorship')}
                          for c in development[:100]], original)


if __name__ == '__main__':
    unittest.main()
