"""Opt-in loopback generic JSON chat over a pinned existing local model snapshot.

Separate from evals/local_models.py: JSON requests are never graph extraction.
No downloads, provider keys, proxying, subprocesses or shared model-server changes.
"""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import platform
import time

ADAPTER_KIND = "s4-generic-json-chat-v1"
MAX_BODY = 100000
MAX_CONTEXT_BYTES = 48000
MAX_OUTPUT_TOKENS = 1024


def check_request(body, model):
    if not isinstance(body, dict) or body.get("model") != model:
        raise ValueError("invalid model")
    if set(body) - {"model", "messages", "temperature", "max_tokens", "seed", "response_format"}:
        raise ValueError("unknown request field")
    messages = body.get("messages")
    if (not isinstance(messages, list) or not 1 <= len(messages) <= 16
            or any(not isinstance(m, dict) or set(m) != {"role", "content"}
                   or m["role"] not in ("system", "user", "assistant") or not isinstance(m["content"], str)
                   for m in messages)
            or len(json.dumps(messages, ensure_ascii=False).encode()) > MAX_CONTEXT_BYTES):
        raise ValueError("invalid messages")
    limit = body.get("max_tokens")
    if type(limit) is not int or not 1 <= limit <= MAX_OUTPUT_TOKENS or body.get("temperature", 0) != 0:
        raise ValueError("invalid generation budget")
    response_format = body.get("response_format", {})
    if response_format.get("type") != "json_schema":
        raise ValueError("generic JSON schema required")
    schema = response_format.get("json_schema", {}).get("schema")
    if not isinstance(schema, dict) or schema.get("type") != "object" or len(json.dumps(schema)) > 16000:
        raise ValueError("bounded object schema required")
    return messages, limit, schema


def snapshot_hashes(path):
    root = Path(path)
    result = {}
    for file in sorted(root.rglob("*")):
        if file.is_file() and file.suffix in (".json", ".txt", ".model", ".safetensors", ".bin"):
            digest = hashlib.sha256()
            with file.open("rb") as stream:
                for block in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(block)
            result[str(file.relative_to(root))] = digest.hexdigest()
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-path", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--port", type=int, default=8099)
    parser.add_argument("--threads", type=int, default=2)
    parser.add_argument("--allow-local-inference", action="store_true")
    args = parser.parse_args(argv)
    if not args.allow_local_inference or os.environ.get("S4_ALLOW_LOCAL_MODEL_CALLS") != "true":
        parser.error("local inference requires explicit flag and S4_ALLOW_LOCAL_MODEL_CALLS=true")
    if not Path(args.model_path).is_dir() or not 1 <= args.threads <= 32 or not 1 <= args.port <= 65535:
        parser.error("existing local snapshot and valid resource limits required")
    # These are set before importing the optional runtime; no model fetch fallback.
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
    import torch
    import transformers
    from transformers import AutoModelForCausalLM, AutoTokenizer
    from lmformatenforcer import JsonSchemaParser
    from lmformatenforcer.integrations.transformers import build_transformers_prefix_allowed_tokens_fn
    torch.set_num_threads(args.threads)
    tokenizer = AutoTokenizer.from_pretrained(args.model_path, local_files_only=True, trust_remote_code=False)
    model = AutoModelForCausalLM.from_pretrained(args.model_path, local_files_only=True,
                                                trust_remote_code=False, use_safetensors=True,
                                                dtype=torch.float32).eval()
    metadata = {"adapter_kind": ADAPTER_KIND, "model": args.model, "revision": args.revision,
                "files_sha256": snapshot_hashes(args.model_path), "network_policy": "local_files_only",
                "generation": "greedy CPU float32; generic schema-constrained JSON; no cache",
                "runtime": {"torch": torch.__version__, "transformers": transformers.__version__,
                            "platform": platform.platform(), "threads": args.threads},
                "cost": {"provider_fee_usd": 0, "compute_cost_usd": None, "total_cost_usd": None},
                "limits": {"input_utf8_bytes": MAX_CONTEXT_BYTES, "output_tokens": MAX_OUTPUT_TOKENS},
                "usage": {"requests": 0, "prompt_tokens": 0, "completion_tokens": 0}}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *unused):
            pass

        def reply(self, code, body):
            data = json.dumps(body, ensure_ascii=False).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            self.reply(200, metadata) if self.path == "/metadata" else self.reply(404, {"error": "not_found"})

        def do_POST(self):
            if self.path != "/v1/chat/completions":
                self.reply(404, {"error": "not_found"})
                return
            try:
                size = int(self.headers.get("Content-Length", "0"))
                if not 0 < size <= MAX_BODY:
                    raise ValueError()
                body = json.loads(self.rfile.read(size))
                messages, limit, schema = check_request(body, args.model)
                started = time.perf_counter()
                inputs = tokenizer.apply_chat_template(messages, add_generation_prompt=True,
                                                       tokenize=True, return_dict=True, return_tensors="pt")
                prompt_tokens = inputs["input_ids"].shape[1]
                context_limit = getattr(model.config, "max_position_embeddings", 32768)
                if prompt_tokens + limit > context_limit:
                    raise ValueError()
                constraint = build_transformers_prefix_allowed_tokens_fn(tokenizer, JsonSchemaParser(schema))
                with torch.inference_mode():
                    generated = model.generate(**inputs, do_sample=False, max_new_tokens=limit,
                                               temperature=None, top_p=None, top_k=None,
                                               prefix_allowed_tokens_fn=constraint)
                output = generated[0, prompt_tokens:]
                content = tokenizer.decode(output, skip_special_tokens=True)
                metadata["usage"]["requests"] += 1
                metadata["usage"]["prompt_tokens"] += prompt_tokens
                metadata["usage"]["completion_tokens"] += len(output)
                self.reply(200, {"choices": [{"message": {"content": content}}],
                                 "usage": {"prompt_tokens": prompt_tokens, "completion_tokens": len(output)},
                                 "elapsed_ms": (time.perf_counter() - started) * 1000})
            except (ValueError, TypeError, KeyError, UnicodeError):
                self.reply(400, {"error": "invalid_model_request"})
            except Exception:
                # Never echo source text, paths or raw model/runtime exceptions to clients.
                self.reply(500, {"error": "local_generation_failed"})

    server = HTTPServer(("127.0.0.1", args.port), Handler)
    server.timeout = 60
    print(json.dumps({"listening": "127.0.0.1:" + str(args.port), "adapter_kind": ADAPTER_KIND}), flush=True)
    try:
        server.serve_forever()
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
