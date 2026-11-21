"""Check a frozen P6 package offline; no authenticity, scoring or certification."""
import argparse
import hashlib
import json
from pathlib import Path
from uuid import UUID

from p6_check import check_index, digest, file_identity, package_file, read_json, read_object, run_prefix
from pilot import input_digest

PROVENANCE = {
    "evidence_origin": "owner_transcribed_external",
    "verifier_authentication": "unverified_claim",
    "platform_verification": "not_performed",
}


def one(rows, **fields):
    if not isinstance(rows, list) or any(not isinstance(row, dict) for row in rows):
        raise ValueError("expected rows of JSON objects")
    matches = [row for row in rows if all(row.get(key) == value for key, value in fields.items())]
    if len(matches) != 1:
        raise ValueError("expected one matching row")
    return matches[0]


def binding(case_link, *, freeze, freeze_sha256, tasks, observation, delivery_ref):
    root, link, index, synthetic = check_index(case_link)
    original, transcription = link["independent_original"], link["owner_transcription"]
    if synthetic or any(item["status"] != "RECORDED" for item in (link, original, transcription)):
        raise ValueError("case-link and both supporting sections must be RECORDED, not synthetic")
    if transcription["withdrawal_status"] != "active":
        raise ValueError("owner transcription was not recorded as active")

    freeze_record = read_object(freeze, digest(freeze_sha256), label="freeze")
    freeze_data = freeze_record["data"]
    if freeze_record.get("status") == "BLOCKED" or freeze_data.get("contaminated") is True:
        raise ValueError("frozen batch is blocked or contaminated")
    tasks_hash = digest(freeze_data["tasks_sha256"])
    task_rows = read_json(tasks, tasks_hash, label="tasks")
    obs = read_object(observation)
    if (obs["kind"], obs["split"], obs["tasks_sha256"]) != (
        "pilot-observations-v2", "held-out", tasks_hash
    ):
        raise ValueError("observation must be v2 held-out and belong to the frozen tasks")
    if obs["product_execution"] != "RECORDED":
        raise ValueError("observation sheet product execution is not RECORDED")

    case_id, scenario, arm, client = (link[key] for key in ("case_id", "scenario", "arm", "client"))
    if scenario not in ("database_wait", "expense_review") or arm not in ("baseline", "reuse"):
        raise ValueError("unexpected case scenario or arm")
    reuse_client = {"database_wait": "codex", "expense_review": "claude_code"}[scenario]
    if client not in ("codex", "claude_code") or (arm == "reuse" and client != reuse_client):
        raise ValueError("unexpected client for pilot scenario or reuse arm")
    prefix = run_prefix(link["run_prefix"])
    if prefix[1] > 10000 or len(prefix[3]) > 64:
        raise ValueError("prefix exceeds Run verification limits")
    task = one(task_rows, id=case_id)
    if task["scenario"] != scenario or not isinstance(task["input"], dict):
        raise ValueError("frozen task scenario or input mismatch")
    expected_input = input_digest(task["input"])
    row = one(obs["cases"], id=case_id, arm=arm)
    if (row["scenario"], row["tasks_sha256"], row["input_sha256"]) != (
        scenario, tasks_hash, expected_input
    ):
        raise ValueError("observation row does not match frozen case input")
    if row["run_id"] != prefix[0]:
        raise ValueError("observation row Run mismatch")
    read_object(package_file(root, case_link.name), digest(row["case_link_sha256"]), label="case link")
    if row["outcome"] not in ("COMPLETED", "ERROR", "TIMEOUT"):
        raise ValueError("observation case has no recorded execution")
    references = row["evidence"]
    if not isinstance(references, list) or case_link.name not in references:
        raise ValueError("observation row must reference the case-link filename")

    delivery_entry = one(index["files"], path=delivery_ref, type="delivered_input")
    delivery_path = package_file(root, delivery_ref)
    delivery = read_object(delivery_path, digest(delivery_entry["sha256"]), label="delivery")
    if tuple(delivery[key] for key in ("case_id", "scenario", "arm", "client", "run_id")) != (
        case_id, scenario, arm, client, prefix[0]
    ):
        raise ValueError("delivery record case, arm, client or Run mismatch")
    if not isinstance(delivery["input"], dict) or input_digest(delivery["input"]) != expected_input:
        raise ValueError("delivered input does not match frozen case input")

    original_path = package_file(root, original["ref"])
    receipt_path = package_file(root, transcription["receipt_ref"])
    refs = [case_link.name, link["evidence_index"]["ref"], *(item["path"] for item in index["files"])]
    indexed_identities = {file_identity(package_file(root, ref)) for ref in refs}
    separate = {file_identity(original_path), file_identity(receipt_path)}
    if len(separate) != 2 or separate & indexed_identities:
        raise ValueError("original and receipt must be separate non-indexed files")
    with original_path.open("rb") as handle:
        hasher = hashlib.sha256()
        for chunk in iter(lambda: handle.read(65536), b""):
            hasher.update(chunk)
    if original_path.stat().st_size == 0 or hasher.hexdigest() != digest(original["sha256"]):
        raise ValueError("original SHA-256 mismatch or empty original")
    receipt = read_object(receipt_path, digest(transcription["receipt_sha256"]), label="receipt")
    if receipt["withdrawal"] is not None:
        raise ValueError("saved receipt is withdrawn")
    if any(receipt.get(key) != value for key, value in PROVENANCE.items()):
        raise ValueError("unexpected owner receipt provenance")
    assessment = receipt["assessment"]
    if run_prefix(dict(assessment, run_id=receipt["run_id"])) != prefix:
        raise ValueError("owner receipt Run prefix mismatch")
    verification_id = transcription["verification_id"]
    parsed_id = UUID(verification_id) if isinstance(verification_id, str) else None
    if (parsed_id is None or str(parsed_id) != verification_id
            or parsed_id.int == 0 or assessment["verification_id"] != verification_id):
        raise ValueError("owner receipt verification ID mismatch")
    if assessment["conclusion"] not in ("passed", "failed", "inconclusive"):
        raise ValueError("invalid owner-transcribed conclusion")
    return {
        "result": "STRUCTURAL_BINDING_ONLY",
        "saved_receipt": "NOT_WITHDRAWN_IN_SUPPLIED_BYTES",
        "current_run_or_withdrawal": "NOT_CHECKED",
        "evidence_authenticity": "NOT_CHECKED",
        "pilot_acceptance": "NOT_CERTIFIED",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("case_link", type=Path)
    parser.add_argument("--freeze", required=True, type=Path)
    parser.add_argument("--freeze-sha256", required=True)
    parser.add_argument("--tasks", required=True, type=Path)
    parser.add_argument("--observation", required=True, type=Path)
    parser.add_argument("--delivery-ref", required=True,
                        help="package-relative delivery record already listed in the P6 index")
    try:
        print(json.dumps(binding(**vars(parser.parse_args()))))
    except OSError:
        parser.exit(1, "P6 binding check failed: required file unavailable\n")
    except (ValueError, KeyError, TypeError, RecursionError) as error:
        parser.exit(1, f"P6 binding check failed: {error}\n")


if __name__ == "__main__":
    main()
