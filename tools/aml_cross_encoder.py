"""Offline cross-encoder regression over frozen original candidates; no host changes."""
import argparse
import importlib.metadata
import json
import math
from pathlib import Path
import time
from aml_diagnose import DATASETS, load_dataset, read_json
from aml_drill import DrillError, percentiles
from aml_quality import decode_hits, digest, score, source_key, summarize

MODEL_ID = "BAAI/bge-reranker-v2-m3"
REVISION = "953dc6f6f85a1b2dbfca4c34a2796e7dde08d41e"
MODEL_FILES = {"config.json", "model.safetensors", "sentencepiece.bpe.model",
               "special_tokens_map.json", "tokenizer.json", "tokenizer_config.json"}


def rank_indices(scores, depth):
    if not scores or len(scores) > 100 or depth not in (40, 100):
        raise DrillError("invalid_candidate_depth")
    if any(type(x) not in (float, int) or not math.isfinite(x) for x in scores):
        raise DrillError("invalid_cross_encoder_scores")
    return sorted(range(min(depth, len(scores))), key=lambda i: (-scores[i], i))


def check_lengths(lengths, maximum=512):
    if not lengths or any(type(n) is not int or n <= 0 or n > maximum for n in lengths):
        raise DrillError("cross_encoder_input_limit")


def verify_model(directory, manifest):
    if manifest.get("model") != MODEL_ID or manifest.get("revision") != REVISION:
        raise DrillError("model_revision_mismatch")
    files = manifest.get("files", [])
    if len(files) != len(MODEL_FILES) or {x["name"] for x in files} != MODEL_FILES:
        raise DrillError("invalid_model_manifest")
    for entry in files:
        path = directory / entry["name"]
        if path.stat().st_size != entry["bytes"] or digest(path) != entry["sha256"]:
            raise DrillError("model_integrity_mismatch")


class LocalReranker:
    def __init__(self, directory, device, batch_size, report, max_pair_tokens=512):
        import torch
        import transformers
        from transformers import AutoModelForSequenceClassification, AutoTokenizer
        if max_pair_tokens not in (512, 1024):
            raise DrillError("invalid_pair_limit")
        self.max_pair_tokens = max_pair_tokens
        self.torch, self.device, self.batch_size = torch, device, batch_size
        torch.set_num_threads(4)
        torch.manual_seed(0)
        if device == "cuda" and not torch.cuda.is_available():
            raise DrillError("requested_cuda_unavailable")
        self.tokenizer = AutoTokenizer.from_pretrained(directory, local_files_only=True, trust_remote_code=False)
        dtype = torch.float16 if device == "cuda" else torch.float32
        started = time.perf_counter()
        self.model = AutoModelForSequenceClassification.from_pretrained(
            directory, local_files_only=True, trust_remote_code=False, use_safetensors=True, torch_dtype=dtype)
        self.model.to(device).eval()
        if device == "cuda":
            torch.cuda.synchronize()
            torch.cuda.reset_peak_memory_stats()
        report["runtime"] = {"torch": torch.__version__, "transformers": transformers.__version__,
            "device": device, "dtype": str(dtype), "batch_size": batch_size, "max_pair_tokens": max_pair_tokens,
            "truncation": False, "threads": 4, "seed": 0, "load_ms": round((time.perf_counter() - started) * 1000, 2),
            "cuda_build": torch.version.cuda, "gpu": torch.cuda.get_device_name(0) if device == "cuda" else None,
            "packages": {p: importlib.metadata.version(p) for p in ("tokenizers", "safetensors", "sentencepiece", "huggingface_hub")}}

    def predict(self, query, hits):
        if not hits or len(hits) > 100 or len(query.encode()) > 4000 or sum(len(h["content"].encode()) for h in hits) > 100000:
            raise DrillError("cross_encoder_candidate_limit")
        started = time.perf_counter()
        lengths, scores = [], []
        for offset in range(0, len(hits), self.batch_size):
            batch = hits[offset:offset + self.batch_size]
            pairs = [(query, h["content"]) for h in batch]
            encoded = self.tokenizer(pairs, padding=False, truncation=False)
            batch_lengths = [len(tokens) for tokens in encoded["input_ids"]]
            check_lengths(batch_lengths, self.max_pair_tokens)
            lengths.extend(batch_lengths)
            inputs = self.tokenizer.pad(encoded, padding=True, return_tensors="pt").to(self.device)
            with self.torch.inference_mode():
                values = self.model(**inputs, return_dict=True).logits.reshape(-1).float().cpu().tolist()
            if len(values) != len(batch):
                raise DrillError("cross_encoder_output_shape")
            scores.extend(values)
        rank_indices(scores, 100)  # Reject NaN/infinity before saving any ranking.
        if self.device == "cuda":
            self.torch.cuda.synchronize()
        return scores, lengths, round((time.perf_counter() - started) * 1000, 2)


