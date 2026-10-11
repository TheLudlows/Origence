"""Process-death recovery against the real host, with a controlled model stall."""
from contextlib import closing
from concurrent.futures import ThreadPoolExecutor
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import sqlite3
import tempfile
import threading
import unittest
import aml_drill


@unittest.skipUnless(os.environ.get("AML_TEST_BINARY"), "set AML_TEST_BINARY to test the built host")
class RecoveryTests(unittest.TestCase):
    def test_process_death_during_embedding_reuses_original_batch(self):
        entered = threading.Event()
        release = threading.Event()
        calls = []
        class Model(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass
            def do_POST(self):
                self.rfile.read(int(self.headers["Content-Length"]))
                calls.append(1)
                if len(calls) == 1:
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
                    host.call("/admin/aml/namespace", {})
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
                        accepted_job = db.execute("SELECT job_id FROM oc_aml_adds").fetchone()[0]
                    host.start()
                    self.assertTrue(host.call("/aml/add", batch)[0]["success"])
                    first = host.call("/aml/search", {"user_id": "Alice", "query": "Atlas", "top_k": 100})[0]
                    self.assertEqual(len(first["data"]), 2)
                    self.assertTrue(host.call("/aml/add", batch)[0]["success"])
                    self.assertEqual(host.call("/aml/search", {"user_id": "Alice", "query": "Atlas", "top_k": 100})[0], first)
                    host.stop()
                    with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as db:
                        self.assertEqual(db.execute("SELECT job_id FROM oc_aml_adds").fetchall(), [(accepted_job,)])
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


if __name__ == "__main__":
    unittest.main()
