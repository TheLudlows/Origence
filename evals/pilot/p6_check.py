"""Offline P6 index linkage and file checks; never certifies evidence."""
import argparse
import hashlib
import json
import re
import stat
from pathlib import Path, PureWindowsPath
from uuid import UUID

SHA256 = re.compile(r"[0-9a-f]{64}")
MAX_JSON = 1024 * 1024


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON member")
        result[key] = value
    return result


def read_object(path, expected_hash=None):
    with path.open("rb") as handle:
        raw = handle.read(MAX_JSON + 1)
    if len(raw) > MAX_JSON:
        raise ValueError("JSON exceeds 1 MiB")
    if expected_hash is not None and hashlib.sha256(raw).hexdigest() != expected_hash:
        raise ValueError("index SHA-256 mismatch")
    result = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object)
    if not isinstance(result, dict):
        raise ValueError("expected JSON object")
    return result


def package_file(root, ref):
    if not isinstance(ref, str) or re.search(r'[\\:*?"<>|\x00-\x1f]', ref):
        raise ValueError("expected package-relative file path")
    parts = ref.split("/")
    if any(not part or part.endswith((".", " ")) or PureWindowsPath(part).is_reserved()
           for part in parts):
        raise ValueError("unsupported package path segment")
    path = root
    for part in parts:
        path /= part
        meta = path.lstat()
        if stat.S_ISLNK(meta.st_mode) or (
            getattr(meta, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT
        ):
            raise ValueError("links and reparse points are unsupported")
    if not stat.S_ISREG(meta.st_mode) or not path.resolve(strict=True).is_relative_to(root):
        raise ValueError("expected regular file within package")
    return path


def digest(value):
    if not isinstance(value, str) or not SHA256.fullmatch(value):
        raise ValueError("expected lowercase SHA-256")
    return value


def run_prefix(value):
    run_id = value["run_id"]
    parsed = UUID(run_id) if isinstance(run_id, str) else None
    if parsed is None or str(parsed) != run_id or parsed.int == 0:
        raise ValueError("expected canonical non-nil Run UUID")
    end = value["through_sequence"]
    basis = value["basis_sequences"]
    if type(end) is not int or end < 1 or not isinstance(basis, list) or not basis or any(
        type(seq) is not int or not 1 <= seq <= end for seq in basis
    ) or len(basis) != len(set(basis)):
        raise ValueError("invalid Run prefix or basis sequences")
    return (run_id, end, digest(value["prefix_digest"]), tuple(basis))


def check(case_link):
    root = case_link.parent.resolve(strict=True)
    link_file = package_file(root, case_link.name)
    link = read_object(link_file)
    case_id = link["case_id"]
    if link["pilot_item"] != "P6" or not isinstance(case_id, str) or not case_id.strip():
        raise ValueError("expected P6 case ID")
    prefix = run_prefix(link["run_prefix"])
    entry = link["evidence_index"]
    index_file = package_file(root, entry["ref"])
    if index_file == link_file:
        raise ValueError("index cannot reference the case link")
    index = read_object(index_file, digest(entry["sha256"]))
    if run_prefix(index["run_prefix"]) != prefix:
        raise ValueError("Run prefix tuple mismatch")
    synthetic = index.get("scope") == "PUBLIC_SYNTHETIC_ONLY"
    if synthetic and (link["status"] != "NOT_RUN" or
                      link["independent_original"]["status"] != "NOT_RUN" or
                      link["owner_transcription"]["status"] != "NOT_RUN"):
        raise ValueError("public synthetic linkage must remain NOT_RUN")
    files = index["files"]
    if not isinstance(files, list) or not files:
        raise ValueError("expected nonempty evidence file list")
    seen = {link_file, index_file}
    for item in files:
        if item["case_id"] != case_id or item["pilot_item"] != "P6":
            raise ValueError("evidence belongs to another case or pilot item")
        path = package_file(root, item["path"])
        if path in seen:
            raise ValueError("duplicate or reserved evidence file")
        seen.add(path)
        length, checksum = item["bytes"], digest(item["sha256"])
        if type(length) is not int or length < 0 or length != path.stat().st_size:
            raise ValueError("evidence byte count mismatch")
        hasher, count = hashlib.sha256(), 0
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(65536), b""):
                count += len(chunk)
                if count > length:
                    raise ValueError("evidence byte count mismatch")
                hasher.update(chunk)
        if count != length:
            raise ValueError("evidence byte count mismatch")
        if hasher.hexdigest() != checksum:
            raise ValueError("evidence SHA-256 mismatch")
    return {"result": "INDEX_LINKAGE_ONLY", "public_synthetic": synthetic,
            "files_checked": len(files), "evidence_authenticity": "NOT_CHECKED",
            "pilot_acceptance": "NOT_CERTIFIED"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("case_link", type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(check(args.case_link)))
    except (OSError, ValueError, KeyError, TypeError, RecursionError) as error:
        parser.exit(1, f"P6 linkage check failed: {error}\n")


if __name__ == "__main__":
    main()