def evaluate(args, report):
    manifest = read_json(args.model_manifest)
    verify_model(Path(args.model_dir), manifest)
    report["model"] = manifest
    report["model_manifest_sha256"] = digest(args.model_manifest)
    reranker = LocalReranker(args.model_dir, args.device, args.batch_size, report)
    report["datasets"] = {}
    for dataset in DATASETS if args.dataset == "both" else [args.dataset]:
        corpus, questions, _, rows, _, hashes = load_dataset(dataset)
        part = {"inputs": hashes, "queries": []}
        report["datasets"][dataset] = part
        for q in questions:
            hits = rows[q["id"]]["seed"]
            keys = decode_hits(hits, corpus["batches"], q["user_id"])
            logits, lengths, elapsed = reranker.predict(q["query"], hits)
            gold = [source_key(x) for x in q["required"]]
            rankings = {str(depth): rank_indices(logits, depth) for depth in (40, 100)}
            metrics = {str(depth): {str(k): score(gold, [keys[i] for i in ranking], k) for k in (5, 10, 20)}
                       for depth, ranking in rankings.items()}
            part["queries"].append({"id": q["id"], "scores": logits, "pair_token_lengths": lengths,
                "candidate_ids": [h["id"] for h in hits], "ranked_indices": rankings, "metrics": metrics,
                "rerank_top100_ms": elapsed, "historical_seed_ms": rows[q["id"]]["seed_ms"]})
            print(json.dumps({"completed": q["id"], "rerank_top100_ms": elapsed}), flush=True)
        part["summary"] = {str(depth): {str(k): summarize(
            [{"metrics": r["metrics"][str(depth)]} for r in part["queries"]], k) for k in (5, 10, 20)} for depth in (40, 100)}
        part["rerank_top100_latency"] = percentiles([r["rerank_top100_ms"] for r in part["queries"]])
    if args.device == "cuda":
        report["gpu_memory"] = {"peak_allocated_bytes": reranker.torch.cuda.max_memory_allocated(),
                                "peak_reserved_bytes": reranker.torch.cuda.max_memory_reserved()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", required=True)
    parser.add_argument("--model-manifest", required=True)
    parser.add_argument("--report", required=True)
    parser.add_argument("--dataset", choices=(*DATASETS, "both"), default="both")
    parser.add_argument("--device", choices=("cpu", "cuda"), default="cuda")
    parser.add_argument("--batch-size", type=int, choices=range(1, 17), default=8)
    args = parser.parse_args()
    report = {"schema": "aml-cross-encoder-regression-v1", "status": "running", "runner_sha256": digest(__file__),
              "diagnosis_adapter_sha256": digest(Path(__file__).with_name("aml_diagnose.py")),
              "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "limitations": ["already-observed synthetic regression data; not unseen evaluation or official score",
                              "scores are relevance logits, not calibrated answerability probabilities",
                              "40-candidate rankings derived from top100 pair scores; only top100 inference latency measured",
                              "first inference included, no concurrent load; historical embedding latency is not current end-to-end latency",
                              "count cutoffs only, not a controlled reader-token-budget evaluation; no no-answer threshold",
                              "GPU allocator peak excludes other processes and host RAM; not a total peak-memory measurement"]}
    with open(args.report, "x", encoding="utf-8") as stream:
        try:
            evaluate(args, report)
            report["status"] = "completed"
        except DrillError as error:
            report.update(status="failed", error=str(error))
        except Exception as error:
            report.update(status="failed", error="cross_encoder_failed", error_type=type(error).__name__)
        finally:
            json.dump(report, stream, ensure_ascii=False, indent=2)
            stream.write("\n")
    print(json.dumps({"status": report["status"], "error": report.get("error"), "error_type": report.get("error_type")}))
    return 0 if report["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
