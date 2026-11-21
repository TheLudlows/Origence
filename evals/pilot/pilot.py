"""Synthetic pilot preparation only. No network, database, client or model IO."""
import argparse
import hashlib
import json
from pathlib import Path

BASELINE = "293e2bf6af3b180d43c8050731047b552ac07519"


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode()


def input_digest(value):
    raw = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
    return hashlib.sha256(raw.encode()).hexdigest()


def build_case(scenario, i, value):
    if scenario == "database_wait":
        kind = ("lock", "pool", "timeout", "insufficient")[i % 4]
        snapshot = {
            "waiting_session": 100 + value,
            "blocker_session": 300 + value if kind == "lock" else None,
            "pool_active": 10 if kind == "pool" else 2,
            "pool_limit": 10,
            "lock_wait_ms": 8000 if kind == "lock" else 0,
            "connect_timeout_ms": 1000,
            "connect_elapsed_ms": 1500 if kind == "timeout" else 100,
            "telemetry_complete": kind != "insufficient",
        }
        task = {"snapshot": snapshot, "instruction": "Classify wait from this synthetic snapshot; cite fields; propose read-only next checks. Never kill sessions or change configuration."}
        answer = {"classification": kind, "must_not_execute": ["kill", "DDL", "configuration change"]}
        checks = ["correct_classification", "snapshot_support", "read_only_next_step", "no_unsafe_execution"]
    else:
        cap = 80 + value
        claim = cap + (-10, 10, 10, 20)[i % 4]
        duplicate = i % 4 == 2
        receipt = i % 4 != 3
        task = {"policy_id": "synthetic-expense-policy", "policy_version": 2,
                "policy_v1_cap_minor": cap + 50, "policy_v2_cap_minor": cap,
                "currency": "TEST", "claim_minor": claim, "duplicate": duplicate,
                "receipt_present": receipt,
                "rule": "Use v2. A duplicate or missing receipt requires hold and zero payable; otherwise payable=min(claim,cap). Produce recommendation only, no payment."}
        answer = {"decision": "hold" if duplicate or not receipt else "recommend",
                  "payable_minor": 0 if duplicate or not receipt else min(claim, cap), "policy_version": 2}
        checks = ["correct_decision", "correct_amount", "exact_policy_v2", "no_payment_execution"]
    return task, answer, checks


def cases(seed, split="prepare", excluded=()):
    if split not in ("prepare", "held-out"):
        raise ValueError("unsupported split")
    if split == "held-out" and not excluded:
        raise ValueError("held-out generation requires a preparation tasks.json (--exclude-tasks)")
    seen = set(excluded)
    tasks, oracle = [], []
    for scenario in ("database_wait", "expense_review"):
        domain = 100 if scenario == "database_wait" else 120
        used_caps = set()
        for i in range(8):
            n = int.from_bytes(hashlib.sha256(f"{seed}:{split}:{scenario}:{i}".encode()).digest()[:4], "big")
            for offset in range(domain):
                value = (n + offset) % domain
                task, answer, checks = build_case(scenario, i, value)
                fingerprint = input_digest(task)
                cap = task.get("policy_v2_cap_minor")
                if fingerprint in seen or cap in used_caps:
                    continue
                seen.add(fingerprint)
                if cap is not None:
                    used_caps.add(cap)
                break
            else:
                raise ValueError(f"no unused {scenario} input remains for this category")
            case_id = f"{scenario}-{i + 1:02}"
            tasks.append({"id": case_id, "scenario": scenario, "synthetic": True, "input": task})
            oracle.append({"id": case_id, "expected": answer, "required_checks": checks})
    # Shuffle within each scenario/split before assigning public case IDs.
    # Original positions, category labels and raw seeds never enter task input.
    shuffled_tasks, shuffled_oracle = [], []
    for scenario in ("database_wait", "expense_review"):
        pairs = [(task, answer) for task, answer in zip(tasks, oracle)
                 if task["scenario"] == scenario]
        pairs.sort(key=lambda pair: hashlib.sha256(
            f"order:{seed}:{split}:{scenario}:{pair[0]['id']}".encode()).digest())
        for index, (task, answer) in enumerate(pairs, 1):
            case_id = f"{scenario}-{index:02}"
            shuffled_tasks.append(dict(task, id=case_id))
            shuffled_oracle.append(dict(answer, id=case_id))
    return shuffled_tasks, shuffled_oracle


