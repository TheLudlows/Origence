"""Synthetic pilot preparation only. No network, database, client or model IO."""
import argparse
import hashlib
import json
from pathlib import Path

BASELINE = "293e2bf6af3b180d43c8050731047b552ac07519"


def cases(seed):
    tasks, oracle = [], []
    for scenario in ("database_wait", "expense_review"):
        for i in range(8):
            n = int.from_bytes(hashlib.sha256(f"{seed}:{scenario}:{i}".encode()).digest()[:4], "big")
            case_id = f"{scenario}-{i + 1:02}"
            if scenario == "database_wait":
                kind = ("lock", "pool", "timeout", "insufficient")[i % 4]
                snapshot = {
                    "waiting_session": 100 + n % 100,
                    "blocker_session": 300 + n % 100 if kind == "lock" else None,
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
                cap = 80 + n % 120
                claim = cap + (-10, 0, 10, 20)[i % 4]
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
            tasks.append({"id": case_id, "scenario": scenario, "synthetic": True, "input": task})
            oracle.append({"id": case_id, "expected": answer, "required_checks": checks})
    return tasks, oracle


def observations(tasks, oracle):
    by_id = {row["id"]: row for row in oracle}
    return {"kind": "pilot-observations-v1", "product_execution": "NOT_RUN", "cases": [
        {"id": t["id"], "scenario": t["scenario"], "arm": arm,
         "outcome": "NOT_RUN", "independent_success": False,
         "required_checks": by_id[t["id"]]["required_checks"], "verified_checks": [],
         "reviewer": None, "evidence": []}
        for t in tasks for arm in ("baseline", "reuse")]}


def score(data):
    """Count conservative denominators, never certify pilot acceptance."""
    if data.get("kind") != "pilot-observations-v1":
        raise ValueError("unsupported observation format")
    rows = data["cases"]
    expected = {(f"{scenario}-{i:02}", arm) for scenario in ("database_wait", "expense_review")
                for i in range(1, 9) for arm in ("baseline", "reuse")}
    seen, groups = set(), {}
    for row in rows:
        identity = (row["id"], row["arm"])
        if identity not in expected or identity in seen:
            raise ValueError("unknown or duplicate case/arm")
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
        if len(set(verified)) != len(verified) or not set(verified) <= set(required):
            raise ValueError("invalid verified checks")
        if row["outcome"] not in ("NOT_RUN", "ERROR", "TIMEOUT", "COMPLETED"):
            raise ValueError("invalid outcome")
        if type(row["independent_success"]) is not bool:
            raise ValueError("success must be boolean")
        if (verified or row["independent_success"]) and (not row["reviewer"] or not row["evidence"]):
            raise ValueError("verification requires reviewer and evidence references")
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
            "pending": ["four lanes on one commit", "A1-A7 live evidence", "DBA signoff", "policy owner signoff", "independent signoff"], "groups": groups}


def generate(seed, output):
    tasks, oracle = cases(seed)
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    payloads = {"tasks.json": tasks, "oracle.json": oracle, "observations.json": observations(tasks, oracle)}
    hashes = {}
    for name, value in payloads.items():
        raw = (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode()
        (output / name).write_bytes(raw)
        hashes[name] = hashlib.sha256(raw).hexdigest()
    (output / "manifest.json").write_text(json.dumps({"baseline": BASELINE,
        "synthetic_only": True, "seed_sha256": hashlib.sha256(seed.encode()).hexdigest(),
        "product_execution": "NOT_RUN", "sha256": hashes}, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    gen = sub.add_parser("generate")
    gen.add_argument("--seed", required=True)
    gen.add_argument("--output", required=True, type=Path)
    scoring = sub.add_parser("score")
    scoring.add_argument("observations", type=Path)
    args = parser.parse_args()
    if args.command == "generate":
        generate(args.seed, args.output)
    else:
        print(json.dumps(score(json.loads(args.observations.read_text())), indent=2))


if __name__ == "__main__":
    main()
