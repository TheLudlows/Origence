"""Compare local Qdrant and LanceDB against saved S1 v2 Origence responses."""

import argparse
import hashlib
import http.client
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from urllib.request import ProxyHandler, Request, build_opener
from uuid import uuid4

import jieba
import lancedb
from lancedb.index import FTS
from lancedb.query import FullTextOperator, MatchQuery
from lancedb.rerankers import RRFReranker
import pyarrow as pa

from run import score, validate_evidence

MODES = ("keyword", "vector", "hybrid-original")
BUDGET = 2000
LIMIT = 100
TOP_K = 5


def jsonl(path):
    return [json.loads(line) for line in path.read_bytes().splitlines()]

def segment(tokenizer, text):
    return " ".join(word.lower() for word in tokenizer.cut_for_search(text, HMM=False)
                    if any(char.isalnum() for char in word))

def render_block(hit):
    marker = f'[{hit["asset_id"]}:{hit["version"]}:{hit["chunk_id"]}]'
    identity = json.dumps({"identity": hit["identity"], "kind": hit["kind"],
                           "normalization_status": hit["normalization_status"]},
                          ensure_ascii=False, separators=(",", ":"))
    return f'{marker} {hit["title"]}\n{identity}\n{hit["content"]}\n\n'


def embed_dataset(model_path, config, docs, cases, directory):
    command = [
        sys.executable, str(Path(__file__).with_name("local_models.py")),
        "--embedding-path", str(model_path), "--embedding-model", config["embedding"],
        "--embedding-revision", config["embedding_revision"], "--port", "0",
        "--cache", str(directory / "cache"), "--metadata", str(directory / "models.json"),
    ]
    opener = build_opener(ProxyHandler({}))
    with subprocess.Popen(command, stdout=subprocess.PIPE, text=True) as provider:
        try:
            ready = provider.stdout.readline()
            if not ready:
                raise RuntimeError("local embedding server exited before readiness")
            address = json.loads(ready)["listening"]

            def embed(text):
                body = {"input": text, "model": config["embedding"], "dimensions": 512}
                request = Request(f"http://{address}/v1/embeddings", json.dumps(body).encode(),
                                  {"Content-Type": "application/json"})
                with opener.open(request, timeout=60) as response:
                    return json.load(response)["data"][0]["embedding"]

            vectors = {"documents": {doc["id"]: embed(doc["content"]) for doc in docs},
                       "queries": {case["id"]: embed(case["query"]) for case in cases}}
            return vectors, json.loads((directory / "models.json").read_bytes())
        finally:
            provider.terminate()
            provider.wait(timeout=20)


class Qdrant:
    def __init__(self, port, corpus, vector_by_doc, segmented):
        self.connection = http.client.HTTPConnection("127.0.0.1", port=port, timeout=15)
        self.version = self.call("GET", "/")["version"]
        self.collection = "/collections/s1_v2_competition_" + uuid4().hex
        self.call("PUT", self.collection, {"vectors": {"dense": {"size": 512, "distance": "Cosine"}}})
        self.call("PUT", self.collection + "/index?wait=true", {"field_name": "terms", "field_schema": {
            "type": "text", "scoring": {"type": "bm25"}, "tokenizer": "whitespace", "lowercase": True,
            "min_token_len": 1, "max_token_len": 120,
        }})
        points = [{"id": index, "vector": {"dense": vector_by_doc[source_id]},
                   "payload": {"id": source_id, "version": doc["source_version"], "terms": segmented[source_id]}}
                  for index, (source_id, doc) in enumerate(corpus.items())]
        self.call("PUT", self.collection + "/points?wait=true", {"points": points})

    def call(self, method, path, body=None):
        self.connection.request(method, path, json.dumps(body) if body is not None else None,
                                {"Content-Type": "application/json"})
        response = self.connection.getresponse()
        raw = response.read()
        if response.status != 200:
            raise RuntimeError(f"Qdrant {response.status}: {raw.decode()[:500]}")
        return json.loads(raw)

    def search(self, mode, text, vector):
        keyword = {"query": {"text": text}, "using": "terms", "limit": LIMIT}
        dense = {"query": vector, "using": "dense", "params": {"exact": True}, "limit": LIMIT}
        branches = {"keyword": keyword, "vector": dense, "hybrid-original": {
            "prefetch": [keyword, dense], "query": {"rrf": {"k": 61}}, "limit": LIMIT}}
        result = self.call("POST", self.collection + "/points/query", branches[mode] | {"with_payload": True})
        return [(p["payload"]["id"], p["payload"]["version"], p["score"]) for p in result["result"]["points"]]


