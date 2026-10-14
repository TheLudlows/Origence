import hashlib
import unittest
from pathlib import Path
from unittest.mock import patch

import extract_sql as e


class ReceiptExtractionTests(unittest.TestCase):
    def test_unreviewed_source_hash_rejected_even_when_self_consistent(self):
        source = 'another algorithm'
        with self.assertRaisesRegex(ValueError, 'unreviewed receipt_metadata'):
            e.receipt_variant('candidate.rs', source, 'a' * 40, e.digest(source.encode()))

    def test_receipt_requires_full_commit(self):
        source = 'fixture'
        digest = e.digest(source.encode())
        with patch.object(e, 'RECEIPT_V1_HASH', digest):
            with self.assertRaisesRegex(ValueError, 'exact source commit'):
                e.receipt_variant('candidate.rs', source, 'branch-name', digest)

    def test_commit_label_must_equal_real_checkout(self):
        with patch.object(e.subprocess, 'check_output', return_value='b' * 40):
            with self.assertRaisesRegex(ValueError, 'checkout commit differs'):
                e.verify_candidate_commit('/fixture', 'a' * 40, '0' * 64)

    def test_commit_bytes_must_equal_reviewed_source(self):
        with patch.object(e.subprocess, 'check_output', side_effect=['a' * 40, b'changed']):
            with self.assertRaisesRegex(ValueError, 'committed source'):
                e.verify_candidate_commit('/fixture', 'a' * 40, '0' * 64)

    def test_exact_commit_and_source_are_accepted(self):
        source = b'committed source'
        with patch.object(e.subprocess, 'check_output', side_effect=['a' * 40 + '\n', source]):
            e.verify_candidate_commit('/fixture', 'a' * 40, hashlib.sha256(source).hexdigest())

    def test_boundaries_fail_closed(self):
        with self.assertRaisesRegex(ValueError, 'boundary'):
            e.section('first first last', 'first', 'last')
        with self.assertRaisesRegex(ValueError, 'boundary'):
            e.section('first', 'first', 'last')

    def test_typed_copy_uses_required_option_deserializer(self):
        source = (Path(__file__).parent / 'src/receipt_types.rs').read_text()
        self.assertIn('#[serde(deserialize_with = "receipt_dimension")]', source)
        self.assertIn('#[serde(deny_unknown_fields)]', source)
        self.assertIn('fn ids_json(&self)', source)


if __name__ == '__main__':
    unittest.main()
