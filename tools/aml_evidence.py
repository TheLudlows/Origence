"""Strict chunk provenance for new AML evaluations; historical reports stay unchanged."""
import json
from aml_drill import DrillError

PARSER = "aml-message-ranges-v1"


def source_index(batches, user_id):
    known = {}
    for batch in batches:
        if batch["user_id"] != user_id:
            continue
        for i, message in enumerate(batch["messages"]):
            key = (batch["session_id"], i)
            if key in known:
                raise DrillError("ambiguous_source_session")
            known[key] = (message, len(batch["messages"]))
    return known


def decode_fragments(hits, known):
    """Verify exact UTF-8 slices, scope and parser, without crediting whole messages."""
    if not isinstance(hits, list) or len(hits) > 100:
        raise DrillError("invalid_hit_list")
    out, ids = [], set()
    for hit in hits:
        if not isinstance(hit, dict) or not isinstance(hit.get("id"), str) or not hit["id"] or hit["id"] in ids:
            raise DrillError("invalid_or_duplicate_hit_id")
        ids.add(hit["id"])
        if not isinstance(hit.get("content"), str):
            raise DrillError("missing_source_metadata")
        header, separator, content = hit["content"].partition("\n")
        if not separator or not header.startswith("Source metadata: "):
            raise DrillError("missing_source_metadata")
        try:
            meta = json.loads(header[len("Source metadata: "):])
            if not isinstance(meta, dict) or type(meta.get("message_index")) is not int:
                raise ValueError()
            key = (meta["session_id"], meta["message_index"])
            message, count = known[key]
            start, end = meta["byte_start"], meta["byte_end"]
            raw = message["content"].encode("utf-8")
            if (type(start) is not int or type(end) is not int or not 0 <= start < end <= len(raw)
                    or end - start > 2400):
                raise ValueError()
            # Decoding the slice also rejects offsets within a multi-byte character.
            original = raw[start:end].decode("utf-8")
            if (meta.get("parser") != PARSER or meta.get("byte_basis") != "message_content_utf8"
                    or meta.get("source_path") != f"/messages/{key[1]}/content"
                    or type(meta.get("message_count")) is not int or meta["message_count"] != count
                    or any(meta.get(k) != message.get(k) for k in ("role", "timestamp"))
                    or original != content):
                raise ValueError()
        except (ValueError, KeyError, TypeError, UnicodeError):
            raise DrillError("invalid_source_fragment") from None
        out.append({"key": key, "start": start, "end": end, "message_bytes": len(raw)})
    return out


def complete_messages(fragments):
    grouped = {}
    for part in fragments:
        grouped.setdefault(part["key"], []).append(part)
    complete = set()
    for key, parts in grouped.items():
        cursor = 0
        for part in sorted(parts, key=lambda p: p["start"]):
            if part["start"] > cursor:
                break
            cursor = max(cursor, part["end"])
        if cursor == parts[0]["message_bytes"]:
            complete.add(key)
    return complete


def coverage(fragments, required_messages=None, required_sessions=None):
    """Session recall is a coarse diagnostic, never complete evidence recall."""
    if required_messages is not None:
        gold = set(required_messages)
        got = complete_messages(fragments)
        return {"full_message_recall": len(gold & got) / len(gold) if gold else None,
                "all_required_messages": gold <= got if gold else None}
    gold = set(required_sessions or [])
    got = {p["key"][0] for p in fragments}
    return {"session_recall": len(gold & got) / len(gold) if gold else None,
            "all_answer_sessions_hit": gold <= got if gold else None,
            "complete_evidence_recall": None}
