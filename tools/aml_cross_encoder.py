"""Offline checks of saved reranker scores and pinned model files; no inference."""
import math
from aml_drill import DrillError
from aml_quality import digest

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


def selected_candidates(row, seed, depth):
    if row["candidate_ids"] != [hit["id"] for hit in seed] or len(row["scores"]) != len(seed):
        raise DrillError("cross_encoder_candidates_mismatch")
    expected = rank_indices(row["scores"], depth)
    if row["ranked_indices"][str(depth)] != expected:
        raise DrillError("cross_encoder_ranking_mismatch")
    return [seed[i] for i in expected[:5]]
