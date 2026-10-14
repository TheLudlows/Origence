"""Run: python3 -B -m unittest discover -s tools -p 'test_check_no_join_sql.py'."""
import contextlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import check_no_join_sql as guard


class SqlRules(unittest.TestCase):
    def assert_bad(self, sql):
        self.assertTrue(guard.sql_issues(sql), sql)

    def test_join_spellings_and_self_join(self):
        for join in ("JOIN", "LEFT JOIN", "LEFT OUTER JOIN", "NATURAL JOIN", "CROSS JOIN", "INNER JOIN"):
            with self.subTest(join=join):
                self.assert_bad(f'SELECT * FROM a x {join} a y ON x.id=y.id')

    def test_implicit_from_and_nested_relations(self):
        for sql in (
            "SELECT * FROM a,b WHERE a.id=b.id",
            "SELECT EXISTS(SELECT 1 FROM a, b)",
            "SELECT * FROM a WHERE EXISTS(SELECT 1 FROM b WHERE b.id=a.id)",
            "UPDATE a SET x=(SELECT x FROM b)",
            "DELETE FROM a WHERE id IN (SELECT id FROM b)",
            "UPDATE a SET x=b.x FROM b WHERE a.id=b.id",
            "SELECT * FROM a WHERE id IN (SELECT id FROM a)",
            "INSERT INTO a SELECT * FROM a",
            "SELECT * FROM a WHERE EXISTS(SELECT 1 FROM json_each(a.data))",
            'SELECT * FROM "a", `b`',
            "WITH x AS (SELECT * FROM a), y AS (SELECT * FROM b) SELECT * FROM x WHERE EXISTS(SELECT 1 FROM y)",
            "WITH x AS (SELECT * FROM a) SELECT * FROM x WHERE EXISTS(SELECT 1 FROM x)",
        ):
            with self.subTest(sql=sql): self.assert_bad(sql)

    def test_parenthesized_table_groups_fail_closed(self):
        for sql in (
            "SELECT * FROM (a,b) WHERE a.id=b.id",
            "SELECT * FROM ((a,b))",
            "SELECT * FROM (a)",
            "SELECT * FROM (main.a, main.b)",
        ):
            with self.subTest(sql=sql), self.assertRaises(guard.Unknown):
                guard.sql_issues(sql)
        self.assertEqual(guard.sql_issues("SELECT * FROM (SELECT id FROM a)"), set())

    def test_quotes_comments_commas_and_single_relation_are_not_joins(self):
        for sql in (
            "SELECT 'JOIN FROM a,b', printf('%s,%s',x,y) FROM a /* JOIN b */ -- JOIN c\nWHERE id=?",
            'SELECT "join", "from" FROM "table,with,commas"',
            "SELECT json_extract(x,'$.join'),id FROM a ORDER BY x,id",
            "SELECT * FROM json_each('[1,2]')",
            "SELECT EXISTS(SELECT 1 FROM sqlite_master)",
            "WITH x AS (SELECT * FROM a) SELECT * FROM x",
            "SELECT * FROM a; SELECT * FROM b;",
            "CREATE TABLE a(id INT REFERENCES b(id), value TEXT DEFAULT 'JOIN');",
            "CREATE TRIGGER x BEFORE INSERT ON a BEGIN SELECT RAISE(ABORT,'JOIN b'); END;",
            "UPDATE a SET x=?,y=? WHERE id=?",
        ):
            with self.subTest(sql=sql): self.assertEqual(guard.sql_issues(sql), set())

    def test_trigger_new_old_are_scalars_not_extra_relations(self):
        for sql in (
            "CREATE TRIGGER x BEFORE INSERT ON a WHEN EXISTS(SELECT 1 FROM a WHERE id=NEW.id) BEGIN SELECT RAISE(ABORT,'x'); END;",
            "CREATE TRIGGER x BEFORE INSERT ON a WHEN EXISTS(SELECT 1 FROM b WHERE id=NEW.id) BEGIN SELECT RAISE(ABORT,'x'); END;",
            "CREATE TRIGGER x AFTER UPDATE ON a BEGIN SELECT 1 FROM a WHERE id=OLD.id OR id=NEW.id; END;",
            "CREATE TRIGGER x BEFORE DELETE ON a BEGIN SELECT 1 FROM b WHERE id=OLD.id; END;",
            "CREATE TRIGGER x INSTEAD OF UPDATE ON a BEGIN UPDATE b SET value=NEW.value WHERE id=OLD.id; END;",
            "CREATE TRIGGER x UPDATE OF value ON a BEGIN INSERT INTO b VALUES(NEW.value); END;",
            "CREATE TEMP TRIGGER x AFTER UPDATE ON a WHEN EXISTS(SELECT 1 FROM b WHERE id=NEW.id) BEGIN UPDATE c SET value=NEW.value WHERE id=OLD.id; DELETE FROM d WHERE id=OLD.id; END;",
            "CREATE TRIGGER x AFTER INSERT ON a BEGIN SELECT 'JOIN FROM a,b NEW.id'; SELECT 1 FROM b WHERE id=NEW.id; END; SELECT 1 FROM c;",
        ):
            with self.subTest(sql=sql): self.assertEqual(guard.sql_issues(sql), set())

    def test_trigger_actual_associations_are_still_rejected(self):
        for sql in (
            "CREATE TRIGGER x BEFORE INSERT ON a WHEN EXISTS(SELECT 1 FROM a JOIN b ON a.id=b.id WHERE a.id=NEW.id) BEGIN SELECT 1; END;",
            "CREATE TRIGGER x BEFORE INSERT ON a BEGIN SELECT 1 FROM a,b WHERE a.id=NEW.id; END;",
            "CREATE TRIGGER x AFTER UPDATE ON a BEGIN SELECT 1 FROM b WHERE id=NEW.id AND EXISTS(SELECT 1 FROM c WHERE c.id=b.id); END;",
            "CREATE TRIGGER x AFTER UPDATE ON a BEGIN SELECT 1 FROM b WHERE EXISTS(SELECT 1 FROM b x WHERE x.id=b.id); END;",
            "CREATE TRIGGER x AFTER INSERT ON a BEGIN UPDATE b SET value=(SELECT value FROM c WHERE id=NEW.id); END;",
            "CREATE TRIGGER x AFTER INSERT ON a BEGIN INSERT INTO b SELECT * FROM c WHERE id=NEW.id; END;",
            "CREATE TRIGGER x AFTER INSERT ON a BEGIN SELECT 1; DELETE FROM b WHERE id IN (SELECT id FROM c); END;",
            "CREATE TRIGGER x AFTER INSERT ON a WHEN EXISTS(SELECT 1 FROM b, c) BEGIN SELECT 1; END;",
        ):
            with self.subTest(sql=sql): self.assert_bad(sql)

    def test_unrecognized_trigger_body_does_not_pass(self):
        with self.assertRaises(guard.Unknown):
            guard.sql_issues("CREATE TRIGGER x BEFORE INSERT ON a SELECT 1 FROM b;")

    def test_malformed_lexical_input_is_unknown(self):
        for sql in ("SELECT 'unterminated", "SELECT /* broken", "SELECT (1", "SELECT )"):
            with self.subTest(sql=sql), self.assertRaises(guard.Unknown):
                guard.sql_issues(sql)


