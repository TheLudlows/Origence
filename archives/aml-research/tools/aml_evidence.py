"""Strict chunk provenance for new AML evaluations; historical reports stay unchanged."""
import json
import uuid
from aml_drill import DrillError

PARSER = "message-batch-ranges-v1"
# Exact historical marker remains recognizable by offline evidence verifiers.
# Current fixtures and hosts must emit PARSER with source event and batch IDs.
LEGACY_PARSER = "aml-message-ranges-v1"


def source_index(batches, user_id):
    known = {}
    for batch in batches:
        if batch["user_id"] != user_id:
            continue
        for i, message in enumerate(batch["messages"]):
            key = (batch["session_id"], batch["request_id"], i)
            if key in known:
                raise DrillError("ambiguous_source_batch")
            known[key] = (message, len(batch["messages"]))
    return known


def decode_fragments(hits, known, *, allow_legacy=False):
    """Verify exact UTF-8 slices, scope and parser, without crediting whole messages."""
    if not isinstance(hits, list) or len(hits) > 100:
        raise DrillError("invalid_hit_list")
    out, ids, source_events, event_batches = [], set(), {}, {}
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
            if allow_legacy and meta.get("parser") == LEGACY_PARSER:
                # Old evidence has no batch ID and is valid only when its full
                # source corpus makes the old session/index locator unambiguous.
                matches = [key for key in known if key[0] == meta["session_id"] and key[2] == meta["message_index"]]
                if len(matches) != 1:
                    raise ValueError()
                key = matches[0]
            elif meta.get("parser") == PARSER:
                key = (meta["session_id"], meta["external_batch_id"], meta["message_index"])
                event = str(uuid.UUID(meta["source_event_id"]))
                if any(type(meta.get(field)) is not int or meta[field] != 1
                       for field in ("parser_version", "chunking_version")):
                    raise ValueError()
                if (source_events.setdefault(key[:2], event) != event
                        or event_batches.setdefault(event, key[:2]) != key[:2]):
                    raise ValueError()
            else:
                raise ValueError()
            if meta.get("timestamp") is not None and type(meta["timestamp"]) is not int:
                raise ValueError()
            message, count = known[key]
            start, end = meta["byte_start"], meta["byte_end"]
            raw = message["content"].encode("utf-8")
            if (type(start) is not int or type(end) is not int or not 0 <= start < end <= len(raw)
                    or end - start > 2400):
                raise ValueError()
            # Decoding the slice also rejects offsets within a multi-byte character.
            original = raw[start:end].decode("utf-8")
            if (meta.get("byte_basis") != "message_content_utf8"
                    or meta.get("source_path") != f"/messages/{key[2]}/content"
                    or type(meta.get("message_count")) is not int or meta["message_count"] != count
                    or any(meta.get(k) != message.get(k) for k in ("role", "timestamp"))
                    or original != content):
                raise ValueError()
        except (ValueError, KeyError, TypeError, AttributeError, UnicodeError):
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



def resolve_required_messages(known, required):
    """Resolve historical session/index labels only with an unambiguous corpus."""
    resolved = []
    for session, index in required:
        matches = [key for key in known if key[0] == session and key[2] == index]
        if len(matches) != 1:
            raise DrillError("ambiguous_required_source")
        resolved.append(matches[0])
    return resolved


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
