"""Archive byte integrity, dependency closure, and strict source provenance."""
import ast
import hashlib
import importlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import aml_artifacts
from aml_drill import DrillError

ROOT = Path(__file__).resolve().parents[1]
ARCHIVE = ROOT / "archives/aml-research"
REMOVED = {
    "aml_budget_eval", "aml_temporal_eval", "aml_maintenance_scale", "aml_oracle",
    "aml_cross_encoder_answer", "aml_compaction_profile", "aml_search_profile",
    "aml_vector_profile",
}


class ArchiveTests(unittest.TestCase):
    def test_original_sources_are_complete_and_byte_identical_to_manifest(self):
        manifest = json.loads((ARCHIVE / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["source_commit"], "44b74835928321482ca62fac0d16c716e0bb732b")
        entries = {entry["archive_path"]: entry for entry in manifest["files"]}
        self.assertEqual(len(entries), len(manifest["files"]))
        self.assertEqual(set(entries), {p.relative_to(ARCHIVE).as_posix() for p in (ARCHIVE / "tools").iterdir() if p.is_file()})
        for name, entry in entries.items():
            with self.subTest(path=name):
                self.assertEqual(name, entry["original_path"])
                data = (ARCHIVE / name).read_bytes()
                self.assertEqual(len(data), entry["bytes"])
                self.assertEqual(hashlib.sha256(data).hexdigest(), entry["sha256"])
                blob = b"blob " + str(len(data)).encode() + b"\0" + data
                self.assertEqual(hashlib.sha1(blob).hexdigest(), entry["git_blob_sha1"])
        self.assertIn("tools/requirements-aml-rerank.txt", entries)
        self.assertFalse((ROOT / "tools/requirements-aml-rerank.txt").exists())

    def test_archived_local_imports_form_a_complete_dependency_group(self):
        manifest = json.loads((ARCHIVE / "manifest.json").read_text(encoding="utf-8"))
        entries = {entry["archive_path"]: entry for entry in manifest["files"]}
        for name, entry in entries.items():
            if not name.endswith(".py"):
                continue
            imports = set()
            for node in ast.walk(ast.parse((ARCHIVE / name).read_text(encoding="utf-8"))):
                if isinstance(node, ast.ImportFrom) and node.module and node.module.startswith("aml_"):
                    imports.add("tools/" + node.module + ".py")
                if isinstance(node, ast.Import):
                    imports.update("tools/" + alias.name + ".py" for alias in node.names if alias.name.startswith("aml_"))
            with self.subTest(path=name):
                self.assertEqual(imports, set(entry["local_dependencies"]))
                self.assertLessEqual(imports, entries.keys())

    def test_active_helpers_do_not_expose_research_execution(self):
        for name in ("aml_answer", "aml_quality", "aml_experiment", "aml_cross_encoder", "aml_artifacts"):
            module = importlib.import_module(name)
            with self.subTest(module=name):
                self.assertEqual(Path(module.__file__).resolve().parent, ROOT / "tools")
                for entrypoint in ("main", "evaluate", "rerank", "Chat", "LocalReranker"):
                    self.assertFalse(hasattr(module, entrypoint), entrypoint)
        for name in REMOVED:
            self.assertFalse((ROOT / "tools" / (name + ".py")).exists())
            self.assertTrue((ARCHIVE / "tools" / (name + ".py")).is_file())

    def test_active_aml_imports_do_not_reenter_archived_runners(self):
        for path in (ROOT / "tools").glob("aml_*.py"):
            for node in ast.walk(ast.parse(path.read_text(encoding="utf-8"))):
                if isinstance(node, ast.ImportFrom):
                    self.assertNotIn(node.module, REMOVED, str(path))
                elif isinstance(node, ast.Import):
                    for alias in node.names:
                        self.assertNotIn(alias.name, REMOVED, str(path))


class FrozenSourceTests(unittest.TestCase):
    def test_default_source_is_archived_original_not_current_helper(self):
        self.assertEqual(aml_artifacts.source_path("tools/aml_quality.py"), ARCHIVE / "tools/aml_quality.py")
        expected = hashlib.sha256((ARCHIVE / "tools/aml_quality.py").read_bytes()).hexdigest()
        aml_artifacts.verify_frozen_files({"tools/aml_quality.py": expected})
        with self.assertRaisesRegex(DrillError, "frozen_input_changed"):
            aml_artifacts.verify_frozen_files({"tools/aml_quality.py": expected}, ROOT)

    def test_explicit_source_root_never_falls_back_for_missing_or_changed_file(self):
        expected = hashlib.sha256((ARCHIVE / "tools/aml_quality.py").read_bytes()).hexdigest()
        with tempfile.TemporaryDirectory() as folder:
            source = Path(folder)
            with self.assertRaisesRegex(DrillError, "frozen_source_missing"):
                aml_artifacts.verify_frozen_files({"tools/aml_quality.py": expected}, source)
            (source / "tools").mkdir()
            (source / "tools/aml_quality.py").write_text("changed\n", encoding="utf-8")
            with self.assertRaisesRegex(DrillError, "frozen_input_changed"):
                aml_artifacts.verify_frozen_files({"tools/aml_quality.py": expected}, source)
            (source / "tools/aml_quality.py").write_bytes((ARCHIVE / "tools/aml_quality.py").read_bytes())
            aml_artifacts.verify_frozen_files({"tools/aml_quality.py": expected}, source)

    def test_source_override_does_not_replace_current_frozen_data(self):
        with tempfile.TemporaryDirectory() as folder:
            root, source = Path(folder) / "current", Path(folder) / "old"
            for directory in (root, source):
                (directory / "evals").mkdir(parents=True)
                (directory / "tools").mkdir()
            data, code = b"current frozen evidence", b"historical frozen source"
            (root / "evals/fixture.json").write_bytes(data)
            (source / "evals/fixture.json").write_bytes(b"different old evidence")
            (source / "tools/fixture.py").write_bytes(code)
            files = {"evals/fixture.json": hashlib.sha256(data).hexdigest(),
                     "tools/fixture.py": hashlib.sha256(code).hexdigest()}
            with patch.object(aml_artifacts, "ROOT", root):
                aml_artifacts.verify_frozen_files(files, source)
                (root / "evals/fixture.json").write_bytes(b"tampered")
                with self.assertRaisesRegex(DrillError, "frozen_input_changed"):
                    aml_artifacts.verify_frozen_files(files, source)

    def test_budget_freeze_checks_exact_source_bytes_without_rewriting_freeze(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "tools").mkdir()
            data = b"# frozen historical runner\n"
            (root / "tools/fixture.py").write_bytes(data)
            frozen = {"files": {"tools/fixture.py": hashlib.sha256(data).hexdigest()}}
            freeze_path = root / "freeze.json"
            freeze_path.write_text(json.dumps(frozen), encoding="utf-8")
            original = freeze_path.read_bytes()
            with patch.object(aml_artifacts, "OUT", root):
                self.assertEqual(aml_artifacts.verify_freeze(root), frozen)
                (root / "tools/fixture.py").write_bytes(data + b"# modified\n")
                with self.assertRaisesRegex(DrillError, "frozen_input_changed"):
                    aml_artifacts.verify_freeze(root)
            self.assertEqual(freeze_path.read_bytes(), original)

    def test_out_of_tree_source_paths_fail_closed(self):
        for name in ("/tools/example.py", "../tools/example.py", "tools/../../example.py", "tools\\example.py", "evals/example.json"):
            with self.subTest(path=name), self.assertRaisesRegex(DrillError, "invalid_frozen_source_path"):
                aml_artifacts.source_path(name)
        for name in ("/example.json", "../example.json", "tools/../example.py", "tools\\example.py"):
            with self.subTest(path=name), self.assertRaisesRegex(DrillError, "invalid_frozen_input_path"):
                aml_artifacts.verify_frozen_files({name: "unused"})


if __name__ == "__main__":
    unittest.main()
