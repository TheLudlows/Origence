"""Real-host process recovery and stopped-host backup with controlled local models."""
from contextlib import closing
from concurrent.futures import ThreadPoolExecutor
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import sqlite3
import shutil
import tempfile
import threading
import unittest
import uuid
import aml_drill


def controlled_model(*, stall_first=False):
    """Only loopback fixture calls; no external provider or model credentials."""
    entered = threading.Event()
    release = threading.Event()
    calls = []

    class Model(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            self.rfile.read(int(self.headers["Content-Length"]))
            calls.append(1)
            if stall_first and len(calls) == 1:
                entered.set()
                release.wait(30)
            try:
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(b'{"data":[{"embedding":[1,0.5]}]}')
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                pass

    model = ThreadingHTTPServer(("127.0.0.1", 0), Model)
    thread = threading.Thread(target=model.serve_forever, daemon=True)
    thread.start()
    return model, thread, entered, release, calls


def stopped_snapshot(database):
    """Capture immutable references from a stopped fixture's read-only database."""
    queries = {
        "commands": "SELECT response,request_hash,principal_id,key FROM oc_commands WHERE operation='message_batch.ingest' AND dedup_scope='workspace' ORDER BY key",
        "jobs": "SELECT id,asset_id,source_event_id,created_by,generation,state,payload FROM oc_jobs ORDER BY id",
        "assets": "SELECT id,current_version,deleted FROM oc_assets ORDER BY id",
        "sources": "SELECT id,kind,content,state,created_by FROM oc_events ORDER BY id",
        "versions": "SELECT asset_id,version,source_event_id,content_hash FROM oc_versions ORDER BY asset_id,version",
        "chunks": "SELECT id,asset_id,version,ordinal,content,locator FROM oc_chunks ORDER BY asset_id,version,ordinal",
    }
    with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as db:
        return {name: db.execute(query).fetchall() for name, query in queries.items()}


@unittest.skipUnless(os.environ.get("AML_TEST_BINARY"), "set AML_TEST_BINARY to test the built host")
class RecoveryTests(unittest.TestCase):
    def test_process_death_during_embedding_reuses_original_batch(self):
        model, thread, entered, release, _calls = controlled_model(stall_first=True)
        parent = Path(__file__).resolve().parents[1] / "target" / "aml-drills"
        parent.mkdir(parents=True, exist_ok=True)
        try:
            with tempfile.TemporaryDirectory(prefix="inflight-", dir=parent) as directory:
                self.assertEqual(Path(directory).resolve().parent, parent.resolve())
                env = {**os.environ, "OC_MODEL_BASE_URL": "http://127.0.0.1:" + str(model.server_port),
                       "OC_MODEL_API_KEY": "fixture", "OC_EMBEDDING_MODEL": "fixture", "OC_EMBEDDING_DIMENSION": "2"}
                host = aml_drill.Host(os.environ["AML_TEST_BINARY"], directory, env)
                try:
                    host.bootstrap()
                    host.start()
                    batch = {"request_id": "inflight", "user_id": "Alice", "session_id": "one",
                             "messages": [{"role": "user", "content": "Atlas requires approval. 原文🙂"},
                                          {"role": "assistant", "content": "Acknowledged. 已记录。"}]}
                    with ThreadPoolExecutor(max_workers=1) as pool:
                        original = pool.submit(host.call, "/aml/add", batch)
                        self.assertTrue(entered.wait(20), "worker did not enter model stage")
                        host.stop()
                        release.set()
                        with self.assertRaises(aml_drill.DrillError):
                            original.result(timeout=10)
                    # Confirm accepted work survived, with nothing partially published.
                    database = Path(directory) / "data" / "context.db"
                    with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as db:
                        self.assertEqual(db.execute("SELECT state FROM oc_jobs").fetchall(), [("processing",)])
                        self.assertEqual(db.execute("SELECT count(*) FROM oc_versions").fetchone()[0], 0)
                        receipt_text = db.execute("SELECT response FROM oc_commands WHERE operation='message_batch.ingest' AND dedup_scope='workspace'").fetchone()[0]
                        accepted = json.loads(receipt_text)
                        accepted_job = accepted["job_id"]
                        self.assertEqual(db.execute("SELECT count(*) FROM sqlite_master WHERE name LIKE 'oc_aml_%'").fetchone()[0], 0)
                    host.start()
                    self.assertTrue(host.call("/aml/add", batch)[0]["success"])
                    first = host.call("/aml/search", {"user_id": "Alice", "query": "Atlas", "top_k": 100})[0]
                    self.assertEqual(len(first["data"]), 2)
                    self.assertTrue(host.call("/aml/add", batch)[0]["success"])
                    self.assertEqual(host.call("/aml/search", {"user_id": "Alice", "query": "Atlas", "top_k": 100})[0], first)
                    host.stop()
                    with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as db:
                        receipts = db.execute("SELECT response FROM oc_commands WHERE operation='message_batch.ingest' AND dedup_scope='workspace'").fetchall()
                        self.assertEqual([json.loads(row[0])["job_id"] for row in receipts], [accepted_job])
                        self.assertEqual(json.loads(receipts[0][0]), accepted)
                        self.assertEqual(db.execute("SELECT state FROM oc_jobs").fetchall(), [("completed",)])
                        self.assertEqual(db.execute("SELECT count(*) FROM oc_versions").fetchone()[0], 1)
                        self.assertEqual(db.execute("SELECT count(*) FROM oc_chunks").fetchone()[0], 2)
                finally:
                    release.set()
                    host.stop()
            self.assertFalse(Path(directory).exists())
        finally:
            release.set()
            model.shutdown()
            model.server_close()
            thread.join(timeout=5)


    def test_stopped_host_backup_restore_preserves_receipt_and_retrievable_ids(self):
        model, thread, _entered, release, _calls = controlled_model()
        parent = Path(__file__).resolve().parents[1] / "target" / "aml-drills"
        parent.mkdir(parents=True, exist_ok=True)
        try:
            with tempfile.TemporaryDirectory(prefix="backup-restore-", dir=parent) as directory:
                created = Path(directory).resolve()
                self.assertEqual(created.parent, parent.resolve())
                env = {**os.environ, "OC_MODEL_BASE_URL": "http://127.0.0.1:" + str(model.server_port),
                       "OC_MODEL_API_KEY": "fixture", "OC_EMBEDDING_MODEL": "fixture", "OC_EMBEDDING_DIMENSION": "2"}
                host = aml_drill.Host(os.environ["AML_TEST_BINARY"], created, env)
                original_data = created / "data"
                backup_data = created / "backup"
                restored_data = created / "restored"
                try:
                    host.bootstrap()
                    host.start()
                    batch = {"request_id": "backup:完整", "user_id": "Alice:备份🙂", "session_id": "same-version",
                             "messages": [{"role": "user", "timestamp": -1,
                                           "content": "Atlas requires approval. 原文🙂 café é\n" * 160},
                                          {"role": "assistant", "content": "Acknowledged. 已记录。"}]}
                    query = {"user_id": batch["user_id"], "query": "Atlas", "top_k": 100}
                    accepted = host.call("/aml/add", batch)[0]
                    self.assertEqual(accepted, {"success": True, **{key: batch[key] for key in ("request_id", "user_id", "session_id")}})
                    first = host.call("/aml/search", query)[0]
                    self.assertGreaterEqual(len(first["data"]), 3)
                    # Stop waits for process exit before copying the entire data
                    # tree, including WAL and native indexes. This is never a
                    # live filesystem snapshot or a claim of power-loss safety.
                    host.stop()
                    self.assertIsNone(host.process)
                    before = stopped_snapshot(original_data / "context.db")
                    for table in ("commands", "jobs", "assets", "sources", "versions"):
                        self.assertEqual(len(before[table]), 1, table)
                    self.assertEqual(len(before["chunks"]), len(first["data"]))
                    receipt = json.loads(before["commands"][0][0])
                    self.assertEqual(receipt["batch_id"], batch["request_id"])
                    self.assertEqual(receipt["session_id"], batch["session_id"])
                    self.assertEqual(before["jobs"][0][0], uuid.UUID(receipt["job_id"]).bytes)
                    self.assertEqual(before["assets"][0][0], uuid.UUID(receipt["asset_id"]).bytes)
                    self.assertEqual(before["sources"][0][0], uuid.UUID(receipt["source_event_id"]).bytes)
                    self.assertEqual(before["jobs"][0][5], "completed")
                    self.assertEqual(before["assets"][0][1:], (receipt["target_version"], 0))
                    self.assertEqual({str(uuid.UUID(bytes=row[0])) for row in before["chunks"]},
                                     {hit["id"] for hit in first["data"]})
                    for hit in first["data"]:
                        metadata = json.loads(hit["content"].partition("\n")[0].removeprefix("Source metadata: "))
                        self.assertEqual(metadata["source_event_id"], receipt["source_event_id"])
                        self.assertEqual(metadata["external_batch_id"], receipt["batch_id"])
                    shutil.copytree(original_data, backup_data)
                    shutil.copytree(backup_data, restored_data)
                    self.assertEqual(stopped_snapshot(backup_data / "context.db"), before)
                    self.assertEqual(stopped_snapshot(restored_data / "context.db"), before)
                    # Same executable, model profile, and original key; restore
                    # creates no new root/key and must retrieve before any replay.
                    host.env["OC_DATA_DIR"] = str(restored_data)
                    host.start()
                    self.assertEqual(host.call("/aml/search", query)[0], first)
                    for _ in range(2):
                        self.assertEqual(host.call("/aml/add", batch)[0], accepted)
                        self.assertEqual(host.call("/aml/search", query)[0], first)
                    host.stop()
                    self.assertEqual(stopped_snapshot(restored_data / "context.db"), before)
                    self.assertEqual(stopped_snapshot(original_data / "context.db"), before)
                    self.assertEqual(stopped_snapshot(backup_data / "context.db"), before)
                finally:
                    host.stop()
            self.assertFalse(created.exists())
        finally:
            release.set()
            model.shutdown()
            model.server_close()
            thread.join(timeout=5)


if __name__ == "__main__":
    unittest.main()
