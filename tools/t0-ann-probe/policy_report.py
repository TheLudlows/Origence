#!/usr/bin/env python3
"""Independent, fail-closed reader for the frozen synthetic filter-policy probe.

No engine execution or parameter tuning occurs here. The retained native oracle
is audited, not independently replaced by an exhaustive Python nearest-neighbor
search. Synthetic authority and full-vector distance checks are regenerated
from the frozen fixture, independently of native pass flags and counters.
"""
from __future__ import annotations

from functools import lru_cache
import hashlib
import math
import re
from pathlib import Path
import statistics
import struct

HERE = Path(__file__).resolve().parent
SCHEMA = "origence-t0-filter-policy-v1"
POLICY_ID = "eligible4096-fraction40-ivf32of64-v1"
CONFIG = {"rows": 10000, "dimension": 128, "queries_per_bucket": 8,
          "partitions": 64, "nprobes": 32, "refine_factor": 10,
          "index_kind": "ivf_flat", "seed": 52}
CONTRACT_SHA256 = hashlib.sha256((HERE / "policy-contract.json").read_bytes()).hexdigest()
PHASES = ("indexed", "fresh_before_refresh", "fresh_after_refresh", "reopened")
BASE_BUCKETS = ("empty", "one", "lt_10", "lt_100", "dense_current", "sparse",
                "other_tenant", "other_workspace", "source_version", "historical_generation")
COUNTS = (100, 101, 512, 513, 2048, 4095, 4096, 4097)
BUCKETS = BASE_BUCKETS + tuple(f"count_{n}" for n in COUNTS) + tuple(
    f"rotated_{n}" for n in (4095, 4096, 4097)) + ("subsample_2", "subsample_3")
IDENTITY = ("id", "source", "tenant", "workspace", "version", "generation")
INVARIANTS = ("visibility_violations", "source_violations", "score_distance_violations", "duplicate_ids")
COMPONENTS = ("eligibility", "authority_map", "selection_filter_query", "native_decode_sort", "verification")
EMPTY_PLAN = "EMPTY_NO_NATIVE_QUERY"
SETUP_SDK_CALLS = {"connect": 1, "create_empty_table": 1, "add": 40, "compact": 1,
                   "create_index": 1, "index_refresh": 1, "open_table": 1,
                   "list_indices": 4, "index_stats": 4, "table_stats": 5,
                   "table_version": 13, "checkout": 4, "checkout_latest": 3}
CALL_COUNT_SCOPE = "Completed explicit LanceDB SDK calls, not underlying file reads/RPCs. Vector execution and explain calls are separate in call_counts."
UUID_PATTERN = re.compile(r"\buuid=([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})(?![0-9a-fA-F-])")


def finite(value):
    if type(value) not in (int, float):
        return False
    try:
        return math.isfinite(value)
    except OverflowError:
        return False


def f32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def _mix(value):
    mask = (1 << 64) - 1
    value = (value + 0x9E3779B97F4A7C15) & mask
    value = ((value ^ (value >> 30)) * 0xBF58476D1CE4E5B9) & mask
    value = ((value ^ (value >> 27)) * 0x94D049BB133111EB) & mask
    return value ^ (value >> 31)


@lru_cache(maxsize=10000)
def fixture_row(index):
    fresh = index >= 9984
    logical, historical = index // 2, index < 9984 and index % 2 == 0
    category = logical % 23
    return {"id": f"artifact-{index:09}", "source": f"source-{logical // 8:07}",
            "tenant": "tenant-b" if not fresh and category == 0 else "tenant-a",
            "workspace": "workspace-b" if not fresh and category == 1 else "workspace-a",
            "version": 1 if historical else 2, "generation": 0 if historical else 1,
            "visible": fresh or category != 2, "fresh": fresh}


