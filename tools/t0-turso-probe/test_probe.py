"""Cheap tests for the probe, deliberately separate from engine acceptance."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("t0_verify", Path(__file__).with_name("verify.py"))
probe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(probe)


class ProbeTests(unittest.TestCase):
    def test_real_ddl_fixture_and_relational_assertions(self):
        failures = []
        with tempfile.TemporaryDirectory() as tmp:
            db = probe.SQLite(Path(tmp) / "baseline.db")
            self.assertEqual(probe.initialize(db), 47)
            probe.seed(db)

            def record(name, fn):
                try:
                    fn()
                except Exception as error:
                    failures.append((name, str(error)))
                    db.raw("ROLLBACK", op="execute")

            probe.relational_cases(db, record)
            db.close()
        self.assertEqual(failures, [])

    def test_literal_query_is_and_deduplicated_and_escaped(self):
        self.assertEqual(probe.literal_fts_query("北京 Rust Rust"), '"北京" AND "Rust"')
        self.assertEqual(probe.literal_fts_query("AND OR NOT"), '"AND" AND "OR" AND "NOT"')
        self.assertEqual(probe.literal_fts_query('foo"bar path\\part'), '"foo\\"bar" AND "path\\\\part"')
        self.assertIsNone(probe.literal_fts_query(" \n\t"))

    def test_trigger_statement_is_not_split_at_internal_semicolon(self):
        text = (probe.ROOT / "src/storage/sqlite-schema.sql").read_text()
        parsed = list(probe.statements(text))
        triggers = [part for part in parsed if "CREATE TRIGGER" in part]
        self.assertEqual(len(triggers), 2)
        self.assertTrue(all(part.rstrip().endswith("END;") for part in triggers))

    def test_constraint_gate_rejects_unrelated_engine_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            db = probe.SQLite(Path(tmp) / "baseline.db")
            probe.initialize(db)
            probe.seed(db)
            with self.assertRaises(AssertionError):
                probe.expected_error(db, "this is invalid SQL")
            db.close()

    def test_immutable_owner_probe_detects_missing_trigger(self):
        failures = []
        with tempfile.TemporaryDirectory() as tmp:
            db = probe.SQLite(Path(tmp) / "mutant.db")
            probe.initialize(db)
            probe.seed(db)
            db.execute("DROP TRIGGER oc_workspace_owner_immutable")

            def record(name, fn):
                try:
                    fn()
                except Exception:
                    failures.append(name)
                    db.raw("ROLLBACK", op="execute")

            probe.relational_cases(db, record)
            db.close()
        self.assertIn("owner_immutable_trigger", failures)


if __name__ == "__main__":
    unittest.main()
