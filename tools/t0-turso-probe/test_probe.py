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

    def test_narrow_schema_quote_candidate_rejects_semantic_change(self):
        class SchemaView:
            def __init__(self, db, mutation, object_name="oc_commands"):
                self.db, self.mutation, self.object_name = db, mutation, object_name

            def query(self, sql, params=()):
                rows = self.db.query(sql, params)
                if params == [self.object_name]:
                    return [[self.mutation(rows[0][0])]]
                return rows

        with tempfile.TemporaryDirectory() as tmp:
            db = probe.SQLite(Path(tmp) / "baseline.db")
            probe.initialize(db)
            quoted = SchemaView(db, lambda sql: sql.replace("key TEXT", '"key" TEXT').replace("operation, key)", 'operation, "key")'))
            with self.assertRaises(AssertionError):
                probe.schema_roundtrip(quoted)
            probe.schema_roundtrip(quoted, allow_known_key_quote=True)
            changed = SchemaView(db, lambda sql: sql.replace("request_hash TEXT NOT NULL", "request_hash TEXT"))
            with self.assertRaises(AssertionError):
                probe.schema_roundtrip(changed, allow_known_key_quote=True)
            changed_default = SchemaView(
                db, lambda sql: sql.replace("DEFAULT 'pending'", "DEFAULT 'PENDING'"),
                object_name="oc_jobs",
            )
            for allow_quote in [False, True]:
                with self.subTest(allow_quote=allow_quote), self.assertRaises(AssertionError):
                    probe.schema_roundtrip(changed_default, allow_known_key_quote=allow_quote)
            db.close()

    def test_sql_normalization_preserves_quoted_tokens(self):
        self.assertEqual(probe.normalize_sql("SELECT 'a b', \"key\";"),
                         probe.normalize_sql("select /* comment */ 'a b',\"key\""))
        for original, changed in [
            ("SELECT 'pending'", "SELECT 'PENDING'"),
            ("SELECT 'a b'", "SELECT 'ab'"),
            ("SELECT 'it''s A'", "SELECT 'it''s a'"),
            ('SELECT "a b"', 'SELECT "ab"'),
            ("SELECT 'a--b/*c*/'", "SELECT 'ab'"),
            ("SELECT 12", "SELECT 1 2"),
        ]:
            with self.subTest(original=original):
                self.assertNotEqual(probe.normalize_sql(original), probe.normalize_sql(changed))


if __name__ == "__main__":
    unittest.main()
