"""Benchmark S1 v2 through the three native document search entry points."""

import argparse
import asyncio
import hashlib
import io
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.request import ProxyHandler, Request, build_opener

TOP_K = 5
BUDGET = 2000
MODEL = "BAAI/bge-small-zh-v1.5"
OPENER = build_opener(ProxyHandler({}))


def load_dataset(dataset):
    manifest = json.loads((dataset / "manifest.json").read_bytes())
    for name, digest in manifest["sha256"].items():
        if hashlib.sha256((dataset / name).read_bytes()).hexdigest() != digest:
            raise ValueError(f"Dataset hash mismatch: {name}")
    return (
        *(
            [json.loads(line) for line in (dataset / name).read_bytes().splitlines()]
            for name in ("corpus.jsonl", "cases.jsonl")
        ),
        manifest,
    )


def json_http(base, path, body=None, timeout=30):
    req = Request(
        base + path,
        None if body is None else json.dumps(body).encode(),
        {"Content-Type": "application/json"},
    )
    with OPENER.open(req, timeout=timeout) as res:
        return json.load(res)


def normalize(parts, corpus):
    ranked = {}
    for source, version, text in parts:
        if (
            source not in corpus
            or version != corpus[source]["source_version"]
            or not isinstance(text, str)
        ):
            raise ValueError("cannot map native hit to exact imported source")
        key = (source, version)
        if key not in ranked and len(ranked) == TOP_K:
            continue
        if key not in ranked:
            ranked[key] = []
        if text and text not in ranked[key]:
            ranked[key].append(text)
    hits = []
    used = 0
    for (source, version), texts in ranked.items():
        selected = []
        for text in texts:
            header = f"[{source}:{version}]\n" if not selected else ""
            raw = f"{header}{text}\n"
            size = len(raw.encode("utf-8"))
            if used + size <= BUDGET:
                selected.append(text)
                used += size
        hits.append(
            {
                "source_id": source,
                "source_version": version,
                "parts": texts,
                "selected": selected,
            }
        )
    return hits, used


def score(case, hits):
    relevant = set(case["relevant_source_ids"])
    gold = case.get("evidence", [])
    returned = {x["source_id"] for x in hits}
    covered = sum(
        any(
            hit["source_id"] == label["source_id"]
            and hit["source_version"] == label["source_version"]
            and any(label["quote"] in part for part in hit["selected"])
            for hit in hits
        )
        for label in gold
    )
    return {
        "recall": len(relevant & returned) / len(relevant) if relevant else None,
        "quote_coverage": covered / len(gold) if gold else None,
        "no_answer_retrieval": bool(hits) if not relevant else None,
        "no_answer_context": any(x["selected"] for x in hits) if not relevant else None,
    }


def summary(rows):
    groups = {}
    for split in ("all", "development", "heldout"):
        subset = [r for r in rows if split == "all" or r["split"] == split]
        positive = [r for r in subset if r["answerable"]]
        negative = [r for r in subset if not r["answerable"]]
        timing = sorted(r["elapsed_ms"] for r in subset if r["status"] == "ok")
        groups[split] = {
            "queries": len(subset),
            "answerable": len(positive),
            "no_answer": len(negative),
            "recall_at_5": (
                sum(r["recall"] or 0 for r in positive) / len(positive)
                if positive
                else None
            ),
            "quote_coverage": (
                sum(r["quote_coverage"] or 0 for r in positive) / len(positive)
                if positive
                else None
            ),
            "no_answer_false_retrieval": sum(
                r["no_answer_retrieval"] is True for r in negative
            ),
            "no_answer_context_returned": sum(
                r["no_answer_context"] is True for r in negative
            ),
            "no_answer_failures": sum(r["status"] != "ok" for r in negative),
            "success_p50_ms": (
                timing[math.ceil(len(timing) * 0.50) - 1] if timing else None
            ),
            "success_p95_ms": (
                timing[math.ceil(len(timing) * 0.95) - 1] if timing else None
            ),
            "statuses": {
                s: sum(r["status"] == s for r in subset)
                for s in sorted({r["status"] for r in subset})
            },
        }
    return groups