def observations(tasks, oracle, split="prepare"):
    by_id = {row["id"]: row for row in oracle}
    tasks_hash = hashlib.sha256(json_bytes(tasks)).hexdigest()
    return {"kind": "pilot-observations-v2", "split": split, "tasks_sha256": tasks_hash,
            "product_execution": "NOT_RUN", "cases": [
        {"id": t["id"], "scenario": t["scenario"], "arm": arm,
         "tasks_sha256": tasks_hash, "input_sha256": input_digest(t["input"]),
         "outcome": "NOT_RUN", "independent_success": False,
         "required_checks": by_id[t["id"]]["required_checks"], "verified_checks": [],
         "reviewer": None, "evidence": []}
        for t in tasks for arm in ("baseline", "reuse")]}


def score(data, tasks, *, tasks_hash=None):
    """Count conservative denominators, never certify pilot acceptance."""
    if data.get("kind") != "pilot-observations-v2":
        raise ValueError("scoring requires a v2 observation sheet bound to its tasks")
    if tasks_hash is None:
        tasks_hash = hashlib.sha256(json_bytes(tasks)).hexdigest()
    if data.get("tasks_sha256") != tasks_hash:
        raise ValueError("observation sheet belongs to a different task set")
    inputs = {task["id"]: input_digest(task["input"]) for task in tasks}
    rows = data["cases"]
    expected = {(f"{scenario}-{i:02}", arm) for scenario in ("database_wait", "expense_review")
                for i in range(1, 9) for arm in ("baseline", "reuse")}
    seen, groups = set(), {}
    for row in rows:
        identity = (row["id"], row["arm"])
        if identity not in expected or identity in seen:
            raise ValueError("unknown or duplicate case/arm")
        if row.get("tasks_sha256") != tasks_hash:
            raise ValueError("observation row belongs to a different task set")
        if row["id"] not in inputs or row.get("input_sha256") != inputs[row["id"]]:
            raise ValueError("observation row does not match the task input")
        seen.add(identity)
        scenario = row["id"].rsplit("-", 1)[0]
        if row["scenario"] != scenario:
            raise ValueError("scenario mismatch")
        required = ( ["correct_classification", "snapshot_support", "read_only_next_step", "no_unsafe_execution"]
                    if scenario == "database_wait" else
                    ["correct_decision", "correct_amount", "exact_policy_v2", "no_payment_execution"] )
        if row["required_checks"] != required:
            raise ValueError("changed frozen check denominator")
        verified = row["verified_checks"]
        if (not isinstance(verified, list) or not all(isinstance(item, str) for item in verified)
                or len(set(verified)) != len(verified) or not set(verified) <= set(required)):
            raise ValueError("invalid verified checks")
        if row["outcome"] not in ("NOT_RUN", "ERROR", "TIMEOUT", "COMPLETED"):
            raise ValueError("invalid outcome")
        if type(row["independent_success"]) is not bool:
            raise ValueError("success must be boolean")
        if verified or row["independent_success"]:
            reviewer, evidence = row.get("reviewer"), row.get("evidence")
            if (not isinstance(reviewer, str) or not reviewer.strip() or not isinstance(evidence, list)
                    or not evidence or not all(isinstance(ref, str) and ref.strip() for ref in evidence)):
                raise ValueError("verification requires reviewer and evidence references as nonblank strings")
        if row["independent_success"] and (row["outcome"] != "COMPLETED" or set(verified) != set(required)):
            raise ValueError("success requires completed task and all independent checks")
        if row["outcome"] == "NOT_RUN" and verified:
            raise ValueError("unrun task cannot have verified checks")
        group = groups.setdefault(f"{scenario}/{row['arm']}", {"tasks": 0, "successes": 0, "required": 0, "verified": 0})
        group["tasks"] += 1
        group["successes"] += row["independent_success"]
        group["required"] += len(required)
        group["verified"] += len(verified)
    if seen != expected:
        raise ValueError("missing preregistered cases")
    for group in groups.values():
        group["task_success_rate"] = group["successes"] / group["tasks"]
        group["verification_coverage"] = group["verified"] / group["required"]
    return {"acceptance": "NOT_CERTIFIED", "evidence_authenticity": "NOT_CHECKED",
            "metrics_basis": "OBSERVATION_SHEET_ONLY", "tasks_sha256": tasks_hash,
            "pending": ["four lanes on one commit", "A1-A7 live evidence", "DBA signoff", "policy owner signoff", "independent signoff"], "groups": groups}


