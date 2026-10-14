"""Offline data access and strict provenance for the frozen AML research artifacts.

Data always comes from this checkout. Source hashes use exactly the requested
source tree, or the archived pre-cleanup sources by default. Never substitute a
current helper, ignore a missing file, or retry against another source tree.
"""
import gzip
import hashlib
import json
from pathlib import Path, PurePosixPath
from aml_drill import DrillError
from aml_evidence import decode_fragments, coverage, resolve_required_messages
from aml_public import load_public
from aml_quality import digest, source_key

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "evals/aml/budget-v1"
TEMPORAL_OUT = ROOT / "evals/aml/temporal-v1"
ARCHIVE_ROOT = ROOT / "archives/aml-research"

ARMS = ("vector", "chat", "bge")


def read(path):
    opener = gzip.open if str(path).endswith(".gz") else open
    with opener(path, "rt", encoding="utf-8") as stream:
        return json.load(stream)


def load_data(dataset):
    if dataset == "public":
        return load_public(ROOT)
    corpus = read(ROOT / "evals/aml/v4/corpus.json")
    labels = read(ROOT / "evals/aml/v4/rubrics.json")["questions"]
    for q in corpus["questions"]:
        labels[q["id"]]["cluster"] = q["pair_id"]
    return corpus, labels


def question_payload(q):
    result = {"question": q["query"]}
    if "question_date" in q:
        result["question_date"] = q["question_date"]
    return result


def metrics(hits, known, label, dataset):
    parts = decode_fragments(hits, known, allow_legacy=True)
    return coverage(parts, required_messages=resolve_required_messages(known, [source_key(x) for x in label["required_source_keys"]])) if dataset == "v4" else coverage(parts, required_sessions=label["required_sessions"])


def artifact_path(dataset, stage, arm=None):
    return OUT / (dataset + "-" + stage + ("-" + arm if arm else "") + ".json.gz")


def load_artifact(dataset, stage, arm=None):
    path = artifact_path(dataset, stage, arm)
    value = read(path)
    if value.get("status") != "completed" or value.get("freeze_sha256") != digest(OUT / "freeze.json"):
        raise DrillError("invalid_upstream_artifact")
    return value, {"file": path.name, "sha256": digest(path)}


def validate_rows(rows, questions):
    ids = [r["id"] for r in rows]
    if len(ids) != len(set(ids)) or ids != [q["id"] for q in questions]:
        raise DrillError("incomplete_or_reordered_questions")


PHASES = ("baseline", "compacted", "reopened", "grown", "recompacted")


def payload_hash(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True,
                                    separators=(",", ":")).encode()).hexdigest()



def source_path(name, source_root=None):
    """Resolve one explicit repository-relative source path, without fallback."""
    relative = PurePosixPath(name)
    if (relative.is_absolute() or ".." in relative.parts or "\\" in name
            or len(relative.parts) < 2 or relative.parts[0] != "tools"):
        raise DrillError("invalid_frozen_source_path")
    root = Path(source_root) if source_root is not None else ARCHIVE_ROOT
    path = root.joinpath(*relative.parts)
    if not path.resolve().is_relative_to(root.resolve()):
        raise DrillError("invalid_frozen_source_path")
    if not path.is_file():
        raise DrillError("frozen_source_missing")
    return path


def verify_frozen_files(files, source_root=None):
    for name, expected in files.items():
        relative = PurePosixPath(name)
        if relative.is_absolute() or ".." in relative.parts or "\\" in name:
            raise DrillError("invalid_frozen_input_path")
        if relative.parts and relative.parts[0] == "tools":
            path = source_path(name, source_root)
        else:
            path = ROOT.joinpath(*relative.parts)
            if not path.resolve().is_relative_to(ROOT.resolve()) or not path.is_file():
                raise DrillError("frozen_input_missing")
        if digest(path) != expected:
            raise DrillError("frozen_input_changed")


def verify_freeze(source_root=None):
    frozen = read(OUT / "freeze.json")
    verify_frozen_files(frozen["files"], source_root)
    return frozen
