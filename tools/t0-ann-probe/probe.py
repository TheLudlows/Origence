#!/usr/bin/env python3
"""Explicit bounded launcher/reporter; never installs software or generates a lock."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
SCHEMA = "origence-t0-ann-v1"
PHASES = ("indexed", "fresh_before_refresh", "fresh_after_refresh")
BUCKETS = ("empty", "one", "lt_10", "lt_100", "dense_current", "sparse",
           "other_tenant", "other_workspace", "source_version", "historical_generation")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def baseline_evidence() -> dict:
    expected = json.loads((HERE / "source-baseline.json").read_text())
    actual = {path: sha256(REPO / path) for path in expected["sha256"]}
    return {"commit": expected["commit"], "expected": expected["sha256"], "actual": actual,
            "matches": actual == expected["sha256"]}


def finite_number(value) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


IDENTITY_FIELDS = ("id", "source", "tenant", "workspace", "version", "generation")


def plan_uses_ann(plan) -> bool:
    return isinstance(plan, str) and any(marker in plan for marker in ("ANNIVF", "ANNIvf", "ANNHNSW"))


def audit_hits(container: dict, key: str, label: str, errors: list[str]) -> list[dict]:
    hits = container.get(key)
    if not isinstance(hits, list):
        errors.append(f"{label}: retained {key} missing")
        return []
    if len(hits) > 100:
        errors.append(f"{label}: retained {key} exceeds fixed candidate target")
    seen = set()
    for hit in hits:
        if not isinstance(hit, dict) or any(field not in hit for field in IDENTITY_FIELDS):
            errors.append(f"{label}: incomplete hit identity in {key}")
            return []
        if any(not isinstance(hit[f], str) for f in ("id", "source", "tenant", "workspace")) or any(type(hit[f]) is not int for f in ("version", "generation")):
            errors.append(f"{label}: invalid hit identity types in {key}")
            return []
        if hit["id"] in seen:
            errors.append(f"{label}: duplicate hit in {key}")
        seen.add(hit["id"])
        distance, score = hit.get("distance"), hit.get("score")
        if not finite_number(distance) or distance < 0 or not finite_number(score):
            errors.append(f"{label}: invalid distance/score in {key}")
            return []
        if not math.isclose(score, 1.0/(1.0+distance), rel_tol=1e-6, abs_tol=1e-7):
            errors.append(f"{label}: score mapping changed in {key}")
    if hits != sorted(hits, key=lambda h: (-h["score"], h["id"])):
        errors.append(f"{label}: score/ID order changed in {key}")
    return hits


def identity_distance_equal(left: list[dict], right: list[dict]) -> bool:
    return len(left) == len(right) and all(
        all(a[f] == b[f] for f in IDENTITY_FIELDS)
        and math.isclose(a["distance"], b["distance"], rel_tol=1e-4, abs_tol=1e-4)
        and math.isclose(a["score"], b["score"], rel_tol=0, abs_tol=1e-5)
        for a, b in zip(left, right))


def recomputed_recall(exact: list[dict], actual: list[dict], k: int) -> float | None:
    expected = {hit["id"] for hit in exact[:k]}
    return len(expected & {hit["id"] for hit in actual[:k]}) / len(expected) if expected else None


def audit_recall(container: dict, exact: list[dict], actual: list[dict], label: str, errors: list[str]) -> None:
    for k in (10, 100):
        measured = recomputed_recall(exact, actual, k)
        claimed = container.get(f"recall_at_{k}")
        if measured is None:
            if claimed is not None:
                errors.append(f"{label}: retained-hit recall@{k} disagrees with scalar")
        elif not finite_number(claimed) or not math.isclose(measured, claimed, rel_tol=0, abs_tol=1e-12):
            errors.append(f"{label}: retained-hit recall@{k} disagrees with scalar")


def validate_report(report: dict) -> list[str]:
    """Independently recompute gates; do not trust a Rust status label alone."""
    errors = []
    if report.get("schema") != SCHEMA:
        errors.append("unknown report schema")
    if report.get("status") != "PASS_SYNTHETIC_T0":
        errors.append(f"native status: {report.get('status', 'missing')}")
    if report.get("lancedb") != "0.23.1" or report.get("candidate_limit") != 100:
        errors.append("dependency or candidate-limit mismatch")
    config = report.get("config", {})
    for digest in ("fixture_indexed_sha256", "fixture_final_sha256"):
        value = report.get(digest, "")
        if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
            errors.append(f"{digest}: canonical fixture digest missing")
    rows = report.get("queries", [])
    if config.get("rows") not in (10000, 100000, 1000000) or config.get("dimension") not in (128, 384, 768, 1024):
        errors.append("unbounded configuration")
    expected = {(phase, bucket, n) for phase in PHASES
                for bucket in BUCKETS + (() if phase == "indexed" else ("fresh_unindexed",))
                for n in range(config.get("queries_per_bucket", 0))}
    actual = [(r.get("phase"), r.get("bucket"), r.get("query")) for r in rows]
    if not expected or set(actual) != expected or len(actual) != len(expected):
        errors.append("missing, duplicate or unexpected query coverage")
    reopened = report.get("reopened_fresh_query", {})
    if (reopened.get("phase"), reopened.get("bucket"), reopened.get("query")) != ("reopened", "fresh_unindexed", 0):
        errors.append("reopened fresh-query identity missing or wrong")
    phases = {}
    for row in rows + [report.get("reopened_fresh_query", {})]:
        label = f"{row.get('phase')}/{row.get('bucket')}/{row.get('query')}"
        exact_hits = audit_hits(row, "exact_hits", label, errors)
        ann_hits = audit_hits(row, "ann_hits", label, errors)
        verified_hits = audit_hits(row, "verified_ann_hits", label, errors)
        fallback_hits = audit_hits(row, "fallback_hits", label, errors)
        audit_recall(row, exact_hits, verified_hits, label, errors)
        if not identity_distance_equal(exact_hits, fallback_hits):
            errors.append(f"{label}: retained fallback differs from exact")
        if not identity_distance_equal(ann_hits, verified_hits):
            errors.append(f"{label}: supposedly clean ANN hits changed at final verification")
        if len(exact_hits) != row.get("exact_count") or len(ann_hits) != row.get("ann_count"):
            errors.append(f"{label}: retained hit counts disagree with scalars")
        if plan_uses_ann(row.get("ann_plan")) != row.get("native_ann_proved"):
            errors.append(f"{label}: ANN plan text contradicts proof flag")
        if not isinstance(row.get("exact_plan"), str) or not row.get("exact_plan") or plan_uses_ann(row.get("exact_plan")) or row.get("exact_bypass_proved") is not True:
            errors.append(f"{label}: exact plan text does not prove bypass")
        if row.get("passed") is not True:
            errors.append(f"{label}: native gate failed")
        if row.get("candidate_limit") != 100 or row.get("fallback_exact_match") is not True:
            errors.append(f"{label}: fixed target or exact fallback failed")
        eligible = row.get("eligible_count", -1)
        fixed_counts = {"empty":0,"one":1,"lt_10":7,"lt_100":37,"source_version":8,"fresh_unindexed":16}
        bucket = row.get("bucket")
        if bucket in fixed_counts and eligible != fixed_counts[bucket]:
            errors.append(f"{label}: required bucket cardinality {fixed_counts[bucket]} missing")
        elif bucket in ("dense_current", "historical_generation") and (not isinstance(eligible, int) or eligible <= 512):
            errors.append(f"{label}: dense/history bucket must exceed 512 rows")
        elif bucket not in fixed_counts and (not isinstance(eligible, int) or eligible <= 0):
            errors.append(f"{label}: representative bucket is empty")
        if not isinstance(eligible, int) or eligible < 0 or row.get("exact_count") != min(100, eligible):
            errors.append(f"{label}: exact count mismatch")
        for metric in ("recall_at_10", "recall_at_100"):
            value = row.get(metric)
            if eligible == 0:
                if value is not None or row.get("ann_count") != 0:
                    errors.append(f"{label}: empty query returned results or invented recall")
            elif not finite_number(value) or not .95 <= value <= 1:
                errors.append(f"{label}: {metric} below 0.95 or invalid")
        if row.get("exact_bypass_proved") is not True or (eligible > 0 and row.get("native_ann_proved") is not True):
            errors.append(f"{label}: native plan proof missing")
        for mode in ("exact_invariants", "ann_invariants", "fallback_invariants"):
            checks = row.get(mode, {})
            if set(checks) != {"visibility_violations", "source_violations", "score_distance_violations", "duplicate_ids"} or any(v != 0 for v in checks.values()):
                errors.append(f"{label}: {mode} not zero")
        phases.setdefault(row.get("phase"), set()).add(row.get("snapshot"))
        if row.get("query") == 0:
            batch = row.get("batched_512", {})
            batch_ann = audit_hits(batch, "ann_hits", label+"/batched", errors)
            batch_exact = audit_hits(batch, "exact_hits", label+"/batched", errors)
            batch_verified = audit_hits(batch, "verified_ann_hits", label+"/batched", errors)
            audit_recall(batch, exact_hits, batch_verified, label+"/batched", errors)
            if not identity_distance_equal(exact_hits, batch_exact):
                errors.append(f"{label}: retained batched exact differs from one-query exact")
            if not identity_distance_equal(batch_ann, batch_verified):
                errors.append(f"{label}: supposedly clean batched hits changed at final verification")
            if batch.get("native_calls_per_mode") != (max(eligible, 0) + 511) // 512:
                errors.append(f"{label}: wrong 512-ID query count")
            if batch.get("passed") is not True or batch.get("exact_matches_one_query") is not True:
                errors.append(f"{label}: 512-ID gate failed")
            for mode in ("invariants", "raw_ann_invariants", "raw_exact_invariants"):
                checks = batch.get(mode, {})
                if set(checks) != {"visibility_violations", "source_violations", "score_distance_violations", "duplicate_ids"} or any(v != 0 for v in checks.values()):
                    errors.append(f"{label}: batched {mode} not zero")
            for metric in ("recall_at_10", "recall_at_100"):
                v = batch.get(metric)
                if eligible > 0 and (not finite_number(v) or not .95 <= v <= 1):
                    errors.append(f"{label}: batched {metric} below 0.95")
            if eligible > 0:
                adversaries = row.get("synthetic_authority_adversaries", {})
                if set(adversaries) != {"wrong_source_version_generation_scope_rejected", "withdrawal_between_native_and_final_rejected"} or not all(v is True for v in adversaries.values()):
                    errors.append(f"{label}: synthetic authority adversaries failed")
    if any(len(versions) != 1 or None in versions for versions in phases.values()):
        errors.append("a phase did not use one fixed snapshot")
    if phases.get("reopened") != phases.get("fresh_after_refresh"):
        errors.append("reopened query is not on the final snapshot")
    total = config.get("rows")
    for key, indexed, unindexed in (("indexed_evidence", total - 16 if isinstance(total, int) else -1, 0),
                                    ("fresh_evidence_before_refresh", total - 16 if isinstance(total, int) else -1, 16),
                                    ("fresh_evidence_after_refresh", total, 0), ("reopened_evidence", total, 0)):
        stats = report.get(key, {}).get("index_stats", {})
        if stats.get("indexed_rows") != indexed or stats.get("unindexed_rows") != unindexed:
            errors.append(f"{key}: unexpected indexed/unindexed coverage")
        expected_type = config.get("index_kind", "").upper()
        if stats.get("distance_type") != "L2" or stats.get("type") != expected_type:
            errors.append(f"{key}: wrong metric/index type")
        indices = report.get(key, {}).get("list_indices")
        if indices != [{"name":"t0_vector_l2", "type":expected_type, "columns":["vector"]}]:
            errors.append(f"{key}: index-list metadata missing or inconsistent")
    if report.get("temporary_database_removed") is not True:
        errors.append("temporary database cleanup not verified")
    return errors


def percentile(values: list[float], percent: float) -> float | None:
    if not values:
        return None
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * percent) - 1)]


def summarize(report: dict, errors: list[str]) -> str:
    lines = ["# Synthetic T0 ANN result", "", f"Native status: {report.get('status', 'NOT_RUN')}",
             f"Independent report checks: {'PASS' if not errors else 'FAIL'}", "",
             "This is isolated synthetic evidence. It is not a production SLO, an application authorization test, or a real-corpus recall result.", ""]
    config = report.get("config", {})
    lines += ["Configuration: " + json.dumps(config, sort_keys=True), "",
              "Every query returns at most 100 candidates; Recall@10 and Recall@100 use the same candidate run.",
              "Per-bucket values below are minima. Plans are prepared before each pair. First native executions are retained; these are descriptive smoke percentiles, not cold or unbiased performance acceptance.", ""]
    queries = report.get("queries", [])
    for phase in PHASES:
        lines += [f"## {phase}", ""]
        for bucket in BUCKETS + (() if phase == "indexed" else ("fresh_unindexed",)):
            group = [q for q in queries if q.get("phase") == phase and q.get("bucket") == bucket]
            if not group:
                lines.append(f"- {bucket}: NOT_RUN")
                continue
            exact_times = [q["exact_native_ms"] for q in group]
            ann_times = [q["ann_native_ms"] for q in group]
            recalls = [[q[m] for q in group if q.get(m) is not None] for m in ("recall_at_10", "recall_at_100")]
            minima = [min(v) if v else None for v in recalls]
            lines.append(f"- {bucket}: n={len(group)}, eligible={group[0]['eligible_count']}, minimum Recall@10/@100={minima}; "
                         f"exact p50/p95={percentile(exact_times,.5):.3f}/{percentile(exact_times,.95):.3f} ms; "
                         f"ANN p50/p95={percentile(ann_times,.5):.3f}/{percentile(ann_times,.95):.3f} ms")
        lines.append("")
    lines += ["## Gate errors", ""] + ([f"- {e}" for e in errors] if errors else ["None."])
    lines += ["", "## Interpretation limits", ""] + [f"- {s}" for s in report.get("limitations", [])]
    lines += ["- Full query plans, all raw query rows, 512-ID enumeration/query/verification costs, build/refresh times, and index coverage are in native-report.json.",
              "- Native ANN failures remain failures even when forced exact fallback is correct. Nothing automatically changes parameters or promotes this to production.", ""]
    return "\n".join(lines)


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--execute", action="store_true", help="actually run a prebuilt locked executable; default is plan only")
    p.add_argument("--report", type=Path, help="validate/summarize an existing native JSON; no native execution")
    p.add_argument("--rows", type=int, choices=(10000, 100000, 1000000), default=10000)
    p.add_argument("--dimension", type=int, choices=(128, 384, 768, 1024), default=128)
    p.add_argument("--allow-scale", action="store_true", help="explicit opt-in to >10k rows or >128D; no automatic ladder")
    p.add_argument("--queries", type=int, choices=range(1,21), default=3)
    p.add_argument("--partitions", type=int, choices=range(2,257), default=64)
    p.add_argument("--nprobes", type=int, choices=range(1,257), default=32)
    p.add_argument("--refine-factor", type=int, choices=range(1,21), default=10)
    p.add_argument("--index", choices=("ivf_flat", "ivf_pq", "ivf_hnsw_pq"), default="ivf_flat")
    p.add_argument("--seed", type=int, default=52)
    p.add_argument("--timeout-seconds", type=int, default=1800, help="native process deadline, capped at 7200")
    p.add_argument("--binary", type=Path, help="prebuilt probe executable; default is isolated target/release")
    return p


def main() -> int:
    args = parser().parse_args()
    if args.report:
        if args.execute:
            raise SystemExit("--report and --execute are mutually exclusive")
        report = json.loads(args.report.read_text())
        errors = validate_report(report)
        print(summarize(report, errors))
        return int(bool(errors))
    if args.nprobes > args.partitions or not 1 <= args.timeout_seconds <= 7200 or not 0 <= args.seed < 2**64:
        raise SystemExit("invalid nprobes, timeout or seed")
    if (args.rows > 10000 or args.dimension > 128) and not args.allow_scale:
        raise SystemExit("scale ladder requires explicit --allow-scale")
    config = {"rows":args.rows,"dimension":args.dimension,"queries_per_bucket":args.queries,
              "partitions":args.partitions,"nprobes":args.nprobes,"refine_factor":args.refine_factor,
              "index_kind":args.index,"seed":args.seed}
    evidence = baseline_evidence()
    manifest = HERE / "Cargo.toml"
    lock = HERE / "Cargo.lock"
    binary = (args.binary or HERE / "target/release/origence-t0-ann-probe").resolve()
    plan = {"status":"PLANNED_NOT_RUN","config":config,"raw_vector_bytes":args.rows*args.dimension*4,
            "memory_warning":"Arrow, indexes, training and metadata add substantial overhead; this is not a RAM limit",
            "cargo_available":shutil.which("cargo") is not None,"rustc_available":shutil.which("rustc") is not None,
            "lock_exists":lock.exists(),"binary_exists":binary.is_file(),"baseline_matches":evidence["matches"],
            "manifest_sha256":sha256(manifest),"timeout_seconds":args.timeout_seconds}
    if not args.execute:
        print(json.dumps(plan, indent=2))
        return 0
    if not evidence["matches"]:
        raise SystemExit("baseline source files changed; reconcile baseline evidence before executing")
    if not lock.is_file() or not binary.is_file():
        raise SystemExit("NOT_RUN: generate Cargo.lock with Cargo, then build the isolated executable with --locked; see README")
    import tomllib
    locked = tomllib.loads(lock.read_text())
    lancedb = [p for p in locked.get("package", []) if p.get("name") == "lancedb"]
    if len(lancedb) != 1 or lancedb[0]["version"] != "0.23.1":
        raise SystemExit("Cargo.lock must resolve exactly lancedb 0.23.1")
    (HERE / "runs").mkdir(exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix=f"{args.index}-{args.rows}-{args.dimension}-", dir=HERE / "runs"))
    (output / "config.json").write_text(json.dumps(config, indent=2)+"\n")
    (output / "environment.json").write_text(json.dumps({**plan,"baseline":evidence,"platform":platform.platform(),
        "python":sys.version,"binary_sha256":sha256(binary),"lock_sha256":sha256(lock),
        "source_sha256":{p:sha256(HERE/p) for p in ("src/main.rs", "src/fixture.rs", "probe.py")},
        "build_verification":"A binary hash is recorded; the caller must retain the --locked build log proving its provenance."}, indent=2)+"\n")
    native = output / "native-report.json"
    started = time.monotonic()
    timeout = False
    with (output / "native.log").open("w") as log:
        process = subprocess.Popen([str(binary), str(output / "config.json"), str(native)], cwd=HERE,
            stdout=log, stderr=subprocess.STDOUT, start_new_session=(os.name == "posix"))
        try:
            code = process.wait(timeout=args.timeout_seconds)
        except subprocess.TimeoutExpired:
            timeout = True
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()
            code = process.wait()
    if native.is_file() and native.stat().st_size:
        try:
            report = json.loads(native.read_text())
            errors = validate_report(report)
        except (json.JSONDecodeError, ValueError, TypeError, KeyError) as error:
            report = {"schema":SCHEMA,"status":"MALFORMED_NATIVE_REPORT","config":config}
            errors = [f"Native report is incomplete or malformed: {error}"]
    else:
        report = {"schema":SCHEMA,"status":"TIMEOUT" if timeout else "FAILED_WITHOUT_REPORT","config":config}
        errors = ["Native report absent/incomplete; no result may be treated as a pass."]
    if code != 0:
        errors.append(f"native process exit={code}; timeout={timeout}")
    if timeout:
        errors.append("Hard process deadline reached; an isolated temporary fixture may remain under runs. No production path was opened.")
    (output / "summary.md").write_text(summarize(report, errors))
    (output / "driver-result.json").write_text(json.dumps({"status":"PASS_SYNTHETIC_T0" if not errors else "FAIL",
        "errors":errors,"elapsed_seconds":time.monotonic()-started,"native_exit":code,"timeout":timeout}, indent=2)+"\n")
    print(output)
    print("PASS_SYNTHETIC_T0" if not errors else "FAIL")
    return int(bool(errors))


if __name__ == "__main__":
    raise SystemExit(main())
