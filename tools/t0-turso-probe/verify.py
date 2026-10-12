#!/usr/bin/env python3
"""Bounded synthetic Origence SQL/FTS probe; no app data or providers.

The Rust bridge is deliberately a subprocess: killing it is the behavior under
test. A 30-second request deadline and a 180-second per-process lifetime prevent
unbounded hangs; timeout/crash is evidence of failure, never a successful skip.
SQLite mode validates fixture SQL/expectations only, not Turso compatibility.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import queue
import re
import shutil
import sqlite3
import subprocess
import tempfile
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
SCHEMAS = [
    "sqlite-schema.sql", "memory-identity-schema.sql", "memory-semantics-schema.sql",
    "session-schema.sql", "learning-schema.sql", "sqlite-graph-schema.sql",
]
BASELINE = "d0521c0935083e71890ab02c100920c2edaa7227"
T, W, P, E, A, J = ({"blob": f"{n:032x}"} for n in range(1, 7))
C = {"blob": f"{7:032x}"}
CHUNK_COLUMNS = "tenant_id,workspace_id,id,asset_id,version,ordinal,content,locator,search_terms"


class NotRun(Exception):
    """A requested observation did not occur; never promote it to PASS."""


def blob(n):
    return {"blob": f"{n:032x}"}


def literal_fts_query(terms):
    """Input is already lexical/search_terms output, never advanced FTS syntax."""
    unique = list(dict.fromkeys(terms.split()))
    return " AND ".join('"' + term.replace('\\', '\\\\').replace('"', '\\"') + '"' for term in unique) or None


def statements(text):
    """SQLite's lexer is only a statement boundary detector, not the tested engine."""
    pending = ""
    for line in text.splitlines(keepends=True):
        pending += line
        if sqlite3.complete_statement(pending):
            yield pending
            pending = ""
    assert not pending.strip(), "unterminated DDL fixture"


def rust_strings(path):
    text = (ROOT / path).read_text()
    for match in re.finditer(r'"(?:[^"\\]|\\.)*"', text, re.S):
        raw = re.sub(r"\\\n\s*", "", match.group())
        try:
            yield json.loads(raw)
        except json.JSONDecodeError:
            continue


def actual_sql(path, prefix):
    matches = [value for value in rust_strings(path) if value.startswith(prefix)]
    assert len(matches) == 1, (path, prefix, len(matches))
    return matches[0]


