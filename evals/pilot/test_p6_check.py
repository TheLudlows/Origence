import hashlib
import json
from pathlib import Path
import shutil
import tempfile
import unittest

from p6_check import check


class P6LinkageTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.parent = Path(tmp.name)
        sample = Path(__file__).parent / "examples" / "p6-synthetic"
        self.root = shutil.copytree(sample, self.parent / "package")
        self.link = self.root / "case-link.json"

    def edit_index(self, mutate):
        path = self.root / "index.json"
        index = json.loads(path.read_bytes())
        mutate(index)
        raw = json.dumps(index).encode()
        path.write_bytes(raw)
        self.edit_link(lambda link: link["evidence_index"].update(
            sha256=hashlib.sha256(raw).hexdigest()
        ))

    def edit_link(self, mutate):
        link = json.loads(self.link.read_bytes())
        mutate(link)
        self.link.write_text(json.dumps(link), encoding="utf-8")

    def test_public_example_reports_only_linkage(self):
        self.assertEqual(check(self.link), {
            "result": "INDEX_LINKAGE_ONLY", "public_synthetic": True,
            "files_checked": 1, "evidence_authenticity": "NOT_CHECKED",
            "pilot_acceptance": "NOT_CERTIFIED",
        })

    def test_cannot_promote_public_example(self):
        self.edit_link(lambda link: link.update(status="PASS"))
        with self.assertRaisesRegex(ValueError, "NOT_RUN"):
            check(self.link)

    def test_index_hash_is_of_raw_bytes(self):
        path = self.root / "index.json"
        path.write_bytes(path.read_bytes() + b" ")
        with self.assertRaisesRegex(ValueError, "index SHA-256"):
            check(self.link)

    def test_evidence_byte_count_and_hash(self):
        path = self.root / "basis.txt"
        raw = path.read_bytes()
        path.write_bytes(raw + b"x")
        with self.assertRaisesRegex(ValueError, "byte count"):
            check(self.link)
        path.write_bytes(b"x" * len(raw))
        with self.assertRaisesRegex(ValueError, "evidence SHA-256"):
            check(self.link)

    def test_prefix_mismatch_even_when_index_hash_matches(self):
        self.edit_index(lambda index: index["run_prefix"].update(through_sequence=3))
        with self.assertRaisesRegex(ValueError, "tuple mismatch"):
            check(self.link)

    def test_basis_must_be_unique_positive_in_range_non_bool(self):
        for basis in ([1, 1], [0], [3], [True], []):
            with self.subTest(basis=basis):
                self.edit_link(lambda link: link["run_prefix"].update(basis_sequences=basis))
                with self.assertRaisesRegex(ValueError, "basis sequences"):
                    check(self.link)

    def test_index_relative_paths(self):
        for ref in ("../index.json", "/index.json", "C:/index.json", r"..\index.json"):
            with self.subTest(ref=ref):
                self.edit_link(lambda link: link["evidence_index"].update(ref=ref))
                with self.assertRaises(ValueError):
                    check(self.link)

    def test_evidence_cannot_escape_package(self):
        outside = self.parent / "basis.txt"
        shutil.copyfile(self.root / "basis.txt", outside)
        self.edit_index(lambda index: index["files"][0].update(path="../basis.txt"))
        with self.assertRaises(ValueError):
            check(self.link)

    def test_symlink_to_matching_bytes_is_rejected(self):
        outside = self.parent / "basis.txt"
        local = self.root / "basis.txt"
        local.rename(outside)
        try:
            local.symlink_to(outside)
        except (NotImplementedError, OSError):
            self.skipTest("test user cannot create symlinks")
        with self.assertRaisesRegex(ValueError, "links"):
            check(self.link)

    def test_wrong_case_and_duplicate_evidence(self):
        self.edit_index(lambda index: index["files"][0].update(case_id="another"))
        with self.assertRaisesRegex(ValueError, "another case"):
            check(self.link)
        self.edit_index(lambda index: index["files"][0].update(case_id="public-synthetic-p6"))
        self.edit_index(lambda index: index["files"].append(index["files"][0]))
        with self.assertRaisesRegex(ValueError, "duplicate"):
            check(self.link)

    def test_rejects_duplicate_json_members(self):
        raw = self.link.read_bytes()
        self.link.write_bytes(b'{"status":"PASS",' + raw[1:])
        with self.assertRaisesRegex(ValueError, "duplicate JSON"):
            check(self.link)


if __name__ == "__main__":
    unittest.main()