class Lance:
    def __init__(self, directory, corpus, vector_by_doc, segmented):
        self.version = lancedb.__version__
        table = pa.table({
            "id": list(corpus), "version": [d["source_version"] for d in corpus.values()],
            "terms": [segmented[key] for key in corpus],
            "vector": pa.array([vector_by_doc[key] for key in corpus], type=pa.list_(pa.float32(), 512)),
        })
        self.table = lancedb.connect(directory).create_table("s1_v2", table)
        self.table.create_index("terms", config=FTS(base_tokenizer="whitespace", stem=False,
                                                    remove_stop_words=False, ascii_folding=False,
                                                    max_token_length=120))

    def search(self, mode, text, vector, operator=FullTextOperator.OR):
        query = MatchQuery(text, "terms", operator=operator)
        if mode == "keyword":
            builder = self.table.search(query, query_type="fts")
            score_key = "_score"
        elif mode == "vector":
            builder = self.table.search(vector).distance_type("cosine").bypass_vector_index()
            score_key = "_distance"
        else:
            builder = (self.table.search(query_type="hybrid")
                       .text(query).vector(vector).distance_type("cosine").bypass_vector_index()
                       .rerank(RRFReranker(K=60)))
            score_key = "_relevance_score"
        return [(r["id"], r["version"], r[score_key]) for r in builder.limit(LIMIT).to_list()]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, default=Path(__file__).parent / "retrieval/v2")
    parser.add_argument("--baseline", required=True, type=Path)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--vectors", type=Path, help="reuse vectors.json from a previous run")
    source.add_argument("--embedding-path", type=Path, help="local pinned BGE snapshot used for both competitors")
    parser.add_argument("--qdrant-port", required=True, type=int, help="port of a disposable loopback Qdrant")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    dataset = args.dataset
    manifest = json.loads((dataset / "manifest.json").read_bytes())
    for name, digest in manifest["sha256"].items():
        assert hashlib.sha256((dataset / name).read_bytes()).hexdigest() == digest
    docs, cases = (jsonl(dataset / name) for name in ("corpus.jsonl", "cases.jsonl"))
    validate_evidence(docs, cases, required=True)
    corpus = {doc["id"]: doc for doc in docs}
    baseline = {mode: jsonl(args.baseline / mode / "results.jsonl") for mode in MODES}
    imports = json.loads((args.baseline / "imports.json").read_bytes())
    asset_to_doc = {row["asset_id"]: row["source_id"] for row in imports}
    hits = baseline["vector"][0]["response"]["hits"]
    assert len(hits) == len(asset_to_doc) == len(docs) == 36
    frozen_hits = {asset_to_doc[hit["asset_id"]]: hit for hit in hits}
    for key, doc in corpus.items():
        hit = frozen_hits[key]
        assert (hit["version"], hit["title"], hit["content"]) == (doc["source_version"], doc["title"], doc["content"])
    blocks = {key: render_block(hit) for key, hit in frozen_hits.items()}
    for mode, records in baseline.items():
        assert len(records) == len(cases) == 300
        for row, case in zip(records, cases):
            assert row["id"] == case["id"] and row["query"] == case["query"]
            assert row["status"] == row["resolve_status"] == "ok"
            text = "".join(blocks[asset_to_doc[source["asset_id"]]] for source in row["context"]["sources"])
            assert text == row["context"]["rendered_context"]
    args.output.mkdir()
    with tempfile.TemporaryDirectory(prefix="lance-", dir=args.output.parent) as temp:
        if args.embedding_path is not None:
            vectors, metadata = embed_dataset(args.embedding_path, manifest["fixed_experiment"], docs, cases,
                                              Path(temp))
            (args.output / "vectors.json").write_text(json.dumps(vectors, separators=(",", ":")) + "\n",
                                                       encoding="utf-8")
            (args.output / "models.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
        else:
            vectors = json.loads(args.vectors.read_bytes())
        assert set(vectors["documents"]) == set(corpus)
        assert set(vectors["queries"]) == {case["id"] for case in cases}
        tokenizer = jieba.Tokenizer()
        tokenizer.tmp_dir = temp
        segmented = {source_id: segment(tokenizer, doc["content"]) for source_id, doc in corpus.items()}
        qdrant = Qdrant(args.qdrant_port, corpus, vectors["documents"], segmented)
        lance = Lance(temp, corpus, vectors["documents"], segmented)
        engines = [("Qdrant", qdrant), ("LanceDB", lance)]
        results = []
        strict_and = []
        for index, case in enumerate(cases):
            terms = segment(tokenizer, case["query"])
            query_vector = vectors["queries"][case["id"]]
            gold = case["relevant_source_ids"]
            evidence = case.get("evidence", [])
            strict_ranked = [key for key, _, _ in lance.search("keyword", terms, query_vector,
                                                              operator=FullTextOperator.AND)[:TOP_K]]
            strict_and.append({"id": case["id"], "split": case["split"], "ranked": strict_ranked,
                               "gold": gold, "recall": score(strict_ranked, gold, TOP_K)["recall"],
                               "matches_origence": strict_ranked == baseline["keyword"][index]["ranked_source_ids"]})
            for mode in MODES:
                original = baseline[mode][index]
                candidates = [(asset_to_doc[hit["asset_id"]], hit["version"], hit["score"])
                              for hit in original["response"]["hits"]]
                matches = [("Origence", candidates)]
                matches.extend((name, engine.search(mode, terms, query_vector)) for name, engine in engines)
                for name, ranked in matches:
                    ranked_ids = [source_id for source_id, _, _ in ranked]
                    by_source = {source_id: version for source_id, version, _ in ranked}
                    assert len(ranked_ids) == len(by_source)
                    assert all(by_source[key] == corpus[key]["source_version"] for key in ranked_ids)
                    if mode != "keyword":
                        assert len(ranked_ids) == len(docs)
                    def quote_coverage(ids):
                        if not evidence:
                            return None
                        return sum(item["source_id"] in ids
                                   and by_source[item["source_id"]] == item["source_version"]
                                   and item["quote"] in corpus[item["source_id"]]["content"]
                                   for item in evidence) / len(evidence)
                    context_ids, used = [], 0
                    if name == "Origence":
                        context_ids = [asset_to_doc[source["asset_id"]] for source in original["context"]["sources"]]
                        used = original["context_bytes"]
                    else:
                        for key in ranked_ids:
                            size = len(blocks[key].encode("utf-8"))
                            if used + size <= BUDGET:
                                used += size
                                context_ids.append(key)
                    row = {"engine": name, "mode": mode, "id": case["id"], "split": case["split"],
                           "category": case["category"], "query": case["query"], "gold": gold,
                           "ranked": ranked[:TOP_K], "context": context_ids, "context_bytes": used,
                           "evidence_recall": quote_coverage(set(ranked_ids[:TOP_K])),
                           "coverage": quote_coverage(set(context_ids)), **score(ranked_ids, gold, TOP_K)}
                    if name == "Origence":
                        assert all(original[key] == row[key] for key in ("recall", "mrr", "ndcg", "evidence_recall", "coverage", "false_positive"))
                    results.append(row)
        versions = {name: engine.version for name, engine in engines}
        qdrant.connection.close()
    summary = {}
    for engine in ("Origence", "Qdrant", "LanceDB"):
        summary[engine] = {}
        for mode in MODES:
            groups = {}
            for split in ("all", "development", "heldout"):
                rows = [r for r in results if r["engine"] == engine and r["mode"] == mode
                        and (split == "all" or r["split"] == split)]
                answers = [r for r in rows if r["gold"]]
                negatives = [r for r in rows if not r["gold"]]
                groups[split] = {key: sum(r[key] for r in answers) / len(answers)
                                 for key in ("recall", "mrr", "ndcg", "evidence_recall", "coverage")}
                groups[split].update(answerable=len(answers), unanswerable=len(negatives),
                                     false_positive=sum(r["false_positive"] for r in negatives),
                                     no_answer_context=sum(bool(r["context"]) for r in negatives))
            summary[engine][mode] = groups
    (args.output / "rows.jsonl").write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in results), encoding="utf-8")
    (args.output / "summary.json").write_text(json.dumps({"versions": versions, "summary": summary}, indent=2) + "\n",
                                             encoding="utf-8")
    (args.output / "lancedb-strict-and.json").write_text(json.dumps(strict_and, ensure_ascii=False, indent=2) + "\n",
                                                        encoding="utf-8")
    print(json.dumps(versions), len(results), "rows")

if __name__ == "__main__":
    main()