async def evaluate(args, docs, cases, search):
    corpus = {doc["id"]: doc for doc in docs}
    results = []
    with (args.output / "rows.jsonl").open("x", encoding="utf-8") as stream:
        for case in cases:
            start = time.perf_counter()
            try:
                raw = await search(case["query"])
            except (TimeoutError, asyncio.TimeoutError):
                status = "timeout"
            except HTTPError as error:
                status = f"http_{error.code}"
                if args.smoke:
                    print(error.read().decode(errors="replace"), file=sys.stderr)
            except URLError as error:
                status = (
                    "timeout"
                    if isinstance(error.reason, TimeoutError)
                    else "transport_error"
                )
            except Exception as error:
                status = f"request_{type(error).__name__}"
                if args.smoke:
                    print(repr(error), file=sys.stderr)
            else:
                status = "ok"
            elapsed = (time.perf_counter() - start) * 1000
            hits, used = [], 0
            if status == "ok":
                try:
                    hits, used = normalize(raw, corpus)
                except (ValueError, TypeError, KeyError) as error:
                    status = "mapping_error"
                    if args.smoke:
                        print(repr(error), repr(raw)[:2000], file=sys.stderr)
            row = {
                "id": case["id"],
                "split": case["split"],
                "category": case.get("category"),
                "answerable": bool(case["relevant_source_ids"]),
                "status": status,
                "elapsed_ms": elapsed,
                "hits": hits,
                "context_bytes": used,
                **score(case, hits),
            }
            if status != "ok":
                row["no_answer_retrieval"] = None
                row["no_answer_context"] = None
            stream.write(json.dumps(row, ensure_ascii=False) + "\n")
            stream.flush()
            results.append(row)
    return summary(results)


def origence(args, docs, cases):
    sys.path.insert(0, str(args.repo / "tools"))
    from mcp_smoke import Host, isolated_env

    with tempfile.TemporaryDirectory(prefix="s1-docs-origence-") as directory:
        env = isolated_env(directory)
        env.update(
            OC_ENABLE_MODELS="true",
            OC_MODEL_BASE_URL=args.model_url,
            OC_EMBEDDING_MODEL=MODEL,
            OC_EMBEDDING_DIMENSION="512",
        )
        for key in ("OC_ANYSEEK_URL", "OC_ANYSEEK_API_KEY"):
            env.pop(key, None)
        binary = str(args.binary.resolve())
        bootstrap = subprocess.run(
            [binary, "--offline", "workspace-create", "s1-public-docs"],
            env=env,
            capture_output=True,
            check=True,
            timeout=60,
        )
        token = json.loads(bootstrap.stdout)["token"]
        with Host(binary, env, token, [token], serve_args=("--local",)) as host:
            sources = {}
            for doc in docs:
                data = host.request(
                    "/v1/knowledge",
                    {
                        "title": doc["title"],
                        "content": doc["content"],
                        "format": "text",
                    },
                )
                deadline = time.monotonic() + 120
                while time.monotonic() < deadline:
                    job = host.request("/v1/jobs/" + data["job_id"])
                    if job["state"] == "completed" and job["outcome"] == "published":
                        break
                    if job["state"] in (
                        "failed",
                        "superseded",
                        "cancelled",
                        "completed",
                    ):
                        raise RuntimeError("Origence publication failed")
                    time.sleep(0.1)
                else:
                    raise TimeoutError("Origence publication timed out")
                sources[data["asset_id"]] = doc["id"]

            async def search(query):
                response = host.request(
                    "/v1/search",
                    {
                        "query": query,
                        "mode": "hybrid",
                        "limit": 100,
                        "components": {"summaries": False, "graph": False},
                        "allow_partial": False,
                    },
                )
                if response["effective_mode"] != "hybrid":
                    raise ValueError("unexpected Origence search mode")
                return [
                    (sources.get(hit["asset_id"]), hit["version"], hit["content"])
                    for hit in response["hits"]
                ]

            return asyncio.run(evaluate(args, docs, cases, search))