@lru_cache(maxsize=10000)
def fixture_vector(index):
    cluster = (index // 2) % 16
    output = []
    for dimension in range(128):
        center = (_mix(52 ^ (cluster * 128 + dimension)) >> 40) / (1 << 24)
        noise = (_mix(52 ^ (index * 128 + dimension) ^ 0xA55A) >> 40) / (1 << 24)
        output.append(f32(f32(center * 2.0) + f32(noise * f32(0.12))))
    return tuple(output)


def vector_sha256(vector):
    return hashlib.sha256(struct.pack("<128f", *vector)).hexdigest()


def ids_sha256(ids):
    digest = hashlib.sha256(struct.pack("<Q", len(ids)))
    for identifier in ids:
        value = identifier.encode("utf-8")
        digest.update(struct.pack("<Q", len(value)))
        digest.update(value)
    return digest.hexdigest()


@lru_cache(maxsize=2)
def fixture_sha256(count):
    digest = hashlib.sha256(struct.pack("<Q", count))
    for index in range(count):
        row = fixture_row(index)
        for key in ("id", "source", "tenant", "workspace"):
            value = row[key].encode("utf-8")
            digest.update(struct.pack("<Q", len(value)))
            digest.update(value)
        digest.update(struct.pack("<iqBBQ", row["version"], row["generation"],
                                  row["visible"], row["fresh"], 128))
        digest.update(struct.pack("<128f", *fixture_vector(index)))
    return digest.hexdigest()


def selected_route(eligible_count, snapshot_rows):
    if eligible_count == 0:
        return "empty"
    return "ann" if eligible_count >= 4096 and eligible_count * 5 >= snapshot_rows * 2 else "exact"


@lru_cache(maxsize=1)
def expected_cases():
    """Regenerate selection order, metadata and cardinalities from the fixture."""
    result = {}
    for phase in PHASES:
        names = ("fresh_unindexed",) if phase == "reopened" else BUCKETS + (
            () if phase == "indexed" else ("fresh_unindexed",))
        count = 9984 if phase == "indexed" else 10000
        for bucket in names:
            scope = {"tenant": "tenant-a", "workspace": "workspace-a", "version": 2,
                     "generation": 1, "source": None}
            if bucket == "other_tenant":
                scope["tenant"] = "tenant-b"
            if bucket == "other_workspace":
                scope["workspace"] = "workspace-b"
            if bucket == "historical_generation":
                scope.update(version=1, generation=0)
            if bucket == "source_version":
                scope["source"] = "source-0000001"
            base_stride = 97 if bucket == "sparse" else 1
            base_cap = {"empty": 0, "one": 1, "lt_10": 7, "lt_100": 37}.get(bucket)
            candidates = []
            for index in range(count):
                row = fixture_row(index)
                if (row["visible"] and all(row[key] == scope[key] for key in
                    ("tenant", "workspace", "version", "generation")) and
                    (scope["source"] is None or row["source"] == scope["source"]) and
                    (bucket != "fresh_unindexed" or row["fresh"])):
                    candidates.append(index)
            candidates = candidates[::base_stride][:base_cap]
            rotation = 541 if bucket.startswith("rotated_") else 0
            case_stride = int(bucket.rsplit("_", 1)[1]) if bucket.startswith("subsample_") else 1
            case_cap = int(bucket.rsplit("_", 1)[1]) if bucket.startswith(("count_", "rotated_")) else None
            offset = rotation % len(candidates) if candidates else 0
            candidates = (candidates[offset:] + candidates[:offset])[::case_stride][:case_cap]
            identifiers = [fixture_row(index)["id"] for index in candidates]
            case_id = f"{phase}/{bucket}"
            result[case_id] = {"case_id": case_id, "phase": phase, "bucket": bucket,
                "snapshot_rows": count, "eligible_count": len(candidates), "eligible_ids": identifiers,
                "eligible_ids_sha256": ids_sha256(identifiers), "scope": scope,
                "base_stride": base_stride, "base_cap": base_cap, "rotation": rotation,
                "case_stride": case_stride, "case_cap": case_cap,
                "selected_route": selected_route(len(candidates), count),
                "_indices": tuple(candidates), "_allowed": frozenset(identifiers)}
    return result


@lru_cache(maxsize=576)
def expected_query(case_id, slot):
    eligible = expected_cases()[case_id]["_indices"]
    if not eligible:
        return (0.25,) * 128
    if slot < 4:
        offset = 17 + slot
        anchor = fixture_vector(eligible[(offset * 997) % len(eligible)])
        return tuple(f32(value + f32(((dimension + offset) % 11) * f32(0.00013)))
                     for dimension, value in enumerate(anchor))
    bridge = slot - 4
    left, right = fixture_vector(2 * (3 * bridge + 1)), fixture_vector(2 * (3 * bridge + 8))
    return tuple(f32(f32(f32(a * 0.5) + f32(b * 0.5)) +
                     f32(((dimension + bridge) % 13) * f32(0.00017)))
                 for dimension, (a, b) in enumerate(zip(left, right)))


@lru_cache(maxsize=150000)
def _distance(case_id, slot, index):
    return sum((a - b) ** 2 for a, b in zip(fixture_vector(index), expected_query(case_id, slot)))


def plan_uses_ann(plan):
    return isinstance(plan, str) and any(marker in plan for marker in ("ANNIVF", "ANNIvf", "ANNHNSW"))


def _object(value):
    return value if isinstance(value, dict) else {}


def _equal_value(actual, expected):
    # Python's True == 1 must not make forged counts/configuration valid.
    if type(actual) is not type(expected):
        return False
    if isinstance(expected, dict):
        return actual.keys() == expected.keys() and all(_equal_value(actual[key], value) for key, value in expected.items())
    if isinstance(expected, list):
        return len(actual) == len(expected) and all(_equal_value(a, b) for a, b in zip(actual, expected))
    return actual == expected


def _check_hits(value, case, slot, label, errors):
    if not isinstance(value, list):
        errors.append(f"{label}: retained hits missing")
        return []
    if len(value) > 100:
        errors.append(f"{label}: candidate limit exceeded")
    seen = set()
    safe = []
    for hit in value:
        if (not isinstance(hit, dict) or any(not isinstance(hit.get(key), str) for key in IDENTITY[:4])
                or any(type(hit.get(key)) is not int for key in IDENTITY[4:])
                or not finite(hit.get("distance")) or hit["distance"] < 0
                or not finite(hit.get("score"))):
            errors.append(f"{label}: incomplete or nonfinite hit")
            continue
        safe.append(hit)
        identifier = hit["id"]
        if identifier in seen:
            errors.append(f"{label}: duplicate ID")
        seen.add(identifier)
        try:
            index = int(identifier.removeprefix("artifact-"))
        except ValueError:
            index = -1
        if (index < 0 or index >= case["snapshot_rows"] or
                fixture_row(index)["id"] != identifier or identifier not in case["_allowed"]):
            errors.append(f"{label}: authority/eligibility violation")
            continue
        row = fixture_row(index)
        if any(hit[key] != row[key] for key in IDENTITY):
            errors.append(f"{label}: source/version/generation/scope violation")
        l2 = _distance(case["case_id"], slot, index)
        if (abs(hit["distance"] - l2) > 0.0005 * (1 + l2) or
                abs(hit["score"] - 1 / (1 + l2)) > 0.0005 or
                not math.isclose(hit["score"], 1 / (1 + hit["distance"]), rel_tol=1e-6, abs_tol=1e-7)):
            errors.append(f"{label}: full-vector distance/score violation")
    if safe != sorted(safe, key=lambda hit: (-hit["score"], hit["id"])):
        errors.append(f"{label}: score/ID ordering changed")
    return safe


def same_exact(left, right):
    return len(left) == len(right) and all(
        all(a[key] == b[key] for key in IDENTITY)
        and abs(a["distance"] - b["distance"]) <= 0.0001 * (1 + abs(a["distance"]))
        and abs(a["score"] - b["score"]) <= 0.00001
        for a, b in zip(left, right))


def recall(exact, actual, k):
    expected = {hit["id"] for hit in exact[:k]}
    return len(expected & {hit["id"] for hit in actual[:k]}) / len(expected) if expected else None


def _audit_arm(value, case, slot, label, expected_route, errors):
    arm = _object(value)
    if arm.get("route") != expected_route:
        errors.append(f"{label}: frozen route mismatch")
    for key, expected in (("native_calls", int(case["eligible_count"] > 0)),
                          ("eligible_count", case["eligible_count"])):
        if not _equal_value(arm.get(key), expected):
            errors.append(f"{label}: {key} mismatch")
    wall = arm.get("wall_ms")
    if not finite(wall) or wall <= 0:
        errors.append(f"{label}: positive finite full-pipeline wall time missing")
    components = _object(arm.get("components_ms"))
    if set(components) != set(COMPONENTS) or any(not finite(value) or value < 0 for value in components.values()):
        errors.append(f"{label}: complete nonnegative timing components missing")
    elif finite(wall) and sum(components.values()) > wall + max(1e-9, abs(wall) * 1e-9):
        errors.append(f"{label}: component sum exceeds full-pipeline wall time")
    raw = _check_hits(arm.get("raw_hits"), case, slot, f"{label}/raw", errors)
    verified = _check_hits(arm.get("verified_hits"), case, slot, f"{label}/verified", errors)
    if raw != verified:
        errors.append(f"{label}: final verification changed supposedly clean raw hits")
    if case["eligible_count"] == 0 and (raw or verified):
        errors.append(f"{label}: empty path returned hits")
    return verified


def _audit_evidence(report, cases, errors):
    evidence = (("indexed_evidence", 9984, 0), ("fresh_evidence_before_refresh", 9984, 16),
                ("fresh_evidence_after_refresh", 10000, 0), ("reopened_evidence", 10000, 0))
    expected_indices = [{"name": "t0_vector_l2", "type": "IVF_FLAT", "columns": ["vector"]}]
    for key, indexed, unindexed in evidence:
        item = _object(report.get(key))
        stats = _object(item.get("index_stats"))
        if item.get("list_indices") != expected_indices:
            errors.append(f"{key}: index-list metadata mismatch")
        if (not _equal_value(stats.get("indexed_rows"), indexed) or
                not _equal_value(stats.get("unindexed_rows"), unindexed) or
                stats.get("type") != "IVF_FLAT" or stats.get("distance_type") != "L2"):
            errors.append(f"{key}: index metric/type/coverage mismatch")
    layouts = {}
    for key, rows in (("layout_before_compact", 9984), ("layout_after_compact", 9984),
                      ("layout_after_index", 9984), ("layout_before_index_refresh", 10000),
                      ("layout_after_index_refresh", 10000)):
        layout = _object(report.get(key))
        layouts[key] = layout
        if (not _equal_value(layout.get("rows"), rows) or
                any(type(layout.get(field)) is not int or layout[field] <= 0 for field in ("version", "fragments", "bytes"))):
            errors.append(f"{key}: layout evidence missing or inconsistent")
    for before, after in (("layout_after_compact", "layout_after_index"),
                          ("layout_before_index_refresh", "layout_after_index_refresh")):
        if layouts[before].get("fragments") != layouts[after].get("fragments"):
            errors.append(f"{after}: index action changed physical fragment layout")
    phase_layout = {"indexed": "layout_after_index", "fresh_before_refresh": "layout_before_index_refresh",
                    "fresh_after_refresh": "layout_after_index_refresh", "reopened": "layout_after_index_refresh"}
    for case in cases:
        if not isinstance(case, dict) or case.get("phase") not in phase_layout:
            continue
        snapshot = case.get("snapshot")
        if type(snapshot) is not int or snapshot <= 0 or snapshot != layouts[phase_layout[case["phase"]]].get("version"):
            errors.append(f"{case.get('case_id')}: wrong fixed snapshot")
    versions = [layouts[key].get("version") for key in
                ("layout_after_index", "layout_before_index_refresh", "layout_after_index_refresh")]
    if all(type(value) is int for value in versions) and not versions[0] < versions[1] < versions[2]:
        errors.append("fresh append and index refresh must establish distinct snapshots")


def validate_report(report):
    """Return all integrity/quality failures; performance is a separate gate."""
    errors = []
    if not isinstance(report, dict):
        return ["report is not an object"]
    for key, value in (("schema", SCHEMA), ("policy_id", POLICY_ID), ("config", CONFIG),
                       ("contract_sha256", CONTRACT_SHA256), ("lancedb", "0.23.1"),
                       ("candidate_limit", 100), ("setup_sdk_calls", SETUP_SDK_CALLS),
                       ("call_count_scope", CALL_COUNT_SCOPE), ("timing_protocol", "pair_then_oracle_audit_then_bucket_plans")):
        if not _equal_value(report.get(key), value):
            errors.append(f"{key}: frozen contract mismatch")
    if isinstance(report.get("config"), dict) and any(
            not _equal_value(report["config"].get(key), value) for key, value in CONFIG.items()):
        errors.append("config: frozen value types mismatch")
    if report.get("status") != "PASS_QUALITY":
        errors.append(f"native status: {report.get('status', 'missing')}")
    if report.get("temporary_database_removed") is not True:
        errors.append("temporary database cleanup unverified")
    for key, count in (("fixture_indexed_sha256", 9984), ("fixture_final_sha256", 10000)):
        if report.get(key) != fixture_sha256(count):
            errors.append(f"{key}: frozen canonical fixture digest mismatch")
    expected = expected_cases()
    cases = report.get("cases")
    if not isinstance(cases, list):
        errors.append("case coverage missing")
        cases = []
    ids = [case.get("case_id") if isinstance(case, dict) else None for case in cases]
    if len(ids) != len(expected) or any(not isinstance(value, str) for value in ids) or set(
            value for value in ids if isinstance(value, str)) != set(expected):
        errors.append("missing, duplicate or unexpected case coverage")
    pool = _object(report.get("plan_pool"))
    if not pool:
        errors.append("actual plan pool missing")
    for key, plan in pool.items():
        if not isinstance(plan, str) or not plan:
            errors.append("plan pool: actual plan missing")
        elif key != hashlib.sha256(plan.encode("utf-8")).hexdigest():
            errors.append("plan pool: content hash mismatch")
    used_plans = set()
    for actual in cases:
        case = _object(actual)
        identifier = case.get("case_id")
        if not isinstance(identifier, str) or identifier not in expected:
            continue
        want = expected[identifier]
        for key, value in want.items():
            if not key.startswith("_") and not _equal_value(case.get(key), value):
                errors.append(f"{identifier}: {key} does not match frozen eligibility/shape")
        if case.get("plan_pass") is not True:
            errors.append(f"{identifier}: native plan claim missing or failed")
        if "selected_plan" in case or "exact_plan" in case:
            errors.append(f"{identifier}: obsolete single-vector plan fields present")
        slots = case.get("slot_plan_hashes")
        if not isinstance(slots, list):
            slots = []
        slot_numbers = [value.get("slot") for value in slots if isinstance(value, dict) and type(value.get("slot")) is int]
        if len(slots) != 8 or len(slot_numbers) != 8 or set(slot_numbers) != set(range(8)):
            errors.append(f"{identifier}: missing, duplicate or unexpected per-vector plan coverage")
        for item in slots:
            item = _object(item)
            slot = item.get("slot")
            if type(slot) is not int or slot not in range(8):
                continue
            if set(item) != {"slot", "selected", "exact"}:
                errors.append(f"{identifier}/{slot}: incomplete per-vector plan references")
            for arm, route in (("selected", want["selected_route"]),
                               ("exact", "empty" if want["eligible_count"] == 0 else "exact")):
                label = f"{identifier}/{slot}/{arm}"
                reference = item.get(arm)
                if not isinstance(reference, str):
                    errors.append(f"{label}: plan hash reference missing")
                    continue
                used_plans.add(reference)
                if slot == 0 and case.get(f"{arm}_plan_sha256") != reference:
                    errors.append(f"{label}: first-slot plan hash mismatch")
                plan = pool.get(reference)
                if not isinstance(plan, str) or not plan:
                    errors.append(f"{label}: actual plan missing from pool")
                    continue
                if ((route == "empty" and plan != EMPTY_PLAN) or
                        (route != "empty" and plan == EMPTY_PLAN) or
                        (route == "ann") != plan_uses_ann(plan)):
                    errors.append(f"{label}: plan contradicts frozen selected route")
                if route == "exact" and "KNNVectorDistance" not in plan:
                    errors.append(f"{label}: exhaustive KNNVectorDistance plan missing")
                if route == "ann" and not UUID_PATTERN.search(plan):
                    errors.append(f"{label}: native ANN physical index UUID missing")
                if route == "ann" and ("minimum_nprobes=32" not in plan or "maximum_nprobes=Some(32)" not in plan):
                    errors.append(f"{label}: fixed 32-probe plan evidence missing")
    if set(pool) != used_plans:
        errors.append("plan pool has unused or missing referenced entries")
    _audit_evidence(report, cases, errors)
    queries = report.get("queries")
    if not isinstance(queries, list):
        errors.append("query coverage missing")
        queries = []
    wanted_queries = {(identifier, slot) for identifier in expected for slot in range(8)}
    actual_queries = []
    for value in queries:
        query = _object(value)
        if isinstance(query.get("case_id"), str) and type(query.get("slot")) is int:
            actual_queries.append((query["case_id"], query["slot"]))
    if len(queries) != len(wanted_queries) or len(actual_queries) != len(queries) or set(actual_queries) != wanted_queries:
        errors.append("missing, duplicate or unexpected query coverage")
    for value in queries:
        query = _object(value)
        identifier, slot = query.get("case_id"), query.get("slot")
        if not isinstance(identifier, str) or identifier not in expected or type(slot) is not int or slot not in range(8):
            continue
        case = expected[identifier]
        label = f"{identifier}/{slot}"
        for key, want in (("phase", case["phase"]), ("bucket", case["bucket"]),
                          ("query_kind", "near" if slot < 4 else "bridge"), ("selected_first", slot % 2 == 0)):
            if not _equal_value(query.get(key), want):
                errors.append(f"{label}: {key}/balanced order mismatch")
        vector = query.get("query_vector")
        valid_vector = isinstance(vector, list) and len(vector) == 128 and all(finite(item) and abs(item) <= 3.4028234663852886e38 for item in vector)
        expected_hash = vector_sha256(expected_query(identifier, slot))
        if (not valid_vector or vector_sha256(vector) != expected_hash or
                query.get("query_sha256") != expected_hash):
            errors.append(f"{label}: frozen query vector/hash mismatch")
        selected = _audit_arm(query.get("selected"), case, slot, label + "/selected", case["selected_route"], errors)
        exact = _audit_arm(query.get("exact"), case, slot, label + "/exact",
                           "empty" if case["eligible_count"] == 0 else "exact", errors)
        oracle = _check_hits(query.get("oracle_hits"), case, slot, label + "/oracle", errors)
        if len(oracle) != min(100, case["eligible_count"]):
            errors.append(f"{label}: retained oracle count mismatch")
        if len(exact) != min(100, case["eligible_count"]) or not same_exact(exact, oracle):
            errors.append(f"{label}: exact does not match retained oracle at 100%")
        if query.get("exact_matches_oracle") is not True:
            errors.append(f"{label}: exact oracle claim missing or false")
        selected_exact = case["selected_route"] != "ann"
        if selected_exact and not same_exact(selected, oracle):
            errors.append(f"{label}: selected exact does not match retained oracle at 100%")
        if query.get("selected_exact_matches_oracle") is not (True if selected_exact else None):
            errors.append(f"{label}: selected exact oracle claim contradicts route")
        for k in (10, 100):
            measured, claimed = recall(oracle, selected, k), query.get(f"recall_at_{k}")
            if (measured is None and claimed is not None) or (measured is not None and (
                    not finite(claimed) or not math.isclose(measured, claimed, rel_tol=0, abs_tol=1e-12))):
                errors.append(f"{label}: retained-hit recall@{k} contradicts scalar")
            if measured is not None and measured < 0.95:
                errors.append(f"{label}: per-pair recall@{k} below 0.95")
        for key in ("selected_raw_invariants", "selected_verified_invariants", "exact_raw_invariants",
                    "exact_verified_invariants", "oracle_invariants"):
            checks = _object(query.get(key))
            if set(checks) != set(INVARIANTS) or any(type(value) is not int or value != 0 for value in checks.values()):
                errors.append(f"{label}: {key} missing/nonzero")
        if query.get("quality_pass") is not True:
            errors.append(f"{label}: native quality claim missing or failed")
    nonempty = sum(case["eligible_count"] > 0 for case in expected.values())
    counts = _object(report.get("call_counts"))
    expected_counts = {"selected_native": nonempty * 8, "exact_native": nonempty * 8,
                       "oracle_native": nonempty * 8, "explain_plans": nonempty * 8 * 2}
    if set(counts) != set(expected_counts) or any(not _equal_value(counts.get(key), value) for key, value in expected_counts.items()):
        errors.append("physical native/explain call counts contradict frozen zero/one-call matrix")
    return errors


def benefit_summary(report, quality_errors=None):
    """Compute single-rebuild exploratory gates from independent arm wall times.

The caller must require both fresh rebuilds to pass separately. A combined mean
cannot rescue either build, either quality failure, or an individual case.
"""
    if quality_errors is None:
        quality_errors = validate_report(report)
    ratios, ann_ratios, by_case = [], [], {}
    timed_pairs = []
    selected_total = exact_total = 0.0
    queries = _object(report).get("queries", [])
    for value in queries if isinstance(queries, list) else []:
        query = _object(value)
        case = expected_cases().get(query.get("case_id")) if isinstance(query.get("case_id"), str) else None
        selected, exact = _object(query.get("selected")), _object(query.get("exact"))
        left, right = selected.get("wall_ms"), exact.get("wall_ms")
        if case is None or not finite(left) or left <= 0 or not finite(right) or right <= 0:
            continue
        ratio = left / right
        ratios.append(ratio)
        if type(query.get("slot")) is int:
            timed_pairs.append((case["case_id"], query["slot"]))
        selected_total += left
        exact_total += right
        by_case.setdefault(case["case_id"], []).append(ratio)
        if case["selected_route"] == "ann":
            ann_ratios.append(ratio)
    workload = selected_total / exact_total if exact_total > 0 else None
    ann_median = statistics.median(ann_ratios) if ann_ratios else None
    medians = {key: statistics.median(value) for key, value in sorted(by_case.items())}
    expected_pairs = {(identifier, slot) for identifier in expected_cases() for slot in range(8)}
    complete_timings = (len(ratios) == 576 and len(medians) == 72
                        and set(timed_pairs) == expected_pairs
                        and all(len(value) == 8 for value in by_case.values()))
    failures = []
    if not complete_timings:
        failures.append("complete frozen workload timing coverage missing")
    if workload is None or not finite(workload) or workload > 0.90:
        failures.append("workload sum(selected)/sum(exact) exceeds 0.90 or is missing")
    if ann_median is None or not finite(ann_median) or ann_median > 0.90:
        failures.append("ANN-selected median paired ratio exceeds 0.90 or is missing")
    for key, median in medians.items():
        if not finite(median) or median > 1.10:
            failures.append(f"{key}: median paired ratio exceeds 1.10")
    performance_passed = not failures
    return {"performance_passed": performance_passed, "quality_passed": not quality_errors,
            "workload_sum_time_ratio": workload, "ann_median_paired_ratio": ann_median,
            "per_bucket_median_paired_ratio": medians, "selected_wall_sum_ms": selected_total,
            "exact_wall_sum_ms": exact_total, "performance_errors": failures,
            "recommendation": "candidate_for_tested_mix" if performance_passed and not quality_errors else "exact",
            "interpretation": "Exploratory artificial fixed mix, one rebuild only; both fresh rebuilds must pass separately. Not production, cold-I/O, SQLite cost, or an SLO."}


def summarize(report, errors=None):
    if errors is None:
        errors = validate_report(report)
    benefit = benefit_summary(report, quality_errors=errors)
    lines = ["# Frozen filter-aware ANN T0", "", f"Native status: {_object(report).get('status', 'MISSING')}",
             f"Independent quality/integrity: {'FAIL' if errors else 'PASS'}",
             f"Exploratory performance: {'PASS' if benefit['performance_passed'] else 'FAIL'}",
             f"Recommendation: {benefit['recommendation']}", "", benefit["interpretation"], "",
             f"Workload sum ratio: {benefit['workload_sum_time_ratio']}",
             f"ANN median paired ratio: {benefit['ann_median_paired_ratio']}", "",
             "## Per-case median paired ratios", ""]
    lines += [f"- {key}: {value:.6f}" for key, value in benefit["per_bucket_median_paired_ratio"].items()]
    lines += ["", "## Quality/integrity failures", ""] + ([f"- {error}" for error in errors] or ["None."])
    lines += ["", "## Performance failures", ""] + ([f"- {error}" for error in benefit["performance_errors"]] or ["None."])
    lines += ["", "All failures remain visible. No threshold/probe tuning or production activation is authorized.",
              "Retained oracle identities and distances are audited; this reader does not independently execute an exhaustive nearest-neighbor search.", ""]
    return "\n".join(lines)
