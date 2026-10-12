"""Cheap tests for the probe, deliberately separate from engine acceptance."""
import copy
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

    def test_explicit_schema_candidate_allows_only_known_display_changes(self):
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
            result = probe.schema_roundtrip(quoted, explicit_canonicalization=True)
            self.assertTrue(any(item["object"] == "oc_commands"
                                and "known_identifier_quote:key" in item["engine_adjustments"]
                                for item in result["canonicalization_adjustments"]))
            quoted_action = SchemaView(db, lambda sql: sql.replace("action TEXT", '"action" TEXT'),
                                      object_name="oc_audit")
            with self.assertRaises(AssertionError):
                probe.schema_roundtrip(quoted_action)
            probe.schema_roundtrip(quoted_action, explicit_canonicalization=True)
            guarded_index = SchemaView(db, lambda sql: sql.replace("CREATE INDEX ", "CREATE INDEX IF NOT EXISTS ", 1),
                                       object_name="oc_graph_entities_scope")
            with self.assertRaises(AssertionError):
                probe.schema_roundtrip(guarded_index)
            probe.schema_roundtrip(guarded_index, explicit_canonicalization=True)
            unknown_quote = SchemaView(db, lambda sql: sql.replace("request_hash TEXT", '"request_hash" TEXT'))
            with self.assertRaises(AssertionError):
                probe.schema_roundtrip(unknown_quote, explicit_canonicalization=True)
            changed = SchemaView(db, lambda sql: sql.replace("request_hash TEXT NOT NULL", "request_hash TEXT"))
            with self.assertRaises(AssertionError):
                probe.schema_roundtrip(changed, explicit_canonicalization=True)
            changed_default = SchemaView(
                db, lambda sql: sql.replace("DEFAULT 'pending'", "DEFAULT 'PENDING'"),
                object_name="oc_jobs",
            )
            for canonical in [False, True]:
                with self.subTest(canonical=canonical), self.assertRaises(AssertionError):
                    probe.schema_roundtrip(changed_default, explicit_canonicalization=canonical)
            db.close()

    def test_schema_canonicalization_preserves_guard_text_inside_literals(self):
        tokens, changes = probe.canonical_schema_tokens(
            "CREATE INDEX IF NOT EXISTS x ON t(a) WHERE a='IF NOT EXISTS key action'", "x")
        self.assertEqual(changes, ["create_prefix_if_not_exists"])
        self.assertIn("'IF NOT EXISTS key action'", tokens)
        self.assertNotIn("exists", tokens)
        quoted, changes = probe.canonical_schema_tokens('CREATE TABLE t("key" TEXT)', "t")
        self.assertIn('"key"', quoted)
        self.assertEqual(changes, [])

    def test_schema_metadata_ignores_only_index_display_order(self):
        with tempfile.TemporaryDirectory() as tmp:
            db = probe.SQLite(Path(tmp) / "baseline.db")
            probe.initialize(db)
            reference = probe.schema_metadata(db)
            db.close()
        reordered = copy.deepcopy(reference)
        indexes = reordered["tables"]["oc_chunks"]["indexes"]
        self.assertGreater(len(indexes), 1)
        indexes.reverse()
        for sequence, row in enumerate(indexes):
            row[0] = sequence + 50
        result = probe.compare_schema_metadata(reference, reordered)
        self.assertFalse(result["raw_metadata_identical"])
        self.assertEqual([item["table"] for item in result["index_list_display_changes"]], ["oc_chunks"])

        missing = copy.deepcopy(reordered)
        missing["tables"]["oc_chunks"]["indexes"].pop()
        with self.assertRaises(AssertionError):
            probe.compare_schema_metadata(reference, missing)
        for field in [1, 2, 3, 4]:
            changed = copy.deepcopy(reordered)
            row = changed["tables"]["oc_chunks"]["indexes"][0]
            row[field] = f"changed-{row[field]}" if isinstance(row[field], str) else 1 - row[field]
            with self.subTest(index_field=field), self.assertRaises(AssertionError):
                probe.compare_schema_metadata(reference, changed)
        changed_sql = copy.deepcopy(reordered)
        for row in changed_sql["objects"]:
            if row[1] == "oc_jobs":
                row[3] = row[3].replace("DEFAULT 'pending'", "DEFAULT 'PENDING'")
        with self.assertRaises(AssertionError):
            probe.compare_schema_metadata(reference, changed_sql)

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