def hindsight(args, docs, cases):
    prefix = "/v1/default/knowledge-banks"
    bank = "origence-152-s1-docs-" + args.output.name
    json_http(args.hindsight_url, prefix, {"bank_id": bank})
    scoped = prefix + "/" + bank
    items = [
        {"document_id": d["id"], "text": d["content"], "filename": d["id"] + ".txt"}
        for d in docs
    ]
    info = json_http(
        args.hindsight_url,
        scoped + "/ingest",
        {"items": items, "async": False},
        timeout=120,
    )
    if set(info["document_ids"]) != {d["id"] for d in docs}:
        raise ValueError("Hindsight did not ingest all source IDs")
    ready = json_http(args.hindsight_url, scoped + "/documents?limit=100")
    if ready["total"] != len(docs) or any(
        doc["status"] != "ready" for doc in ready["documents"]
    ):
        raise ValueError("Hindsight source readiness incomplete")

    async def search(query):
        result = json_http(
            args.hindsight_url,
            scoped + "/search",
            {
                "query": query,
                "max_tokens": 12000,
                "budget": "high",
                "enable_reranking": False,
            },
        )
        return [(hit["document_id"], 1, hit["text"]) for hit in result["results"]]

    return asyncio.run(evaluate(args, docs, cases, search))


async def cognee_run(args, docs, cases):
    import cognee
    from starlette.datastructures import UploadFile
    from cognee.api.v1.cognify.cognify import get_indexing_tasks
    from cognee.modules.pipelines.operations.pipeline import run_pipeline

    name = "s1_public_docs"
    files = [
        UploadFile(io.BytesIO(doc["content"].encode()), filename=doc["id"] + ".txt")
        for doc in docs
    ]
    await cognee.add(files, dataset_name=name)
    tasks = await get_indexing_tasks()
    completed = False
    async for event in run_pipeline(
        tasks=tasks, datasets=[name], pipeline_name="indexing_pipeline"
    ):
        print("cognee index", type(event).__name__, flush=True)
        completed = type(event).__name__ == "PipelineRunCompleted"
    if not completed:
        raise RuntimeError("Cognee chunks index failed")

    async def search(query):
        items = await asyncio.wait_for(
            cognee.search(
                query, query_type=cognee.SearchType.CHUNKS, datasets=[name], top_k=100
            ),
            timeout=30,
        )
        output = []
        for item in items:
            if isinstance(item, dict) and "search_result" in item:
                hit_rows = item["search_result"]
            else:
                hit_rows = item.search_result
            for hit in hit_rows:
                source_id = Path(hit["document_name"]).stem
                output.append((source_id, 1, hit["text"]))
        return output

    return await evaluate(args, docs, cases, search)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("engine", choices=["origence", "hindsight", "cognee"])
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model-url", required=True)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--hindsight-url")
    parser.add_argument("--smoke", action="store_true")
    args = parser.parse_args()
    docs, cases, manifest = load_dataset(args.repo / "evals/retrieval/v2")
    if args.smoke:
        docs, cases = docs[:2], cases[:1]
    args.output.mkdir(parents=True)
    initial = json_http(args.model_url.removesuffix("/v1"), "/metadata")["usage"]
    start = time.perf_counter()
    if args.engine == "origence":
        report = origence(args, docs, cases)
    elif args.engine == "hindsight":
        report = hindsight(args, docs, cases)
    else:
        report = asyncio.run(cognee_run(args, docs, cases))
    model = json_http(args.model_url.removesuffix("/v1"), "/metadata")
    output = {
        "engine": args.engine,
        "dataset": manifest["sha256"],
        "top_k": TOP_K,
        "budget_bytes": BUDGET,
        "elapsed_s": time.perf_counter() - start,
        "embedding": model["embedding"],
        "usage": {key: model["usage"][key] - value for key, value in initial.items()},
        "groups": report,
    }
    (args.output / "summary.json").write_text(
        json.dumps(output, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(output["groups"], ensure_ascii=False))


if __name__ == "__main__":
    main()
