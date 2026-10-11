"""Cross-encoder boundary tests run without model dependencies."""
import hashlib
from pathlib import Path
import tempfile
import unittest
from aml_cross_encoder import MODEL_FILES, MODEL_ID, REVISION, check_lengths, rank_indices, verify_model
from aml_drill import DrillError
from aml_cross_encoder_answer import selected_candidates


class CrossEncoderTests(unittest.TestCase):
    def test_answer_evidence_rejects_rebound_ids_and_tampered_ranks(self):
        hits = [{"id": "a", "content": "original a"}, {"id": "b", "content": "original b"}]
        row = {"candidate_ids": ["a", "b"], "scores": [1.0, 2.0], "ranked_indices": {"40": [1, 0]}}
        self.assertEqual(selected_candidates(row, hits, 40), [hits[1], hits[0]])
        for bad in [{**row, "candidate_ids": ["a", "foreign"]}, {**row, "ranked_indices": {"40": [0, 1]}}]:
            with self.assertRaises(DrillError):
                selected_candidates(bad, hits, 40)

    def test_depth_cutoff_is_applied_before_ranking(self):
        scores = [0.0] * 40 + [9.0]
        self.assertEqual(rank_indices(scores, 40), list(range(40)))
        self.assertEqual(rank_indices(scores, 100)[0], 40)
        for values in [[float("nan")], [float("inf")], [True], [], [0.0] * 101]:
            with self.assertRaises(DrillError):
                rank_indices(values, 100)

    def test_long_inputs_fail_instead_of_silent_truncation(self):
        check_lengths([1, 512])
        for lengths in [[], [0], [513], [True]]:
            with self.assertRaises(DrillError):
                check_lengths(lengths)

    def test_model_files_are_whitelisted_and_hash_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            files = []
            for name in sorted(MODEL_FILES):
                (root / name).write_bytes(b"fixture")
                files.append({"name": name, "bytes": 7, "sha256": hashlib.sha256(b"fixture").hexdigest()})
            manifest = {"model": MODEL_ID, "revision": REVISION, "files": files}
            verify_model(root, manifest)
            with self.assertRaises(DrillError):
                verify_model(root, {**manifest, "revision": "main"})
            with self.assertRaises(DrillError):
                verify_model(root, {**manifest, "files": [{**files[0], "name": "../outside"}] + files[1:]})
            (root / "config.json").write_bytes(b"changed")
            with self.assertRaisesRegex(DrillError, "model_integrity_mismatch"):
                verify_model(root, manifest)


if __name__ == "__main__":
    unittest.main()