class Bridge:
    def __init__(self, binary, path, create=True):
        self.path = path
        self.log = open(path.parent / "bridge.stderr", "a", encoding="utf-8")
        self.proc = subprocess.Popen(
            [str(binary), "create" if create else "open", str(path)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log,
            text=True, bufsize=1,
        )
        self.output = queue.Queue()
        self.started = time.monotonic()
        self.lifetime_timer = threading.Timer(180, self.kill)
        self.lifetime_timer.daemon = True
        self.lifetime_timer.start()

        def reader():
            for line in self.proc.stdout:
                self.output.put(line)
            self.output.put(None)

        threading.Thread(target=reader, daemon=True).start()
        try:
            ready = self.read()
            assert ready.get("ready"), ready
        except Exception:
            self.kill()
            raise

    def read(self):
        try:
            line = self.output.get(timeout=30)
        except queue.Empty as error:
            self.kill()
            raise RuntimeError("SQL bridge request timed out after 30 seconds") from error
        if line is None:
            raise RuntimeError(f"SQL bridge exited unexpectedly: {self.proc.poll()}")
        return json.loads(line)

    def raw(self, sql, params=(), op="query", connection="default", **extra):
        if time.monotonic() - self.started >= 180:
            self.kill()
            raise RuntimeError("bridge lifetime limit exceeded")
        request = dict(sql=sql, params=list(params), op=op, connection=connection, **extra)

        def send():
            try:
                self.proc.stdin.write(json.dumps(request, ensure_ascii=False) + "\n")
                self.proc.stdin.flush()
            except (BrokenPipeError, OSError, ValueError) as error:
                self.output.put(json.dumps({"ok": False, "error": f"bridge write failed: {error}"}))

        # The deadline covers blocked pipe writes as well as waiting for a SQL
        # response. A hung child cannot prevent the parent from killing it.
        writer = threading.Thread(target=send, daemon=True)
        writer.start()
        response = self.read()
        writer.join(timeout=1)
        if writer.is_alive():
            self.kill()
            raise RuntimeError("bridge request writer did not finish")
        return response

    def query(self, sql, params=(), **extra):
        response = self.raw(sql, params, **extra)
        assert response["ok"], response
        return response["result"]["rows"]

    def execute(self, sql, params=(), **extra):
        response = self.raw(sql, params, op="execute", **extra)
        assert response["ok"], response
        return response["result"]["changed"]

    def close(self):
        self.lifetime_timer.cancel()
        if self.proc.poll() is None:
            self.proc.stdin.close()
            try:
                self.proc.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.kill()
                raise
        self.log.close()
        assert self.proc.returncode == 0, self.proc.returncode

    def kill(self):
        self.lifetime_timer.cancel()
        if self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait(timeout=15)
        self.log.close()


class SQLite:
    """Fixture baseline. Does not emulate Turso-only FTS/driver operations."""
    def __init__(self, path):
        self.connections = {}
        self.path = path

    def raw(self, sql, params=(), op="query", connection="default", **extra):
        if connection not in self.connections:
            conn = sqlite3.connect(self.path, isolation_level=None)
            conn.execute("PRAGMA journal_mode=WAL")
            conn.execute("PRAGMA foreign_keys=ON")
            self.connections[connection] = conn
        conn = self.connections[connection]
        values = [bytes.fromhex(p["blob"]) if isinstance(p, dict) else p for p in params]
        try:
            cursor = conn.execute(sql, values)
            rows = [[{"blob": p.hex()} if isinstance(p, bytes) else p for p in row]
                    for row in cursor.fetchall()]
            return {"ok": True, "result": {"rows": rows, "changed": cursor.rowcount}}
        except sqlite3.Error as error:
            return {"ok": False, "error_category": "constraint" if isinstance(error, sqlite3.IntegrityError) else "engine_other", "error": str(error)}

    query = Bridge.query
    execute = Bridge.execute

    def close(self):
        for conn in self.connections.values():
            conn.close()

    kill = close


def initialize(db):
    count = 0
    for name in SCHEMAS:
        for sql in statements((ROOT / "src/storage" / name).read_text()):
            db.execute(sql)
            count += 1
    return count


def seed(db):
    db.execute("INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES (?,?,?,?)", [T, W, "synthetic", 1700000000123])
    db.execute("INSERT INTO oc_events(tenant_id,workspace_id,id,content,kind,created_by,created_at) VALUES (?,?,?,?,?,?,?)", [T, W, E, "synthetic source", "text", P, 1700000000123])
    db.execute("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,fact_key,current_version,created_at) VALUES (?,?,?,?,?,?,?,?)", [T, W, A, "memory", "synthetic", "fact", 1, 1700000000123])
    db.execute("INSERT INTO oc_versions(tenant_id,workspace_id,asset_id,version,content,content_hash,source_event_id,title,created_by,created_at) VALUES (?,?,?,?,?,?,?,?,?,?)", [T, W, A, 1, "alpha beta", "fixture", E, "synthetic", P, 1700000000123])
    enqueue = actual_sql("src/storage/sqlite/job_queue.rs", "INSERT INTO oc_jobs(tenant_id")
    db.execute(enqueue, [T, W, J, P, "message_batch_ingest", '{"publication":{"memories":[{"chunks":[{"id":"one"},{"id":"two"}]}]}}', A, E, 1700000000123, 1700000000123])
    db.execute(f"INSERT INTO oc_chunks({CHUNK_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?)", [T, W, C, A, 1, 0, "alpha beta", '{"parser":"memory-semantics:v1","support_sources":["00000000-0000-0000-0000-000000000004"]}', "alpha beta"])


def expected_error(db, sql, params=(), trigger_message=None):
    def snapshot():
        tables = [row[0] for row in db.query("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'oc_%' ORDER BY name")]
        return {name: sorted(json.dumps(row, sort_keys=True) for row in db.query(f'SELECT * FROM "{name}"')) for name in tables}
    before = snapshot()
    response = db.raw(sql, params, op="execute")
    # RAISE(ABORT, ...) is surfaced as the SDK's generic Error in 0.8.2.
    # Admit only an exact, fixture-owned trigger message for those named tests;
    # unrelated syntax/transport errors must still fail the constraint gate.
    rejected = response.get("error_category") == "constraint"
    if trigger_message is not None:
        rejected = (response.get("error_category") in {"constraint", "engine_other"}
                    and response.get("error") == trigger_message)
    assert not response["ok"] and rejected, response
    assert snapshot() == before, "constraint failure changed business rows"
    return {"rejected": True, "engine_error": response["error"], "error_category": response["error_category"], "all_business_rows_unchanged": True}


def normalize_sql(sql):
    """Compare DDL tokens without changing quoted values or identifier bytes."""
    pattern = re.compile(
        r"(?P<quoted>'(?:''|[^'])*'|\"(?:\"\"|[^\"])*\"|`(?:``|[^`])*`|\[[^\]]*\])"
        r"|(?P<comment>--[^\r\n]*|/\*.*?\*/)"
        r"|[A-Za-z_][A-Za-z_0-9]*|[0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?"
        r"|<>|!=|<=|>=|\|\||[^\s]",
        re.S,
    )
    tokens = [match.group() if match.lastgroup == "quoted" else match.group().lower()
              for match in pattern.finditer(sql) if match.lastgroup != "comment"]
    if tokens and tokens[-1] == ";":
        tokens.pop()
    return tuple(tokens)


def schema_roundtrip(db, allow_known_key_quote=False):
    checked = []
    for name in SCHEMAS:
        for statement in statements((ROOT / "src/storage" / name).read_text()):
            match = re.search(r"CREATE (TABLE|(?:UNIQUE )?INDEX|TRIGGER) (?:IF NOT EXISTS )?(\w+)", statement)
            assert match, statement
            kind, obj = match.groups()
            original = statement[match.start():].strip()
            # Existing graph schema uses IF NOT EXISTS; sqlite_master omits it.
            original = original.replace("IF NOT EXISTS ", "")
            rows = db.query("SELECT sql FROM sqlite_master WHERE name=?", [obj])
            actual = rows[0][0] if len(rows) == 1 else ""
            if allow_known_key_quote and obj == "oc_commands":
                # A tightly scoped candidate, never a general quote stripper.
                # This source table has no quoted string literal containing key.
                assert "'" not in original and "'" not in actual
                actual = re.sub(r'"key"', "key", actual)
            assert len(rows) == 1 and normalize_sql(actual) == normalize_sql(original), (obj, rows)
            checked.append(obj)
    return {"checked_objects": checked}


def schema_metadata(db):
    objects = db.query("SELECT type,name,tbl_name,sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY type,name")
    tables = {}
    for schema in SCHEMAS:
        for sql in statements((ROOT / "src/storage" / schema).read_text()):
            match = re.search(r"CREATE TABLE (?:IF NOT EXISTS )?(\w+)", sql)
            if match:
                name = match.group(1)
                tables[name] = {
                    "columns": db.query(f'PRAGMA table_info("{name}")'),
                    "foreign_keys": db.query(f'PRAGMA foreign_key_list("{name}")'),
                    "indexes": db.query(f'PRAGMA index_list("{name}")'),
                }
    value = {"objects": objects, "tables": tables}
    digest = hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()
    return {"sha256": digest, **value}


def relational_cases(db, record):
    record("uuid_json_time_roundtrip", lambda: check_equal(db.query("SELECT id,typeof(id),length(id),created_at FROM oc_workspaces WHERE tenant_id=?", [T]), [[W, "blob", 16, 1700000000123]]))
    record("schema_master_exact_normalization", lambda: schema_roundtrip(db))
    record("schema_narrow_key_identifier_quote_candidate", lambda: schema_roundtrip(db, allow_known_key_quote=True))
    record("composite_foreign_key", lambda: expected_error(db, "INSERT INTO oc_chunks SELECT tenant_id,workspace_id,?, ?,version,99,content,locator,search_terms FROM oc_chunks WHERE id=?", [blob(90), blob(999), C]))
    record("partial_unique_memory_slot", lambda: expected_error(db, "INSERT INTO oc_assets SELECT tenant_id,workspace_id,? ,kind,title,fact_key,current_version,deleted,created_at FROM oc_assets WHERE id=?", [blob(91), A]))
    def immutable_owner():
        db.execute("BEGIN IMMEDIATE")
        db.execute("INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES (?,?,?,?)", [T, blob(75), "valid alternative root", 1])
        # A real root owner satisfies the FK; only the immutable-owner rule
        # should reject this reassignment. A nonexistent owner would falsely
        # pass this probe even with the trigger removed.
        result = expected_error(db, "UPDATE oc_workspaces SET owner_workspace_id=? WHERE id=?", [blob(75), W], trigger_message="workspace ownership is immutable")
        db.execute("ROLLBACK")
        return result
    record("owner_immutable_trigger", immutable_owner)

    def owned_root():
        db.execute("INSERT INTO oc_workspaces VALUES (?,?,?,?,?)", [T, blob(80), "owned", W, 1])
        try:
            return expected_error(db, "INSERT INTO oc_workspaces VALUES (?,?,?,?,?)", [T, blob(81), "nested", blob(80), 1], trigger_message="workspace owner must be a root")
        finally:
            db.execute("DELETE FROM oc_workspaces WHERE id=?", [blob(80)])
    record("one_level_owner_trigger", owned_root)

    def queue_returning():
        sql = actual_sql("src/storage/sqlite/job_queue.rs", "UPDATE oc_jobs SET state = ?, generation")
        db.execute("BEGIN IMMEDIATE")
        check_equal(db.query(sql, ["pending", 0, 2, T, W, J]), [[2]])
        db.execute("ROLLBACK")
        return check_equal(db.query("SELECT generation,run_token FROM oc_jobs WHERE id=?", [J]), [[1, 0]])
    record("real_job_returning_explicit_rollback", queue_returning)

    def actual_json():
        sql = actual_sql("src/storage/sqlite/publication.rs", "SELECT operation,state,generation,asset_id,source_event_id,")
        row = db.query(sql, [T, W, J])[0]
        assert row[-2:] == [2, 2], row
        update = actual_sql("src/storage/sqlite/app.rs", "UPDATE oc_jobs SET payload=json_set")
        db.execute("BEGIN IMMEDIATE")
        db.execute(update, ['{"generation":7}', 2, T, W, J])
        check_equal(db.query("SELECT json_extract(payload,'$.publication.generation') FROM oc_jobs WHERE id=?", [J]), [[7]])
        db.execute("ROLLBACK")
        return {"publication_json_each_distinct": row[-2:], "json_set": True}
    record("real_publication_json_queries", actual_json)

    def visible():
        current = actual_sql("src/storage/sqlite/visibility.rs", "FROM oc_chunks c JOIN")
        support = actual_sql("src/storage/sqlite/visibility.rs", "(json_extract(c.locator")
        sql = f"SELECT c.id {current} AND {support}"
        check_equal(db.query(sql, [T, W]), [[C]])
        db.execute("BEGIN IMMEDIATE")
        db.execute("UPDATE oc_events SET state='retracted' WHERE id=?", [E])
        check_equal(db.query(sql, [T, W]), [])
        db.execute("ROLLBACK")
        check_equal(db.query(sql, [blob(200), W]), [])
        return {"active": 1, "retracted": 0, "wrong_tenant": 0}
    record("real_source_visibility_predicate", visible)

    def graph_atomic():
        db.execute("BEGIN IMMEDIATE")
        for uid in ["first", "second"]:
            db.execute("INSERT INTO oc_graph_entities VALUES (?,?,?,?,?,?,?,?,?)", [uid, "t", "w", uid, uid, "entity", "synthetic", "source", 1])
        db.execute("INSERT INTO oc_graph_relations VALUES (?,?,?,?,?,?,?,?)", ["edge", "t", "w", "first", "second", "edge", "source", 1])
        db.execute("UPDATE oc_jobs SET state='completed' WHERE id=?", [J])
        check_equal(db.query("SELECT count(*) FROM oc_graph_relations"), [[1]])
        db.execute("DELETE FROM oc_graph_entities WHERE uid='first'")
        check_equal(db.query("SELECT count(*) FROM oc_graph_relations"), [[0]])
        db.execute("ROLLBACK")
        check_equal(db.query("SELECT count(*) FROM oc_graph_entities"), [[0]])
        return check_equal(db.query("SELECT state FROM oc_jobs WHERE id=?", [J]), [["pending"]])
    record("graph_cascade_same_transaction_rollback", graph_atomic)

    def actual_recursive_graph():
        sql = actual_sql("src/storage/sqlite_graph.rs", "WITH RECURSIVE reach")
        db.execute("BEGIN IMMEDIATE")
        tenant, workspace = str(uuid.UUID(hex=T["blob"])), str(uuid.UUID(hex=W["blob"]))
        labels = ["a", "b", "c", "d", "e", "incoming", "wrong-scope"]
        ids = {label: str(uuid.UUID(int=9000 + number)) for number, label in enumerate(labels)}
        uids = {label: f"{tenant}|{workspace}|{identifier}" for label, identifier in ids.items()}
        for label in labels:
            db.execute("INSERT INTO oc_graph_entities VALUES (?,?,?,?,?,?,?,?,?)", [uids[label], tenant, workspace, ids[label], label, "entity", "synthetic", "source", 1])
        edges = [("a", "b", tenant), ("a", "b", tenant), ("b", "c", tenant), ("c", "d", tenant), ("d", "e", tenant), ("c", "a", tenant), ("incoming", "a", tenant), ("a", "wrong-scope", "other")]
        for number, (head, tail, tenant) in enumerate(edges):
            db.execute("INSERT INTO oc_graph_relations VALUES (?,?,?,?,?,?,?,?)", [str(number), tenant, workspace, uids[head], uids[tail], "edge", "source", 1])
        tenant = str(uuid.UUID(hex=T["blob"]))
        observed = {}
        for hops, labels in [(0, []), (1, ["b"]), (2, ["b", "c"]), (3, ["b", "c", "d"])]:
            expected = [[ids[label]] for label in labels]
            rows = db.query(sql, [uids["a"], hops, tenant, workspace, tenant, workspace, uids["a"]])
            check_equal(rows, expected)
            observed[str(hops)] = rows
        db.execute("ROLLBACK")
        return {"actual_production_sql_executed": True, "by_hops": observed, "incoming_and_wrong_scope_excluded": True}
    record("real_recursive_graph_direction_hops_scope_cycle", actual_recursive_graph)

    def upsert():
        db.execute("BEGIN IMMEDIATE")
        db.execute("INSERT INTO oc_memory_properties VALUES (?,?,?,?,?,?) ON CONFLICT(tenant_id,workspace_id,predicate) DO UPDATE SET definition=excluded.definition", [T, W, "synthetic", "{}", P, 1])
        db.execute("INSERT INTO oc_memory_properties VALUES (?,?,?,?,?,?) ON CONFLICT(tenant_id,workspace_id,predicate) DO UPDATE SET definition=excluded.definition", [T, W, "synthetic", '{"updated":true}', P, 2])
        check_equal(db.query("SELECT json_extract(definition,'$.updated') FROM oc_memory_properties"), [[1]])
        db.execute("ROLLBACK")
        return {"upsert": True}
    record("composite_upsert", upsert)

    def deferred_fk():
        session, improve, future_job = blob(850), blob(851), blob(852)
        run_sql = "INSERT INTO oc_improve_runs(tenant_id,workspace_id,improve_id,session_id,job_id,request_hash,request,snapshot,lower_sequence,target_sequence,created_by,created_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)"
        run_values = [T, W, improve, session, future_job, "fixture", "{}", "{}", 0, 0, P, 1]
        enqueue = actual_sql("src/storage/sqlite/job_queue.rs", "INSERT INTO oc_jobs(tenant_id")
        db.execute("BEGIN IMMEDIATE")
        db.execute("INSERT INTO oc_improve_cursors VALUES (?,?,?,?)", [T, W, session, 0])
        db.execute(run_sql, run_values)
        db.execute(enqueue, [T, W, future_job, P, "improve", "{}", None, None, 1, 1])
        db.execute("COMMIT")
        check_equal(db.query("SELECT job_id FROM oc_improve_runs WHERE improve_id=?", [improve]), [[future_job]])
        db.execute("BEGIN IMMEDIATE")
        db.execute("DELETE FROM oc_improve_runs WHERE improve_id=?", [improve])
        db.execute("DELETE FROM oc_improve_cursors WHERE session_id=?", [session])
        db.execute("DELETE FROM oc_jobs WHERE id=?", [future_job])
        db.execute("COMMIT")
        db.execute("BEGIN IMMEDIATE")
        db.execute("INSERT INTO oc_improve_cursors VALUES (?,?,?,?)", [T, W, session, 0])
        db.execute(run_sql, run_values)
        result = expected_error(db, "COMMIT")
        db.execute("ROLLBACK")
        check_equal(db.query("SELECT count(*) FROM oc_improve_runs"), [[0]])
        check_equal(db.query("SELECT count(*) FROM oc_improve_cursors"), [[0]])
        return {"late_parent_commit": True, "missing_parent_commit": result}
    record("real_learning_deferred_fk_commit_rollback", deferred_fk)

    def prepare_corpus():
        checked, errors, fragments = [], [], []
        paths = sorted((ROOT / "src/storage/sqlite").glob("*.rs"))
        paths.append(ROOT / "src/storage/sqlite_graph.rs")
        for path in paths:
            for sql in rust_strings(path.relative_to(ROOT)):
                if not re.match(r"^(SELECT|INSERT|UPDATE|DELETE|WITH RECURSIVE)\b", sql) or "{" in sql:
                    continue
                # These two projections are concatenated with CURRENT_CHUNK_ROWS
                # in app.rs. Test the assembled production query, not a fragment.
                if sql.startswith(("SELECT c.id, c.asset_id,c.version", "SELECT c.id,c.asset_id,c.version,a.kind")) and not sql.endswith("LIMIT 100"):
                    fragments.append(sql)
                    sql += " " + actual_sql("src/storage/sqlite/visibility.rs", "FROM oc_chunks c JOIN")
                # EXPLAIN compiles without executing. Question marks inside
                # quoted SQL literals are not placeholders.
                plain = re.sub(r"'(?:[^']|'')*'", "''", sql)
                response = db.raw("EXPLAIN " + sql, [None] * plain.count("?"))
                entry = {"path": str(path.relative_to(ROOT)), "sql_sha256": hashlib.sha256(sql.encode()).hexdigest()}
                if response["ok"]:
                    checked.append(entry)
                else:
                    errors.append({**entry, "sql": sql, "error": response["error"]})
        assert not errors, {"prepared": len(checked), "errors": errors}
        return {"prepared": len(checked), "statements": checked, "assembled_projection_fragments": len(fragments), "dynamic_formatted_sql": "not exhaustively covered; visibility fragments are exercised separately"}
    record("literal_real_sql_prepare_inventory", prepare_corpus)


def check_equal(actual, expected):
    assert actual == expected, {"actual": actual, "expected": expected}
    return {"observed": actual}


def driver_cases(db, record):
    def dropped():
        result = db.raw("UPDATE oc_jobs SET generation=777 WHERE id=?", [J], op="transaction_drop")
        assert result["ok"], result
        return check_equal(db.query("SELECT generation FROM oc_jobs WHERE id=?", [J]), [[1]])
    record("sdk_transaction_drop_then_reuse", dropped)

    def partial(take):
        db.execute("BEGIN IMMEDIATE")
        db.execute("INSERT INTO oc_jobs SELECT tenant_id,workspace_id,?,created_by,operation,payload,state,run_token,generation,attempt,next_retry_at,asset_id,source_event_id,cancel_requested,outcome,result,error_code,created_at,updated_at FROM oc_jobs WHERE id=?", [blob(606), J])
        result = db.raw("UPDATE oc_jobs SET generation=generation+1 RETURNING generation", op="returning_drop", take=take)
        assert result["ok"], result
        assert len(result["result"]["consumed"]) == take, result
        db.execute("ROLLBACK")
        check_equal(db.query("SELECT generation FROM oc_jobs WHERE id=?", [J]), [[1]])
        return result["result"]
    record("returning_unconsumed_drop_rollback_reuse", lambda: partial(0))
    record("returning_partially_consumed_drop_rollback_reuse", lambda: partial(1))

    def cancel():
        db.execute("BEGIN IMMEDIATE")
        result = db.raw("UPDATE oc_jobs SET generation=999 WHERE id=?", [J], op="poll_cancel")
        assert result["ok"], result
        db.execute("ROLLBACK")
        check_equal(db.query("SELECT generation FROM oc_jobs WHERE id=?", [J]), [[1]])
        if result["result"]["interrupted_at"] != "pending_after_first_poll":
            raise NotRun("write completed before first cancellation point; rollback/reuse passed, actual pending-future cancellation NOT_RUN")
        return result["result"]
    record("one_poll_cancel_rollback_reuse", cancel)

    def snapshot():
        db.execute("BEGIN", connection="reader")
        check_equal(db.query("SELECT generation FROM oc_jobs WHERE id=?", [J], connection="reader"), [[1]])
        db.execute("UPDATE oc_jobs SET generation=2 WHERE id=?", [J])
        check_equal(db.query("SELECT generation FROM oc_jobs WHERE id=?", [J], connection="reader"), [[1]])
        db.execute("COMMIT", connection="reader")
        check_equal(db.query("SELECT generation FROM oc_jobs WHERE id=?", [J], connection="reader"), [[2]])
        db.execute("UPDATE oc_jobs SET generation=1 WHERE id=?", [J])
        return {"stable_snapshot": True}
    record("two_connection_read_snapshot", snapshot)


def fts_setup(db):
    for table in ["oc_chunks", "oc_summaries"]:
        db.execute(f"CREATE INDEX t0_{table}_fts ON {table} USING fts(search_terms) WITH (tokenizer = 'whitespace')")


def fts_ids(db, query, connection="default"):
    return db.query("SELECT id FROM oc_chunks WHERE fts_match(search_terms, ?) ORDER BY id", [query], connection=connection)


def fts_cases(db, record):
    if not record("fts_create_on_real_composite_uuid_tables", lambda: (fts_setup(db), {"tables": ["oc_chunks", "oc_summaries"]})[1]):
        return False

    def plan():
        rows = db.query("EXPLAIN QUERY PLAN SELECT id FROM oc_chunks WHERE fts_match(search_terms, ?)", ["alpha"])
        assert "QUERY INDEX METHOD fts" in json.dumps(rows), rows
        return {"plan": rows}
    record("fts_explain_native_index", plan)
    record("fts_explicit_and", lambda: check_equal(fts_ids(db, '"alpha" AND "beta"'), [[C]]))
    record("fts_missing_and_term", lambda: check_equal(fts_ids(db, '"alpha" AND "absent"'), []))

    def literals():
        db.execute("BEGIN IMMEDIATE")
        terms = 'AND OR NOT foo"bar path\\part x:y v1.2 foo_bar 北京'
        db.execute("UPDATE oc_chunks SET search_terms=? WHERE id=?", [terms, C])
        for term in terms.split():
            check_equal(fts_ids(db, literal_fts_query(term)), [[C]])
        check_equal(fts_ids(db, literal_fts_query('AND 北京')), [[C]])
        assert literal_fts_query('  \n\t') is None
        db.execute("ROLLBACK")
        return {"roundtripped_literal_terms": terms.split(), "empty_terms": "no FTS query issued"}
    record("fts_user_operator_punctuation_literal_roundtrip", literals)

    def lifecycle():
        db.execute("BEGIN IMMEDIATE")
        db.execute("UPDATE oc_chunks SET search_terms='北京 Rust foo_bar v1.2' WHERE id=?", [C])
        own = fts_ids(db, '"北京" AND "Rust"')
        other = fts_ids(db, '"北京"', connection="fts_reader")
        check_equal(own, [[C]])
        check_equal(other, [])
        db.execute("ROLLBACK")
        check_equal(fts_ids(db, "alpha"), [[C]])
        check_equal(fts_ids(db, '"北京"'), [])
        return {"own_writes_observed": own, "other_before_commit": other, "rollback": True}
    record("fts_transaction_visibility_rollback", lifecycle)

    def update_delete():
        db.execute("UPDATE oc_chunks SET search_terms='北京 Rust foo_bar v1.2' WHERE id=?", [C])
        check_equal(fts_ids(db, '"北京" AND "Rust"'), [[C]])
        check_equal(fts_ids(db, '"foo_bar"'), [[C]])
        check_equal(fts_ids(db, '"v1.2"'), [[C]])
        check_equal(fts_ids(db, "alpha"), [])
        db.execute("BEGIN IMMEDIATE")
        db.execute("DELETE FROM oc_chunks WHERE id=?", [C])
        check_equal(fts_ids(db, '"北京"'), [])
        db.execute("ROLLBACK")
        db.execute("UPDATE oc_chunks SET search_terms='alpha beta' WHERE id=?", [C])
        db.execute("OPTIMIZE INDEX t0_oc_chunks_fts")
        return check_equal(fts_ids(db, "alpha"), [[C]])
    record("fts_unicode_update_delete_optimize", update_delete)

    def summaries():
        db.execute("BEGIN IMMEDIATE")
        for identifier, term in [(40, "alpha"), (41, "beta")]:
            db.execute("INSERT INTO oc_summaries VALUES (?,?,?,?,?,?,?)", [T, W, blob(identifier), C, term, "fixture", term])
        # Per-term indexed candidates intersect by full scoped chunk key. A
        # single-summary AND query must not replace this cross-summary union.
        candidates = []
        for term in ["alpha", "beta"]:
            rows = db.query("SELECT tenant_id,workspace_id,chunk_id FROM oc_summaries WHERE fts_match(search_terms, ?)", [term])
            candidates.append({json.dumps(row, sort_keys=True) for row in rows})
        assert len(candidates[0] & candidates[1]) == 1
        single = db.query("SELECT id FROM oc_summaries WHERE fts_match(search_terms, 'alpha AND beta')")
        check_equal(single, [])
        db.execute("ROLLBACK")
        return {"matching_chunks": 1, "single_summary_matches": 0}
    record("fts_summary_terms_union_same_chunk", summaries)

    def bm25():
        db.execute("BEGIN IMMEDIATE")
        db.execute(f"INSERT INTO oc_chunks({CHUNK_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?)", [T, W, blob(61), A, 1, 61, "short", "{}", "alpha alpha alpha"])
        db.execute(f"INSERT INTO oc_chunks({CHUNK_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?)", [T, W, blob(62), A, 1, 62, "long", "{}", "alpha " + "filler " * 100])
        rows = db.query("SELECT id,fts_score(search_terms, 'alpha') AS score FROM oc_chunks WHERE fts_match(search_terms,'alpha') ORDER BY score DESC")
        scores = {row[0]["blob"]: row[1] for row in rows}
        assert scores[blob(61)["blob"]] > scores[blob(62)["blob"]] > 0, rows
        assert [row[1] for row in rows] == sorted(scores.values(), reverse=True), rows
        db.execute("ROLLBACK")
        return {"rows": rows, "direction": "descending", "corpus": "global index statistics; scoped production default not selected"}
    record("fts_positive_score_descending", bm25)

    def scoped(shape="joined"):
        current = actual_sql("src/storage/sqlite/visibility.rs", "FROM oc_chunks c JOIN")
        support = actual_sql("src/storage/sqlite/visibility.rs", "(json_extract(c.locator")
        indexes = iter(["?2", "?3"])
        numbered_current = re.sub(r"\?", lambda _: next(indexes), current)
        # The FTS optimizer matches the score and match expressions, including
        # their parameter identity. Equal values in different slots are not an
        # equivalent query shape in this SDK; preserve that original observation.
        original_sql = f"SELECT c.id,fts_score(c.search_terms,?) AS score {current} AND {support} AND fts_match(c.search_terms,?) ORDER BY score DESC LIMIT 1"
        sql = f"SELECT c.id,fts_score(c.search_terms,?1) AS score {numbered_current} AND {support} AND fts_match(c.search_terms,?1) ORDER BY score DESC LIMIT 1"
        if shape in {"materialized", "cte"}:
            hint = "MATERIALIZED " if shape == "materialized" else ""
            joined = numbered_current.replace("FROM oc_chunks c", "FROM oc_chunks c JOIN scored s ON s.tenant_id=c.tenant_id AND s.workspace_id=c.workspace_id AND s.id=c.id", 1)
            sql = f"WITH scored AS {hint}(SELECT tenant_id,workspace_id,id,fts_score(search_terms,?1) AS score FROM oc_chunks WHERE fts_match(search_terms,?1)) SELECT c.id,s.score {joined} AND {support} ORDER BY s.score DESC,c.id LIMIT 1"
        db.execute("BEGIN IMMEDIATE")
        db.execute("UPDATE oc_chunks SET search_terms=? WHERE id=?", ["alpha " + "filler " * 100, C])
        winner = blob(65)
        db.execute(f"INSERT INTO oc_chunks({CHUNK_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?)", [T, W, winner, A, 1, 65, "eligible stronger", "{}", "alpha alpha " + "filler " * 5])
        # Stronger native competitors must lose eligibility before top-k. These
        # represent another tenant, another workspace and a withdrawn source.
        for number, tenant, workspace, state in [(701, blob(701), blob(702), "active"), (711, T, blob(712), "active"), (721, T, W, "retracted")]:
            event, asset, chunk = blob(number + 2), blob(number + 3), blob(number + 4)
            if workspace != W:
                db.execute("INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES (?,?,?,?)", [tenant, workspace, "distractor", 1])
            db.execute("INSERT INTO oc_events(tenant_id,workspace_id,id,content,kind,state,created_by,created_at) VALUES (?,?,?,?,?,?,?,?)", [tenant, workspace, event, "synthetic distractor", "text", state, P, 1])
            db.execute("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,current_version,created_at) VALUES (?,?,?,?,?,?,?)", [tenant, workspace, asset, "knowledge", "distractor", 1, 1])
            db.execute("INSERT INTO oc_versions(tenant_id,workspace_id,asset_id,version,content,content_hash,source_event_id,title,created_by,created_at) VALUES (?,?,?,?,?,?,?,?,?,?)", [tenant, workspace, asset, 1, "distractor", "fixture", event, "distractor", P, 1])
            db.execute(f"INSERT INTO oc_chunks({CHUNK_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?)", [tenant, workspace, chunk, asset, 1, 0, "distractor", "{}", "alpha alpha alpha"])
        unfiltered = db.query("SELECT id FROM oc_chunks WHERE fts_match(search_terms,'alpha') ORDER BY fts_score(search_terms,'alpha') DESC LIMIT 1")
        assert unfiltered and unfiltered[0][0] not in [C, winner], unfiltered
        original_rows = db.query(original_sql, ["alpha", T, W, "alpha"])
        plan = db.query("EXPLAIN QUERY PLAN " + sql, ["alpha", T, W])
        assert "QUERY INDEX METHOD fts" in json.dumps(plan), plan
        vm_plan = db.query("EXPLAIN " + sql, ["alpha", T, W])
        rows = db.query(sql, ["alpha", T, W])
        direct = db.query("SELECT id,fts_score(search_terms,?1) AS score FROM oc_chunks WHERE fts_match(search_terms,?1)", ["alpha"])
        distinct_parameters = db.query("SELECT id,fts_score(search_terms,?1) AS score FROM oc_chunks WHERE fts_match(search_terms,?2)", ["alpha", "alpha"])
        direct_score = next(row[1] for row in direct if row[0] == winner)
        original_score = next(row[1] for row in direct if row[0] == C)
        assert direct_score > original_score > 0, direct
        assert len(rows) == 1 and rows[0][0] == winner and rows[0][1] > 0 and abs(rows[0][1] - direct_score) <= 1e-6, {"shape": shape, "scoped": rows, "direct": direct, "direct_distinct_parameters": distinct_parameters, "plan": plan, "vm_plan": vm_plan}
        check_equal(db.query(sql, ["alpha", blob(999), W]), [])
        db.execute("UPDATE oc_events SET state='retracted' WHERE id=?", [E])
        check_equal(db.query(sql, ["alpha", T, W]), [])
        db.execute("ROLLBACK")
        return {"shape": shape, "native_scoped_topk": rows, "direct_score": direct_score, "direct_distinct_parameters": distinct_parameters, "distinct_parameter_shape_observation": original_rows, "unfiltered_top1_distractor": unfiltered, "plan": plan, "vm_plan": vm_plan, "cross_tenant_and_retracted": 0}
    record("fts_real_visibility_join_before_topk", scoped)
    record("fts_materialized_score_visibility_candidate", lambda: scoped("materialized"))
    record("fts_plain_cte_score_visibility_candidate", lambda: scoped("cte"))
    return True


def crash_case(binary, directory, count, phase):
    directory.mkdir()
    path = directory / "context.db"
    db = Bridge(binary, path)
    try:
        initialize(db)
        seed(db)
        fts_setup(db)
        db.execute("BEGIN IMMEDIATE")
        values = []
        for n in range(count):
            values.extend([T, W, blob(1000 + n), A, 1, 1 + n, "crash boundary", "{}", "crashtoken"])
        db.execute(f"INSERT INTO oc_chunks({CHUNK_COLUMNS}) VALUES " + ",".join(["(?,?,?,?,?,?,?,?,?)"] * count), values)
        db.execute("INSERT INTO oc_summaries VALUES (?,?,?,?,?,?,?)", [T, W, blob(5000), blob(1000), "crash summary", "fixture", "summarytoken"])
        for uid in ["crash-head", "crash-tail"]:
            db.execute("INSERT INTO oc_graph_entities VALUES (?,?,?,?,?,?,?,?,?)", [uid, "t", "w", uid, uid, "entity", "synthetic", "source", 1])
        db.execute("INSERT INTO oc_graph_relations VALUES (?,?,?,?,?,?,?,?)", ["crash-edge", "t", "w", "crash-head", "crash-tail", "supports", "source", 1])
        db.execute("UPDATE oc_jobs SET state='completed' WHERE id=?", [J])
        db.execute("INSERT INTO oc_artifact_ledger(tenant_id,workspace_id,source_id,version,artifact_type,artifact_id,surface,generation,idempotency_key,state,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)", [T, W, E, 1, "chunk", blob(1000), "vector", 1, "synthetic", "committed", 1, 1])
        if phase != "pre_commit":
            db.execute("COMMIT")
        checkpoint = None
        if phase == "post_checkpoint":
            checkpoint = db.query("PRAGMA wal_checkpoint(FULL)")
            assert len(checkpoint) == 1 and len(checkpoint[0]) == 3, checkpoint
            busy, frames, completed = checkpoint[0]
            assert busy == 0 and frames == completed and frames >= 0, checkpoint
        db.kill()
        reopened = Bridge(binary, path, create=False)
        try:
            wanted = 0 if phase == "pre_commit" else count
            check_equal(reopened.query("SELECT count(*) FROM oc_chunks WHERE ordinal>0"), [[wanted]])
            check_equal(reopened.query("SELECT count(*) FROM oc_chunks WHERE fts_match(search_terms,'crashtoken')"), [[wanted]])
            check_equal(reopened.query("SELECT state FROM oc_jobs WHERE id=?", [J]), [["pending" if wanted == 0 else "completed"]])
            check_equal(reopened.query("SELECT count(*) FROM oc_artifact_ledger"), [[int(wanted > 0)]])
            check_equal(reopened.query("SELECT count(*) FROM oc_summaries"), [[int(wanted > 0)]])
            check_equal(reopened.query("SELECT count(*) FROM oc_summaries WHERE fts_match(search_terms,'summarytoken')"), [[int(wanted > 0)]])
            check_equal(reopened.query("SELECT count(*) FROM oc_graph_entities"), [[2 * int(wanted > 0)]])
            check_equal(reopened.query("SELECT count(*) FROM oc_graph_relations"), [[int(wanted > 0)]])
        finally:
            reopened.close()
        # Copy every stopped-file byte, not an assumed lone .db file.
        backup = directory.with_name(directory.name + "-backup")
        shutil.copytree(directory, backup)
        restored = Bridge(binary, backup / "context.db", create=False)
        try:
            check_equal(restored.query("SELECT count(*) FROM oc_chunks WHERE ordinal>0"), [[wanted]])
            check_equal(restored.query("SELECT count(*) FROM oc_chunks WHERE fts_match(search_terms,'crashtoken')"), [[wanted]])
            check_equal(restored.query("SELECT state FROM oc_jobs WHERE id=?", [J]), [["pending" if wanted == 0 else "completed"]])
            check_equal(restored.query("SELECT count(*) FROM oc_artifact_ledger"), [[int(wanted > 0)]])
            check_equal(restored.query("SELECT count(*) FROM oc_summaries"), [[int(wanted > 0)]])
            check_equal(restored.query("SELECT count(*) FROM oc_summaries WHERE fts_match(search_terms,'summarytoken')"), [[int(wanted > 0)]])
            check_equal(restored.query("SELECT count(*) FROM oc_graph_entities"), [[2 * int(wanted > 0)]])
            check_equal(restored.query("SELECT count(*) FROM oc_graph_relations"), [[int(wanted > 0)]])
        finally:
            restored.close()
        return {"rows": count, "phase": phase, "checkpoint": checkpoint, "expected_committed_rows": wanted, "backup_files": sorted(p.name for p in backup.iterdir()), "power_loss_or_mid_io": "NOT_RUN"}
    finally:
        if db.proc.poll() is None:
            db.kill()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--sqlite-baseline", action="store_true")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--crash-rows", default="999,1000,1001")
    args = parser.parse_args()
    assert args.sqlite_baseline != bool(args.binary), "choose exactly one engine"
    counts = [int(value) for value in args.crash_rows.split(",") if value]
    assert len(counts) <= 3 and all(1 <= n <= 1001 for n in counts), "bounded crash counts only"
    if args.binary:
        args.binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"baseline": BASELINE, "engine": "sqlite_fixture_only" if args.sqlite_baseline else "turso=0.8.2", "cases": [], "production_backend_changed": False, "real_models": "NOT_RUN", "arbitrary_io_faults": "NOT_RUN"}
    report["schema_sha256"] = {name: hashlib.sha256((ROOT / "src/storage" / name).read_bytes()).hexdigest() for name in SCHEMAS}
    current = [None]

    def record(name, fn):
        start = time.monotonic()
        try:
            details = fn()
            result = {"case": name, "status": "PASS", "details": details}
        except NotRun as error:
            result = {"case": name, "status": "NOT_RUN", "reason": str(error)}
        except Exception as error:
            result = {"case": name, "status": "FAIL", "error": str(error)}
            # Prevent one failed assertion leaving an open transaction and
            # contaminating independent later probes. A failed recovery remains
            # visible; it does not turn the original failure into a pass.
            if current[0] is not None:
                try:
                    result["rollback_recovery"] = current[0].raw("ROLLBACK", op="execute")
                except Exception as recovery_error:
                    result["rollback_recovery_error"] = str(recovery_error)
        result["elapsed_ms"] = (time.monotonic() - start) * 1000
        report["cases"].append(result)
        print(json.dumps(result, ensure_ascii=False), flush=True)
        (args.output / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
        return result["status"] == "PASS"

    # Pre-register even cases not reached after native startup/DDL failure.
    planned = re.findall(r'\brecord\("([^"]+)"', Path(__file__).read_text())
    planned += [f"crash_{count}_{phase}_backup_restore" for count in counts for phase in ["pre_commit", "post_commit", "post_checkpoint"]]
    report["planned_cases"] = list(dict.fromkeys(planned))
    (args.output / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    try:
        with tempfile.TemporaryDirectory(prefix="origence-t0-") as tmp:
            base = Path(tmp)
            seeded = False
            schema_reference = [None]

            def open_initial():
                current[0] = SQLite(base / "context.db") if args.sqlite_baseline else Bridge(args.binary, base / "context.db")
                return {"opened": True}

            try:
                if record("engine_open", open_initial):
                    db = current[0]
                    schema_ok = record("all_real_schemas_unmodified", lambda: {"statement_count": initialize(db)})
                    seeded = schema_ok and record("real_relational_fixture", lambda: (seed(db), {"seeded": True})[1])
                    if seeded:
                        relational_cases(db, record)
                        if not args.sqlite_baseline:
                            driver_cases(db, record)
                            fts_cases(db, record)
                        def capture_schema_metadata():
                            schema_reference[0] = schema_metadata(db)
                            return schema_reference[0]
                        record("canonical_schema_and_structure_before_close", capture_schema_metadata)
                if current[0] is not None:
                    db, current[0] = current[0], None
                    record("initial_clean_shutdown", lambda: (db.close(), {"closed": True})[1])
                if seeded and not args.sqlite_baseline:
                    def reopen_schema():
                        reopened = Bridge(args.binary, base / "context.db", create=False)
                        try:
                            return schema_roundtrip(reopened)
                        finally:
                            reopened.close()
                    record("same_version_reopen_self_schema", reopen_schema)
                    def reopen_canonical():
                        if schema_reference[0] is None:
                            raise NotRun("no prior schema metadata snapshot")
                        reopened = Bridge(args.binary, base / "context.db", create=False)
                        try:
                            observed = schema_metadata(reopened)
                            check_equal(observed, schema_reference[0])
                            return {"sha256": observed["sha256"], "sqlite_master_columns_fk_indexes_unchanged": True}
                        finally:
                            reopened.close()
                    record("same_version_reopen_canonical_schema_structure", reopen_canonical)
                    for count in counts:
                        for phase in ["pre_commit", "post_commit", "post_checkpoint"]:
                            record(f"crash_{count}_{phase}_backup_restore", lambda count=count, phase=phase: crash_case(args.binary, base / f"crash-{count}-{phase}", count, phase))
            finally:
                if current[0] is not None:
                    current[0].kill()
                    current[0] = None
                diagnostics = []
                for source in base.rglob("bridge.stderr"):
                    relative = source.relative_to(base)
                    target = args.output / "bridge-stderr" / relative
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with source.open("rb") as stream:
                        data = stream.read(1024 * 1024)
                    target.write_bytes(data)
                    diagnostics.append({"path": str(target.relative_to(args.output)), "source_bytes": source.stat().st_size, "retained_bytes": len(data), "truncated": source.stat().st_size > len(data)})
                report["bridge_diagnostics"] = diagnostics
    except Exception as error:
        report["cases"].append({"case": "harness_unhandled_failure", "status": "FAIL", "error": str(error)})
    attempted = {item["case"] for item in report["cases"]}
    for name in report["planned_cases"]:
        if name not in attempted:
            report["cases"].append({"case": name, "status": "NOT_RUN", "reason": "SQLite fixture mode or an earlier failure prevented execution; see attempted case records"})
    report["summary"] = {status: sum(c["status"] == status for c in report["cases"]) for status in ["PASS", "FAIL", "NOT_RUN"]}
    (args.output / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    return int(report["summary"]["FAIL"] > 0)


if __name__ == "__main__":
    raise SystemExit(main())
