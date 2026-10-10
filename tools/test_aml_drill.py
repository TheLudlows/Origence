"""Safety and contract checks for the external-model drill client."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import threading
import unittest
from unittest.mock import patch
import aml_drill


class ClientTests(unittest.TestCase):
    def env(self):
        return {"OC_MODEL_BASE_URL": "https://example.invalid/v1", "OC_MODEL_API_KEY": "secret",
                "OC_EMBEDDING_MODEL": "fixture", "OC_EMBEDDING_DIMENSION": "2"}

    def test_embedding_shape_values_and_dimension(self):
        for vector in [[0, 0], [1], [1, float("nan")], [1, float("inf")], [True, 1], ["1", 2]]:
            with self.subTest(vector=vector), patch.object(aml_drill, "request", return_value={"data": [{"embedding": vector}]}):
                with self.assertRaisesRegex(aml_drill.DrillError, "invalid_embedding"):
                    aml_drill.preflight(self.env())
        with patch.object(aml_drill, "request", return_value={"data": [{"embedding": [1, .5]}]}):
            report = aml_drill.preflight(self.env())
            self.assertEqual(report["dimension"], 2)
            self.assertNotIn("secret", json.dumps(report))
            self.assertNotIn("example.invalid", json.dumps(report))

    def test_nonlocal_http_and_embedded_secrets_are_rejected_before_request(self):
        for base in ["http://example.invalid", "https://user:password@example.invalid", "https://example.invalid?key=secret"]:
            env = self.env()
            env["OC_MODEL_BASE_URL"] = base
            with patch.object(aml_drill, "request") as request:
                with self.assertRaises(aml_drill.DrillError):
                    aml_drill.preflight(env)
                request.assert_not_called()

    def test_provider_errors_and_redirects_never_expose_or_forward_credentials(self):
        visits = []
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass
            def do_GET(self):
                visits.append(self.path)
                if self.path == "/redirect":
                    self.send_response(302)
                    self.send_header("Location", "/destination")
                else:
                    self.send_response(503)
                self.end_headers()
                self.wfile.write(b"private source and secret provider diagnostic")
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            base = "http://127.0.0.1:" + str(server.server_port)
            for path, code in [("/error", "http_503"), ("/redirect", "http_302")]:
                with self.assertRaises(aml_drill.DrillError) as raised:
                    aml_drill.request(base, path, "secret")
                self.assertEqual(str(raised.exception), code)
            self.assertEqual(visits, ["/error", "/redirect"])
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_percentiles_use_nearest_rank(self):
        self.assertEqual(aml_drill.percentiles([]), {"count": 0})
        self.assertEqual(aml_drill.percentiles([10, 40, 20, 30]),
                         {"count": 4, "p50_ms": 20, "p95_ms": 40, "p99_ms": 40})


if __name__ == "__main__":
    unittest.main()
