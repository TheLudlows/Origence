"""Offline AML evidence hashing, provenance validation, and retrieval metrics."""
import hashlib
import json
from aml_drill import DrillError

def stream_digest(stream):
    """Bounded-memory SHA-256, also supported by local GPU Python 3.10."""
    value = hashlib.sha256()
    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
        value.update(chunk)
    return value.hexdigest()


def digest(path):
    with open(path, "rb") as stream:
        return stream_digest(stream)


def source_key(value):
    return (value["session_id"], value["message_index"])


def decode_hits(data, batches, user_id, *, allow_legacy=False):
    """Validate full source fragments, then resolve the old gold-key vocabulary.

    These short-message quality labels omit batch IDs. Refuse any ambiguous
    session/index in their corpus instead of silently crediting another batch.
    Richer batch-aware metrics use aml_evidence directly.
    """
    from aml_evidence import source_index, decode_fragments
    if not isinstance(data, list) or len(data) > 100:
        raise DrillError("invalid_hit_list")
    known = source_index(batches, user_id)
    legacy_keys = [(session, index) for session, _batch, index in known]
    if len(set(legacy_keys)) != len(legacy_keys):
        raise DrillError("ambiguous_quality_source")
    if allow_legacy and data:
        # The earliest frozen reports predate even the legacy range parser.
        # Their explicit archival path means whole-message equality, never
        # interpreting missing ranges as current message-batch provenance.
        metadata = []
        for hit in data:
            try:
                header, separator, content = hit["content"].partition("\n")
                if not separator or not header.startswith("Source metadata: "):
                    raise ValueError()
                meta = json.loads(header.removeprefix("Source metadata: "))
                if not isinstance(meta, dict):
                    raise ValueError()
                metadata.append((hit, meta, content))
            except (KeyError, TypeError, AttributeError, ValueError):
                raise DrillError("invalid_legacy_source") from None
        if all("parser" not in meta for _hit, meta, _content in metadata):
            original = {(session, index): message for (session, _batch, index), (message, _count) in known.items()}
            ids, keys = set(), []
            for hit, meta, content in metadata:
                try:
                    ident = hit["id"]
                    if not isinstance(ident, str) or not ident or ident in ids:
                        raise ValueError()
                    ids.add(ident)
                    if set(meta) != {"session_id", "message_index", "role", "timestamp"} or type(meta["message_index"]) is not int:
                        raise ValueError()
                    if meta["timestamp"] is not None and type(meta["timestamp"]) is not int:
                        raise ValueError()
                    key = source_key(meta)
                    message = original[key]
                    if content != message["content"] or any(meta[field] != message.get(field) for field in ("role", "timestamp")):
                        raise ValueError()
                    keys.append(key)
                except (KeyError, TypeError, ValueError):
                    raise DrillError("invalid_legacy_source") from None
            return keys
    parts = decode_fragments(data, known, allow_legacy=allow_legacy)
    if any(part["start"] != 0 or part["end"] != part["message_bytes"] for part in parts):
        raise DrillError("source_mismatch")
    return [(part["key"][0], part["key"][2]) for part in parts]


def score(required, ranked, k):
    gold = set(required)
    retrieved = set(ranked[:k])
    if not gold:
        return {"recall": None, "all_evidence": None, "reciprocal_rank": None,
                "nonempty_no_answer": bool(ranked[:k])}
    return {"recall": len(gold & retrieved) / len(gold),
            "all_evidence": gold <= retrieved,
            "reciprocal_rank": next((1 / (i + 1) for i, key in enumerate(ranked[:k]) if key in gold), 0),
            "nonempty_no_answer": None}


def summarize(rows, k):
    scored = [row["metrics"][str(k)] for row in rows]
    answerable = [x for x in scored if x["recall"] is not None]
    absent = [x for x in scored if x["nonempty_no_answer"] is not None]
    return {"questions": len(rows), "answerable": len(answerable), "no_answer": len(absent),
            "recall": sum(x["recall"] for x in answerable) / len(answerable) if answerable else None,
            "all_evidence": sum(x["all_evidence"] for x in answerable) / len(answerable) if answerable else None,
            "mrr": sum(x["reciprocal_rank"] for x in answerable) / len(answerable) if answerable else None,
            "nonempty_no_answer": sum(x["nonempty_no_answer"] for x in absent)}