class RustInputs(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / "src").mkdir()

    def tearDown(self):
        self.tmp.cleanup()

    def put(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    def scan(self, source, selected=None):
        self.put("src/lib.rs", source)
        return guard.audit(self.root, selected)

    def test_non_sql_comments_paths_macros_and_literals(self):
        findings = self.scan(r'''// db::query("SELECT * FROM a JOIN b");
            /* outer /* db::query("JOIN") */ still comment */
            fn run() { let description = "JOIN"; path.join("JOIN");
                tokio::join!(one(), two()); let c = '\'';
                db::query("SELECT 'JOIN' FROM a"); }
        ''')
        self.assertEqual(len(findings), 1)
        self.assertFalse(findings[0].issues | findings[0].unresolved)

    def test_escaped_newline_raw_and_turbofish(self):
        findings = self.scan('''fn run() {
            db::query_as::<(String, i64)>("SELECT * FROM a \\\n+                JOIN b ON a.id=b.id");
            db::query(r###"SELECT * FROM a, b WHERE a.id=b.id"###);
        }''')
        self.assertEqual(len(findings), 2)
        self.assertTrue(all(x.issues and not x.unresolved for x in findings))

    def test_constant_and_format_capture_are_expanded(self):
        findings = self.scan('''const ROWS: &str = "FROM a JOIN b ON a.id=b.id";
            fn run() { let sql = format!("SELECT a.id {ROWS}"); db::query(&sql); }''')
        self.assertEqual(len(findings), 1)
        self.assertTrue(findings[0].issues)
        self.assertFalse(findings[0].unresolved)

    def test_concat_include_and_replace(self):
        self.put("src/query.sql", "SELECT * FROM a JOIN b ON a.id=b.id")
        findings = self.scan('''fn run() {
            db::query(concat!("SELECT * FROM a ", "JOIN b ON a.id=b.id"));
            db::raw_sql(include_str!("query.sql"));
            let sql = include_str!("query.sql").replace("JOIN b ON a.id=b.id", "");
            db::query(&sql);
        }''')
        self.assertEqual(len(findings), 3)
        self.assertTrue(findings[0].issues and findings[1].issues)
        self.assertFalse(findings[2].issues | findings[2].unresolved)

    def test_conditional_and_array_variants(self):
        findings = self.scan('''fn run(flag: bool) {
            let clause = if flag { "FROM a" } else { "FROM a,b" };
            db::query(&format!("SELECT * {clause}"));
            for sql in ["SELECT * FROM a", "SELECT * FROM a JOIN b"] { db::query(sql); }
            for (sql, other) in [("SELECT * FROM a,b", "ignored"), ("SELECT 1", "x")] { db::query(sql); }
        }''')
        self.assertEqual(len(findings), 3)
        self.assertTrue(all(x.issues and not x.unresolved for x in findings))

    def test_unknown_runtime_string_is_never_passed(self):
        findings = self.scan('fn run(sql: &str) { db::query(sql); }')
        self.assertTrue(findings[0].unresolved)
        with contextlib.redirect_stdout(io.StringIO()) as out:
            self.assertEqual(guard.main(["--root", str(self.root)]), 2)
        self.assertIn("NOT a pass", out.getvalue())

    def test_unknown_format_fragment_still_rejects_known_join(self):
        findings = self.scan('fn run(name: &str) { db::query(&format!("SELECT * FROM a JOIN {name}")); }')
        self.assertTrue(findings[0].unresolved)
        self.assertTrue(findings[0].issues)

    def test_mutation_cannot_hide_later_sql(self):
        findings = self.scan('''fn run() { let mut sql = "SELECT * FROM a".to_owned();
            sql.push_str(" JOIN b"); db::query(&sql); }''')
        self.assertTrue(findings[0].issues)
        findings = self.scan('''fn run(input: &str) { let mut sql = "SELECT * FROM a".to_owned();
            sql.push_str(input); db::query(&sql); }''')
        self.assertTrue(findings[0].unresolved)

    def test_macro_destination_mutation_is_unresolved(self):
        for mutation in (
            'write!(sql, " JOIN b ON a.id=b.id").unwrap();',
            'writeln!(sql, " JOIN b ON a.id=b.id").unwrap();',
            'std::write!(sql, " JOIN b ON a.id=b.id").unwrap();',
            'opaque_mutator!(sql);',
        ):
            source = 'use std::fmt::Write; fn run() { let mut sql = "SELECT * FROM a".to_owned(); ' + mutation + ' db::query(&sql); }'
            with self.subTest(mutation=mutation):
                self.assertTrue(self.scan(source)[0].unresolved)
                with contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(guard.main(["--root", str(self.root)]), 2)

    def test_macro_literal_matching_binding_name_is_not_mutation(self):
        findings = self.scan('fn run() { let sql = "SELECT * FROM a".to_owned(); println!("sql"); db::query(&sql); }')
        self.assertFalse(findings[0].unresolved)
        self.assertFalse(findings[0].issues)

    def test_parenthesized_from_is_unresolved_at_query_sink(self):
        self.assertTrue(self.scan('fn run() { db::query("SELECT * FROM (a,b) WHERE a.id=b.id"); }')[0].unresolved)
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(guard.main(["--root", str(self.root)]), 2)

    def test_finite_helper_and_certified_placeholder_repetition(self):
        findings = self.scan('''fn rows(table: &str, columns: &str, n: usize) {
            let sql = format!("SELECT {columns} FROM {table} WHERE id IN ({})", vec!["?"; n].join(","));
            db::query(&sql);
        }
        fn run() { rows("a", "id,name", 4); rows("b", "id", 8); }''')
        self.assertEqual(len(findings), 1)
        self.assertFalse(findings[0].issues | findings[0].unresolved)
        self.assertEqual(findings[0].variants, 4)

    def test_numeric_format_constant(self):
        findings = self.scan('const BATCH: usize = 256; fn run() { db::query(&format!("SELECT * FROM a LIMIT {BATCH}")); }')
        self.assertFalse(findings[0].issues | findings[0].unresolved)

    def test_finite_helper_rejects_bad_new_caller(self):
        findings = self.scan('''fn rows(table: &str) { db::query(&format!("SELECT * FROM {table}")); }
            fn run() { rows("a"); rows("a JOIN b"); }''')
        self.assertTrue(findings[0].issues)

    def test_public_sql_parameter_cannot_assume_known_callers(self):
        findings = self.scan('''pub fn rows(table: &str) { db::query(&format!("SELECT * FROM {table}")); }
            fn run() { rows("a"); }''')
        self.assertTrue(findings[0].unresolved)

    def test_safe_optional_append_is_fully_checked(self):
        findings = self.scan('''fn run(flag: bool) { let mut sql = "SELECT * FROM a".to_owned();
            if flag { sql.push_str(" WHERE id=?"); } db::query(&sql); }''')
        self.assertFalse(findings[0].issues | findings[0].unresolved)
        self.assertEqual(findings[0].variants, 2)

    def test_escaped_identifier_requires_surrounding_quotes(self):
        good = r'''fn run(name: &str) { db::query(&format!("SELECT * FROM \"{}\"", name.replace('"', "\"\""))); }'''
        finding = self.scan(good)[0]
        self.assertFalse(finding.issues | finding.unresolved)
        bad = r'''fn run(name: &str) { db::query(&format!("SELECT * FROM {}", name.replace('"', "\"\""))); }'''
        self.assertTrue(self.scan(bad)[0].unresolved)

    def test_array_binding_iteration_and_zip(self):
        findings = self.scan('''fn rows(table: &str) { db::query(&format!("SELECT * FROM {table}")); }
            fn run() { let tables = ["a", "b"];
                for table in tables { rows(table); }
                for (table, value) in tables.into_iter().zip(other) { rows(table); }
            }''')
        self.assertEqual(len(findings), 1)
        self.assertFalse(findings[0].issues | findings[0].unresolved)

    def test_parameter_not_replaced_by_other_module_constant(self):
        self.put("src/other.rs", 'const sql: &str = "SELECT * FROM safe";')
        findings = self.scan('pub fn run(sql: &str) { db::query(sql); }')
        self.assertTrue(findings[0].unresolved)

    def test_same_named_helpers_include_qualified_callers(self):
        self.put("src/a.rs", 'pub(crate) fn rows(table: &str) { db::query(&format!("SELECT * FROM {table}")); } fn run() { rows("a"); }')
        self.put("src/b.rs", 'pub(crate) fn rows(table: &str) { db::query(&format!("SELECT * FROM {table}")); } fn run() { rows("b"); }')
        findings = self.scan('fn run() { a::rows("a JOIN bad"); }')
        found = next(x for x in findings if x.path == "src/a.rs")
        self.assertTrue(found.issues or found.unresolved)

    def test_quote_proof_does_not_apply_in_comment_or_after_closed_literal(self):
        for template in ("SELECT * FROM a /* '{}' */", "SELECT * FROM a WHERE x='literal'{}'suffix'"):
            source = 'pub fn run(input: &str) { db::query(&format!(' + repr(template).replace("\\'", "'") + ', input.replace(\'\\\'\', "\'\'"))); }'
            # JSON double quoting is also valid Rust for these ASCII templates.
            import json
            source = 'pub fn run(input: &str) { db::query(&format!(' + json.dumps(template) + ', input.replace(\'\\\'\', "\'\'"))); }'
            with self.subTest(template=template):
                findings = self.scan(source)
                self.assertTrue(findings[0].unresolved)

    def test_public_modifier_orders_are_unbounded(self):
        for prefix in ('pub unsafe fn', 'pub async unsafe fn', 'pub extern "C" fn'):
            findings = self.scan(prefix + ' rows(table: &str) { db::query(&format!("SELECT * FROM {table}")); } fn run() { rows("safe"); }')
            with self.subTest(prefix=prefix): self.assertTrue(findings[0].unresolved)

    def test_sql_alias_and_query_macro_are_checked(self):
        findings = self.scan('''use crate::storage::native_turso::query as q;
            fn run() { q("SELECT * FROM a JOIN b");
                let issue = db::query; issue("SELECT * FROM a,b");
                sqlx::query!("SELECT * FROM a JOIN b"); }''')
        self.assertEqual(len(findings), 3)
        self.assertTrue(all(x.issues for x in findings))

    def test_invisible_same_file_constant_cannot_replace_parameter(self):
        findings = self.scan('mod other { const sql: &str = "SELECT * FROM safe"; } fn run(sql: &str) { db::query(sql); }')
        self.assertTrue(findings[0].unresolved)

    def test_mutable_alias_and_unknown_string_method_are_unresolved(self):
        for mutation in ('alter(&mut sql);', 'sql.insert_str(15, " JOIN b");', 'let alias = &mut sql; alter(alias);'):
            source = 'fn alter(s: &mut String) { s.push_str(" JOIN b"); } fn run() { let mut sql = "SELECT * FROM a".to_owned(); ' + mutation + ' db::query(&sql); }'
            with self.subTest(mutation=mutation): self.assertTrue(self.scan(source)[0].unresolved)

    def test_native_connection_ufcs_is_a_sql_input(self):
        findings = self.scan('fn run(c: Connection) { turso::Connection::execute(c, "SELECT * FROM a JOIN b", ()); }')
        self.assertEqual(len(findings), 1)
        self.assertTrue(findings[0].issues)

    def test_quote_proof_checks_every_prefix_variant(self):
        findings = self.scan(r'''pub fn run(input: &str, flag: bool) {
            let prefix = if flag { "SELECT * FROM a WHERE x=" } else { "SELECT * FROM a --" };
            db::query(&format!("{}'{}'\n", prefix, input.replace('\'', "''")));
        }''')
        self.assertTrue(findings[0].unresolved)

    def test_source_contract_cannot_apply_to_same_named_foreign_helper(self):
        with patch.object(guard, "FILTER_SOURCE_SHAS", {}):
            findings = self.scan('fn scoped_filter(input: &str) -> &str { input } pub fn run(input: &str) { db::query(scoped_filter(input)); }')
        self.assertTrue(findings[0].unresolved)

    def test_exact_function_scope_and_missing_scope(self):
        findings = self.scan('''fn old() { db::query("SELECT * FROM a JOIN b"); }
            fn new() { db::query("SELECT * FROM a"); }''', {"src/lib.rs::new"})
        self.assertEqual(len(findings), 1)
        self.assertFalse(findings[0].issues | findings[0].unresolved)
        missing = guard.audit(self.root, {"src/lib.rs::typo"})
        self.assertTrue(missing[0].unresolved)

    def test_exact_baseline_changed_sql_and_zero_mode(self):
        findings = self.scan('fn run() { db::query("SELECT * FROM a JOIN b"); }')
        key = findings[0].key
        with patch.object(guard, "BASELINE", frozenset({key})), contextlib.redirect_stdout(io.StringIO()) as out:
            self.assertEqual(guard.main(["--root", str(self.root)]), 0)
            self.assertIn("NOT JOIN-free", out.getvalue())
            self.assertEqual(guard.main(["--root", str(self.root), "--zero", "src/lib.rs::run"]), 1)
            self.put("src/lib.rs", 'fn run() { db::query("SELECT * FROM a JOIN c"); }')
            self.assertEqual(guard.main(["--root", str(self.root)]), 1)

    def test_new_duplicate_site_cannot_reuse_baseline(self):
        source = 'fn run() { db::query("SELECT * FROM a JOIN b"); }'
        key = self.scan(source)[0].key
        self.put("src/lib.rs", source.replace("; }", '; db::query("SELECT * FROM a JOIN b"); }'))
        with patch.object(guard, "BASELINE", frozenset({key})), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(guard.main(["--root", str(self.root)]), 1)

    def test_changed_fragment_invalidates_baseline(self):
        source = 'const PART: &str = "FROM a JOIN b"; fn run() { db::query(&format!("SELECT * {PART}")); }'
        first = self.scan(source)[0]
        second = self.scan(source.replace("JOIN b", "JOIN c"))[0]
        self.assertNotEqual(first.key, second.key)

    def test_unknown_remains_nonzero_even_if_baselined(self):
        first = self.scan('fn run(sql: &str) { db::query(sql); }')[0]
        with patch.object(guard, "BASELINE", frozenset({first.key})), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(guard.main(["--root", str(self.root)]), 2)

    def test_domain_prepare_and_typed_query_are_not_sql(self):
        findings = self.scan('''fn query(scope: Scope, text: &str) -> FullTextQuery { todo!() }
            fn run(service: Service) { prepare(service); ann::prepare(service);
                query(scope(), "search text"); db::query("SELECT 1"); }''')
        self.assertEqual(len(findings), 1)
        self.assertFalse(findings[0].issues | findings[0].unresolved)


class PythonSqlInputs(unittest.TestCase):
    setUp = RustInputs.setUp
    tearDown = RustInputs.tearDown
    put = RustInputs.put
    # Reuse only fixture setup/helpers, not the Rust test suite.
    def scan_python(self, source):
        self.put("tools/query.py", source)
        return guard.audit(self.root)

    def test_python_actual_calls_only(self):
        findings = self.scan_python('''text = 'db.execute("SELECT * FROM a JOIN b")'
# db.execute("SELECT * FROM a JOIN b")
def run(db):
    db.execute("SELECT 'JOIN b' FROM a /* JOIN c */")
    db.executemany("SELECT * FROM a JOIN b", [])
    db.executescript("SELECT * FROM a; SELECT * FROM b;")
''')
        self.assertEqual(len(findings), 3)
        self.assertFalse(findings[0].issues | findings[0].unresolved)
        self.assertTrue(findings[1].issues)
        self.assertFalse(findings[2].issues | findings[2].unresolved)

    def test_python_finite_dictionary_comprehension(self):
        findings = self.scan_python('''def run(db):
    queries = {"a": "SELECT * FROM a", "b": "SELECT * FROM b"}
    return {name: db.execute(query).fetchall() for name, query in queries.items()}
''')
        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].variants, 2)
        self.assertFalse(findings[0].issues | findings[0].unresolved)

    def test_python_dynamic_is_unresolved(self):
        for expr in ('sql', 'f"SELECT * FROM {table}"', 'make_sql()'):
            with self.subTest(expr=expr):
                self.assertTrue(self.scan_python('def run(db, sql, table):\n    db.execute(' + expr + ')\n')[0].unresolved)

    def test_python_assignment_concat_and_alias(self):
        findings = self.scan_python('''def run(db):
    sql = "SELECT * FROM a " + "JOIN b"
    issue = db.execute
    issue(sql)
''')
        self.assertEqual(len(findings), 1)
        self.assertTrue(findings[0].issues)

    def test_python_collection_mutation_is_not_silently_ignored(self):
        for mutation in ('queries.update(extra)', 'alter(queries)', 'queries["b"] = extra'):
            source = 'def run(db, extra):\n    queries = {"a": "SELECT * FROM a"}\n    ' + mutation + '\n    return {name: db.execute(query) for name, query in queries.items()}\n'
            with self.subTest(mutation=mutation): self.assertTrue(self.scan_python(source)[0].unresolved)


if __name__ == "__main__":
    unittest.main()