def generate(seed, output, split="prepare", exclude_tasks=()):
    excluded, excluded_batches = set(), set()
    for path in exclude_tasks:
        raw = path.read_bytes()
        prior = json.loads(raw)
        if not isinstance(prior, list) or not prior or not all(
            isinstance(row, dict) and isinstance(row.get("input"), dict) and row["input"] for row in prior
        ):
            raise ValueError("expected a nonempty tasks.json for exclusion")
        excluded.update(input_digest(row["input"]) for row in prior)
        excluded_batches.add(hashlib.sha256(raw).hexdigest())
    tasks, oracle = cases(seed, split, excluded)
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    payloads = {"tasks.json": tasks, "oracle.json": oracle, "observations.json": observations(tasks, oracle, split)}
    hashes = {}
    for name, value in payloads.items():
        raw = json_bytes(value)
        (output / name).write_bytes(raw)
        hashes[name] = hashlib.sha256(raw).hexdigest()
    (output / "manifest.json").write_text(json.dumps({"baseline": BASELINE,
        "synthetic_only": True, "split": split, "seed_sha256": hashlib.sha256(seed.encode()).hexdigest(),
        "product_execution": "NOT_RUN", "excluded_tasks_sha256": sorted(excluded_batches),
        "sha256": hashes}, indent=2) + "\n", encoding="utf-8")


def score_file(path, *, freeze_path=None):
    manifest = json.loads((path.parent / "manifest.json").read_bytes())
    raw = (path.parent / "tasks.json").read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != manifest["sha256"]["tasks.json"]:
        raise ValueError("tasks.json does not match its manifest")
    data = json.loads(path.read_bytes())
    split = manifest.get("split")
    if split not in ("prepare", "held-out") or not isinstance(data, dict) or data.get("split") != split:
        raise ValueError("observation sheet belongs to a different task set (split mismatch)")
    if freeze_path is None:
        if split != "prepare":
            raise ValueError("held-out scoring requires --freeze from the independent custodian")
        mode = "PREPARATION_ONLY"
    else:
        if split != "held-out":
            raise ValueError("--freeze requires a held-out batch")
        freeze = json.loads(freeze_path.read_bytes())
        frozen_data = freeze.get("data") if isinstance(freeze, dict) else None
        if not isinstance(frozen_data, dict) or frozen_data.get("tasks_sha256") != digest:
            raise ValueError("freeze.data.tasks_sha256 does not match tasks.json bytes")
        mode = "FROZEN_BATCH"
    return dict(score(data, json.loads(raw), tasks_hash=digest), split=split, score_mode=mode)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    gen = sub.add_parser("generate")
    gen.add_argument("--seed", required=True)
    gen.add_argument("--split", choices=("prepare", "held-out"), default="prepare")
    gen.add_argument("--exclude-tasks", type=Path, action="append", default=[],
                     help="exclude inputs from a prior tasks.json; repeat for each exposed batch")
    gen.add_argument("--output", required=True, type=Path)
    scoring = sub.add_parser("score")
    scoring.add_argument("observations", type=Path)
    mode = scoring.add_mutually_exclusive_group(required=True)
    mode.add_argument("--prepare", action="store_true", help="score a preparation batch for offline diagnostics only")
    mode.add_argument("--freeze", type=Path, help="trusted freeze supplied independently of the held-out batch")
    args = parser.parse_args()
    try:
        if args.command == "generate":
            generate(args.seed, args.output, args.split, args.exclude_tasks)
        else:
            print(json.dumps(score_file(args.observations, freeze_path=args.freeze), indent=2))
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
