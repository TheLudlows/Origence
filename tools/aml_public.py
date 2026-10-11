"""Pinned LongMemEval adapter. Labels are separated from model-visible questions."""
import datetime
import hashlib
import json
import re
from pathlib import Path
from aml_drill import DrillError
from aml_quality import digest


def timestamp(value):
    match = re.fullmatch(r"(\d{4}/\d{2}/\d{2}) \([A-Za-z]{3}\) (\d{2}:\d{2})", value)
    if not match:
        raise DrillError("invalid_public_date")
    # The dataset does not specify a zone: UTC is a deterministic encoding convention.
    return int(datetime.datetime.strptime(" ".join(match.groups()), "%Y/%m/%d %H:%M")
               .replace(tzinfo=datetime.timezone.utc).timestamp() * 1000)


def adapt(records, selected):
    by_id = {r["question_id"]: r for r in records}
    if len(by_id) != len(records) or len(set(selected)) != len(selected):
        raise DrillError("duplicate_public_id")
    corpus = {"batches": [], "questions": []}
    labels, owners, parents = {}, {}, {qid: qid for qid in selected}
    def find(x):
        while parents[x] != x:
            x = parents[x]
        return x
    for qid in selected:
        row = by_id[qid]
        sessions, ids, dates = row["haystack_sessions"], row["haystack_session_ids"], row["haystack_dates"]
        if not len(sessions) == len(ids) == len(dates) or len(set(ids)) != len(ids):
            raise DrillError("invalid_public_sessions")
        user = "lme-" + hashlib.sha256(qid.encode()).hexdigest()[:20]
        for sid, date, messages in zip(ids, dates, sessions):
            if sid in owners:
                parents[find(qid)] = find(owners[sid])
            owners[sid] = qid
            if not 1 <= len(messages) <= 256:
                raise DrillError("public_batch_limit")
            converted = []
            for message in messages:
                content = message["content"]
                if (message["role"] not in ("user", "assistant") or not isinstance(content, str)
                        or not content.strip() or "\0" in content):
                    raise DrillError("invalid_public_message")
                converted.append({"role": message["role"], "content": content, "timestamp": timestamp(date)})
            if sum(len(m["content"].encode()) for m in converted) > 1_000_000:
                raise DrillError("public_batch_bytes")
            corpus["batches"].append({"user_id": user, "session_id": sid,
                "request_id": "session-" + hashlib.sha256(sid.encode()).hexdigest(), "messages": converted})
        # No answer, gold session IDs, category, or abstention flag enters the model payload.
        corpus["questions"].append({"id": qid, "user_id": user, "query": row["question"],
                                    "question_date": row["question_date"], "split": "public_test"})
        answerable = not qid.endswith("_abs")
        required = row["answer_session_ids"] if answerable else []
        if not set(required) <= set(ids):
            raise DrillError("missing_public_gold_session")
        labels[qid] = {"answerable": answerable, "expected_answer": str(row["answer"]),
                       "required_sessions": required, "category": row["question_type"],
                       "rubric": "Answer must match the reference semantically and satisfy all question requirements; all substantive claims must follow from cited original evidence. Allow arithmetic and temporal inference. Reject wrong entity, event time, missing conditions, changed strict boundary or unsupported extra claims. For unanswerable questions require abstention."}
    groups = {}
    for qid in selected:
        groups.setdefault(find(qid), []).append(qid)
    for members in groups.values():
        cluster = "history-" + hashlib.sha256("|".join(sorted(members)).encode()).hexdigest()[:16]
        for qid in members:
            labels[qid]["cluster"] = cluster
    return corpus, labels


def load_public(root):
    receipt = json.loads((root / "evals/aml/public/preparation.json").read_text(encoding="utf-8"))
    path = root / "target/aml-public" / receipt["file"]
    if path.stat().st_size != receipt["bytes"] or digest(path) != receipt["sha256"]:
        raise DrillError("public_source_hash_mismatch")
    records = json.loads(path.read_text(encoding="utf-8"))
    selected = [x["id"] for x in receipt["selected"]]
    expected = sorted(records, key=lambda r: hashlib.sha256(("Origence-public-test-v1:" + r["question_id"]).encode()).hexdigest())[:32]
    if [r["question_id"] for r in expected] != selected:
        raise DrillError("public_selection_mismatch")
    return adapt(records, selected)
