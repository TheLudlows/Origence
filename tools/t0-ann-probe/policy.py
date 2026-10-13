#!/usr/bin/env python3
"""Run exactly two fresh bounded builds of the frozen filter-policy experiment."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import signal
import subprocess
import sys
import tempfile
import time

import probe
import policy_report

HERE = Path(__file__).resolve().parent
CONTRACT_PATH = HERE / "policy-contract.json"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def contract() -> dict:
    return json.loads(CONTRACT_PATH.read_text())


def audit_report(path: Path, native_exit: int | None = None, timed_out: bool = False) -> dict:
    try:
        if path.stat().st_size > 256 * 1024 * 1024:
            raise ValueError("Native report exceeds 256 MiB audit bound; original retained without truncation")
        report = json.loads(path.read_text())
        if not isinstance(report,dict):
            raise ValueError("Native report must be an object")
        errors = policy_report.validate_report(report)
        benefit = policy_report.benefit_summary(report, quality_errors=errors)
    except (OSError, ValueError, TypeError, KeyError, OverflowError) as error:
        report = {}
        errors = [f"Missing or malformed native policy report: {error}"]
        benefit = {"performance_passed": False, "reason": "no complete report"}
    if timed_out:
        errors.append("Native process deadline reached; isolated temporary fixture may remain under runs.")
    if native_exit is not None and native_exit != 0:
        errors.append(f"native process exit={native_exit}")
    pool = report.get("plan_pool", {})
    initial_uuids = set()
    cases = report.get("cases")
    for case in cases if isinstance(cases,list) else []:
        if not isinstance(case,dict) or case.get("phase")!="indexed" or case.get("selected_route")!="ann":
            continue
        refs = case.get("slot_plan_hashes")
        for entry in refs if isinstance(refs,list) else []:
            text = pool.get(entry.get("selected")) if isinstance(pool,dict) and isinstance(entry,dict) else None
            if isinstance(text,str):
                initial_uuids.update(re.findall(r"uuid=([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})",text))
    initial_uuids = sorted(initial_uuids)
    if len(initial_uuids) != 1:
        errors.append("Exactly one stable initial ANN index UUID per fresh build is required")
    quality_passed = not errors
    performance_passed = benefit.get("performance_passed") is True
    return {"report":str(path),"report_sha256":sha256(path) if path.is_file() else None,
            "native_status":report.get("status"),"initial_index_uuids":initial_uuids,"quality_errors":errors,"quality_passed":quality_passed,
            "performance":benefit,"native_exit":native_exit,"timed_out":timed_out,
            "fixture_indexed_sha256":report.get("fixture_indexed_sha256"),"fixture_final_sha256":report.get("fixture_final_sha256"),
            "contract_sha256":report.get("contract_sha256"),
            "decision":"BOUNDED_CANDIDATE" if quality_passed and performance_passed else "RECOMMEND_EXACT"}


def combine(results: list[dict]) -> dict:
    errors = []
    if len(results) != 2:
        errors.append("Exactly two retained fresh builds are required for this frozen experiment.")
    for key in ("fixture_indexed_sha256","fixture_final_sha256","contract_sha256"):
        values = [r.get(key) for r in results]
        if len(values) != 2 or not all(isinstance(v,str) and len(v)==64 for v in values) or len(set(values)) != 1:
            errors.append(f"Build identity mismatch or missing {key}")
    if len({r.get("report") for r in results}) != 2:
        errors.append("Two distinct report paths are required; one report cannot count twice.")
    identities = [set(r.get("initial_index_uuids", [])) for r in results]
    if len(identities)!=2 or any(len(value)!=1 for value in identities) or identities[0] & identities[1]:
        errors.append("Distinct fresh initial index UUIDs are required for the two retained builds.")
    quality = not errors and all(r["quality_passed"] for r in results)
    benefit = not errors and all(r["performance"].get("performance_passed") is True for r in results)
    return {"schema":"origence-t0-filter-policy-two-builds-v1","policy_id":contract()["policy_id"],
            "status":"BOUNDED_CANDIDATE" if quality and benefit else "RECOMMEND_EXACT",
            "quality_passed":quality,"performance_passed":benefit,"integrity_errors":errors,"builds":results,
            "interpretation":"Two fresh rebuilds on this frozen synthetic mix only; not independent randomized trials or production acceptance.",
            "stop_rule":contract()["stop_rule"]}


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--execute", action="store_true")
    p.add_argument("--report", action="append", type=Path, help="audit two existing native reports; repeat twice")
    p.add_argument("--binary", type=Path, help="prebuilt locked executable; no build/install is performed")
    p.add_argument("--timeout-seconds", type=int, choices=(900,), default=900)
    args = p.parse_args()
    if args.report:
        if args.execute:
            raise SystemExit("--report and --execute are mutually exclusive")
        result = combine([audit_report(path.resolve()) for path in args.report])
        print(json.dumps(result,indent=2))
        return int(result["status"] != "BOUNDED_CANDIDATE")
    frozen = contract()
    baseline = probe.baseline_evidence()
    binary = (args.binary or HERE / "target/release/origence-t0-ann-probe").resolve()
    lock = HERE / "Cargo.lock"
    dependency_errors = probe.lock_errors(lock)
    plan = {"status":"PLANNED_NOT_RUN","contract":frozen,"contract_sha256":sha256(CONTRACT_PATH),
            "lancedb":probe.LANCEDB_VERSION,"lock_compatible":not dependency_errors,"lock_errors":dependency_errors,
            "baseline_matches":baseline["matches"],"binary_exists":binary.is_file(),"lock_exists":lock.is_file(),
            "native_processes":2,"native_deadline_seconds_each":900,"raw_vector_bytes_per_build":5120000,
            "memory_note":"No hard RAM bound; only fixed rows/dimension/query matrix and native process deadline.",
            "selection_note":"Frozen before new results; no routing inputs from vector distances, answers, observed recall or returned candidate count."}
    if not args.execute:
        print(json.dumps(plan,indent=2))
        return 0
    if not baseline["matches"]:
        raise SystemExit("Application baseline mismatch; no native run started")
    if dependency_errors:
        raise SystemExit("NOT_RUN: Cargo lock bootstrap required; " + "; ".join(dependency_errors))
    if not binary.is_file():
        raise SystemExit("NOT_RUN: isolated Cargo lock and locked release executable required")
    source_names = ["Cargo.toml","Cargo.lock","src/main.rs","src/fixture.rs","src/policy.rs","src/fts.rs","probe.py","policy.py","policy_report.py","policy-contract.json"]
    hashes = {name:sha256(HERE/name) for name in source_names}
    environment = {**plan,"baseline":baseline,"source_sha256":hashes,"binary_sha256":sha256(binary),
                   "platform":platform.platform(),"python":sys.version,
                   "provenance_limit":"Retain matching --locked build log; a binary hash alone does not prove build provenance."}
    (HERE/"runs").mkdir(exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix="filter-policy-v1-",dir=HERE/"runs"))
    (output/"environment.json").write_text(json.dumps(environment,indent=2)+"\n")
    results=[]
    for number in (1,2):
        build = output/f"build-{number}";build.mkdir()
        config = build/"config.json";config.write_text(json.dumps(frozen["config"],indent=2)+"\n")
        report = build/"native-report.json"
        timed_out=False;started=time.monotonic()
        with (build/"native.log").open("w") as log:
            process=subprocess.Popen([str(binary),str(config),str(report),"filter-policy-v1"],cwd=HERE,
                                     stdout=log,stderr=subprocess.STDOUT,start_new_session=(os.name=="posix"))
            try:
                exit_code=process.wait(timeout=900)
            except subprocess.TimeoutExpired:
                timed_out=True
                if os.name=="posix":os.killpg(process.pid,signal.SIGKILL)
                else:process.kill()
                exit_code=process.wait()
        native_wall=time.monotonic()-started
        audit_started=time.monotonic()
        result=audit_report(report,exit_code,timed_out)
        result["native_process_wall_seconds"]=native_wall
        result["python_audit_wall_seconds"]=time.monotonic()-audit_started
        result["fresh_build_number"]=number
        if sha256(binary)!=environment["binary_sha256"] or any(sha256(HERE/name)!=value for name,value in hashes.items()):
            result["quality_errors"].append("Executable/source identity changed during the two-build experiment")
            result["quality_passed"]=False;result["decision"]="RECOMMEND_EXACT"
        (build/"driver-result.json").write_text(json.dumps(result,indent=2)+"\n")
        results.append(result)
        print(json.dumps({"build":number,"path":str(build),"decision":result["decision"],"quality_errors":len(result["quality_errors"])}),flush=True)
        # Preserve the second predeclared rebuild even if the first quality/benefit gate fails.
    combined=combine(results)
    (output/"driver-result.json").write_text(json.dumps(combined,indent=2)+"\n")
    (output/"summary.txt").write_text("Frozen filter-policy experiment\n"+json.dumps(combined,indent=2)+"\n")
    print(output);print(combined["status"])
    return int(combined["status"]!="BOUNDED_CANDIDATE")


if __name__=="__main__":
    raise SystemExit(main())
