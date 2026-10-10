"""Adapter/metric tests only; fixture results are not semantic benchmark scores."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("eval_run", Path(__file__).with_name("run.py"))
RUN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUN)


class Metrics(unittest.TestCase):
    def test_quote_coverage_requires_current_cited_original_chunk(self):
        evidence = [{"source_id":"a","source_version":2,"quote":"原文证据"}]
        hits = [{"asset_id":"asset","version":2,"chunk_id":"chunk","content":"完整原文证据"}]
        mapping = {"asset":"a"}
        context = {"sources":[{"asset_id":"asset","version":2,"chunk_id":"chunk","citation":"[c]"}],
                   "rendered_context":"[c] 完整原文证据"}
        self.assertEqual(RUN.evidence_recall(hits, evidence, mapping, 5), 1)
        self.assertEqual(RUN.context_coverage(context, hits, evidence, mapping), 1)
        context["sources"][0]["version"] = 1
        self.assertEqual(RUN.context_coverage(context, hits, evidence, mapping), 0)
        context["sources"] = [{"entity_id":"e", "evidence":[{"asset_id":"asset","version":2}]}]
        self.assertEqual(RUN.context_coverage(context, hits, evidence, mapping), 0)
        self.assertIsNone(RUN.evidence_recall(hits, [], mapping, 5))

    def test_evidence_validation_rejects_untraceable_labels(self):
        corpus = [{"id":"a","content":"真实原文","source_version":1,"locator":"source-document"}]
        case = {"relevant_source_ids":["a"],"evidence":[{"source_id":"a","source_version":1,
                "locator":"source-document","quote":"真实"}]}
        RUN.validate_evidence(corpus, [case], True)
        for key, value in [("source_version",2),("quote","编造"),("locator","missing")]:
            bad = json.loads(json.dumps(case))
            bad["evidence"][0][key] = value
            with self.assertRaises(ValueError):
                RUN.validate_evidence(corpus, [bad], True)

    def test_deduplicates_chunks_before_document_cutoff(self):
        result = RUN.score(["a", "a", "b", "c"], ["b", "c"], 2)
        self.assertEqual(result["recall"], .5)
        self.assertEqual(result["mrr"], .5)
        self.assertGreater(result["ndcg"], 0)
        self.assertLess(result["ndcg"], 1)

    def test_all_required_sources(self):
        result = RUN.score(["a", "b"], ["a", "b"], 5)
        self.assertEqual(result["recall"], 1)
        self.assertEqual(result["ndcg"], 1)

    def test_no_answer_is_not_recall(self):
        self.assertIsNone(RUN.score([], [], 5)["recall"])
        self.assertFalse(RUN.score([], [], 5)["false_positive"])
        self.assertTrue(RUN.score(["a"], [], 5)["false_positive"])

    def test_no_hit_and_small_latency_sample(self):
        self.assertEqual(RUN.score([], ["a"], 5)["mrr"], 0)
        self.assertEqual(RUN.percentile([1, 2, 100], .95), 100)
        self.assertIsNone(RUN.percentile([], .95))


FAKE_HOST = '''#!/usr/bin/env python3
import json,os,sys
from http.server import HTTPServer,BaseHTTPRequestHandler
assert os.environ['OC_ENABLE_MODELS']=='false'
assert 'OC_API_KEY' not in os.environ and 'OC_MODEL_API_KEY' not in os.environ
if 'workspace-create' in sys.argv:
 print(json.dumps({'token':'fixture-token'}));sys.exit()
assets={}
class Handler(BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def reply(self,status,payload):
  self.send_response(status);self.end_headers();self.wfile.write(json.dumps(payload).encode())
 def do_GET(self):
  if self.path=='/health/ready':self.reply(200,{'status':'ready'})
  elif self.headers.get('Authorization')!='Bearer fixture-token':self.reply(401,{})
  else:self.reply(200,{'state':'completed','outcome':'published'})
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  if self.path=='/v1/knowledge':
   asset='asset-'+str(len(assets));assets[asset]=body
   self.reply(200,{'asset_id':asset,'job_id':'job'})
  elif body['query']=='error':self.reply(500,{})
  else:
   hits=[] if body['query']=='empty' else [{'asset_id':list(assets)[0]}]*2
   self.reply(200,{'hits':hits,'effective_mode':body['mode']})
server=HTTPServer(('127.0.0.1',0),Handler)
print(json.dumps({'listening':'127.0.0.1:'+str(server.server_port)}),flush=True)
server.serve_forever()
'''


@unittest.skipIf(os.name == "nt", "fixture executable uses a POSIX shebang; production driver supports Windows")
class Adapter(unittest.TestCase):
    def test_model_calls_and_matrix_require_explicit_configuration(self):
        args = ["python3", str(Path(__file__).with_name("run.py")), "--binary", "unused", "--output", "unused"]
        for flags in [["--mode","vector"], ["--matrix"]]:
            result = subprocess.run(args + flags, capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("require --allow-model-calls", result.stderr)

    def test_reports_errors_without_counting_them_as_abstentions(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            host = root / "host"
            host.write_text(FAKE_HOST)
            host.chmod(0o700)
            corpus = root / "corpus.jsonl"
            corpus.write_text(json.dumps({"id": "a", "content": "test document"}) + "\n")
            cases = root / "cases.jsonl"
            rows = [{"id": "hit", "query": "hit", "relevant_source_ids": ["a"]},
                    {"id": "none", "query": "empty", "relevant_source_ids": []},
                    {"id": "error", "query": "error", "relevant_source_ids": ["a"]},
                    {"id": "empty-error", "query": "error", "relevant_source_ids": []}]
            cases.write_text("".join(json.dumps(row) + "\n" for row in rows))
            env = dict(os.environ, OC_API_KEY="do-not-persist", OC_MODEL_API_KEY="do-not-persist")
            output = root / "result"
            result = subprocess.run(["python3", str(Path(__file__).with_name("run.py")),
                                     "--binary", str(host), "--corpus", str(corpus), "--cases", str(cases),
                                     "--output", str(output)], env=env, capture_output=True, text=True, timeout=20)
            self.assertNotEqual(result.returncode, 0)
            summary = json.loads((output / "summary.json").read_text())
            self.assertEqual(summary["document_recall_at_k"], .5)
            self.assertEqual(summary["errors"], 2)
            self.assertEqual(summary["no_answer_errors"], 1)
            self.assertEqual(summary["no_answer_false_positives"], 0)
            for path in output.iterdir():
                self.assertNotIn("do-not-persist", path.read_text())
                self.assertNotIn("fixture-token", path.read_text())


if __name__ == "__main__":
    unittest.main()
