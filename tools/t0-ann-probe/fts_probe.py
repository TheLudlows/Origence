#!/usr/bin/env python3
"""Bounded launcher and independent raw-evidence auditor; never installs or builds.

The complete result set is ONLY an audit oracle, never a serving strategy.
Native score-valid TopK and strict artifact-ID tie compatibility are distinct gates.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re

from probe import baseline_evidence, lock_errors
import subprocess
import time

HERE = Path(__file__).resolve().parent
SCHEMA = "origence-t0-lance-fts-v1"
CAP = 1000
BATCH = 64
DEADLINE = 180
SCOPE = "tenant = 'tenant-a' AND workspace = 'workspace-a'"
PHASES = ("indexed", "fresh_before_refresh", "mutated_before_refresh", "after_refresh", "after_compact", "reopened")
BASIC = {"rank": (["quartz"], 1), "and": (["alpha", "beta"], 10),
         "chinese_and": (["江南", "城市"], 10), "stopword": (["the"], 10),
         "case_sensitive": (["Running"], 10), "no_stem": (["run"], 10),
         "no_ascii_fold": (["café"], 10), "literal_operators": (["AND", "OR", "alpha-beta"], 10),
         "missing": (["absentterm"], 10), "ties": (["tieanchor"], 10),
         "deep_authority": (["authoritytest"], 10), "authority_prefilter": (["authoritytest"], 10)}
LIFECYCLE = {"old_term": (["beforetoken"], 10), "new_term": (["aftertoken"], 10),
             "deleted_term": (["deleteprobe"], 10), "fresh_term": (["freshlexeme"], 10),
             "statistics_rank": (["quartz"], 10)}
EXPECTED = {("indexed", name): value for name, value in BASIC.items()} | {
    (phase, name): value for phase in PHASES for name, value in LIFECYCLE.items()}
NOT_RUN = {"turso_authority_integration", "production_security", "summary_cross_document_AND",
           "summary_placement", "learning_closure", "production_identity", "efficient_tie_complete_refill", "10000_row_scale"}
IDENTITY = ("tenant", "workspace", "id", "source", "version", "generation")
TOKENIZER = {"base": "whitespace", "lower_case": False, "stem": False,
             "remove_stop_words": False, "ascii_folding": False, "max_token_length": None}
SCORER_SOURCE = "https://github.com/lance-format/lance/blob/v13.0.0/rust/lance-index/src/scalar/inverted/scorer.rs"


def fixture(phase="indexed"):
    def row(id_, text):
        return dict(tenant="tenant-a", workspace="workspace-a", id=id_, source=f"source-{id_}",
                    version=2, generation=7, text=text, eligible=True, state="current")
    rows = [row("rank-strong", "quartz quartz quartz"), row("rank-weak", "quartz " + "ballast " * 20),
            row("and-both", "alpha beta"), row("and-alpha", "alpha"), row("and-beta", "beta"),
            row("zh-both", "江南 城市"), row("zh-first", "江南"), row("zh-second", "城市"),
            row("literal", "the Running café alpha-beta AND OR"), row("literal-decoy", "run running cafe alpha beta"),
            row("mutable", "beforetoken lifecyclestay"), row("deletable", "deleteprobe")]
    rows += [row(f"tie-{i:03}", "tieanchor parity") for i in reversed(range(96))]
    rows += [row(f"authority-ok-{i:02}", "authoritytest ballast ballast") for i in range(12)]
    for i in range(12):
        r = row(f"authority-bad-{i:02}", "authoritytest")
        r.update(eligible=False, state=("fixture_ineligible_version_label", "fixture_ineligible_generation_label", "fixture_withdrawn_label", "fixture_deleted_label")[i % 4])
        rows.append(r)
    for i in range(120):
        r = row(f"foreign-{i:03}", "quartz alpha beta tieanchor")
        r["tenant" if i % 2 == 0 else "workspace"] = "tenant-b" if i % 2 == 0 else "workspace-b"
        rows.append(r)
    r = row("tie-000", "tieanchor parity")
    r["workspace"] = "workspace-b"
    rows.append(r)
    if phase != "indexed":
        rows.append(row("fresh-row", "freshlexeme"))
    if phase in PHASES[2:]:
        rows = [r for r in rows if r["id"] != "deletable"]
        next(r for r in rows if r["id"] == "mutable")["text"] = "aftertoken lifecyclestay"
    return rows


def key(h):
    return tuple(h[x] for x in ("tenant", "workspace", "id"))


def identity(h):
    return tuple(h[x] for x in IDENTITY)


def scoped(r):
    return r["tenant"] == "tenant-a" and r["workspace"] == "workspace-a"


def authority_filter(rows):
    clauses = [f"(id = '{r['id']}' AND source_id = '{r['source']}' AND version = {r['version']} AND generation = {r['generation']})"
               for r in rows if scoped(r) and r["eligible"]]
    return SCOPE + " AND (" + " OR ".join(clauses) + ")"


def rank_key(h):
    return (-h["score"], *key(h))


def finite(x):
    return type(x) in (int, float) and math.isfinite(x)


def same_hits(a, b):
    return len(a) == len(b) and all(identity(x) == identity(y) and
        math.isclose(x["score"], y["score"], rel_tol=1e-6, abs_tol=1e-6) for x, y in zip(a, b))


def valid_membership(raw, sorted_pool, k):
    wanted = min(k, len(sorted_pool))
    if len(raw) != wanted or any(a["score"] < b["score"] for a, b in zip(raw, raw[1:])):
        return False
    allowed = {identity(h): h["score"] for h in sorted_pool}
    if any(identity(h) not in allowed for h in raw):
        return False
    if not wanted:
        return True
    cutoff = sorted_pool[wanted-1]["score"]
    returned = {identity(h) for h in raw}
    return all(h["score"] >= cutoff for h in raw) and all(identity(h) in returned for h in sorted_pool if h["score"] > cutoff)


def reference_bm25(rows, terms):
    """Independent float64 BM25, fixed source k1=1.2,b=.75, global stats.

    Initial fully indexed fixture uses nonempty documents <=21 tokens, within
    lossless document-norm range. No claim for updated/deleted/fresh statistics.
    Native f32 is compared with fixed 2e-5 absolute / 1e-5 relative tolerance.
    """
    docs = [r["text"].split() for r in rows]
    n = len(docs)
    avgdl = sum(map(len, docs)) / n
    df = {term: sum(term in d for d in docs) for term in terms}
    out = {}
    for r, doc in zip(rows, docs):
        if not all(term in doc for term in terms):
            continue
        score = sum(math.log(1 + (n - df[t] + .5)/(df[t] + .5)) *
                    (2.2 * doc.count(t))/(doc.count(t) + 1.2 * (.25 + .75 * len(doc)/avgdl))
                    for t in terms)
        out[identity(r)] = score
    return out


def audit_hits(hits, rows, terms, label, errors, require_scope=True):
    if not isinstance(hits, list) or len(hits) > CAP:
        errors.append(f"{label}: missing/unbounded raw hits")
        return []
    lookup = {identity(r): r for r in rows}
    seen = set()
    for h in hits:
        if not isinstance(h, dict) or any(f not in h for f in (*IDENTITY, "score")):
            errors.append(f"{label}: incomplete retained identity")
            return []
        if any(type(h[f]) is not str for f in IDENTITY[:4]) or any(type(h[f]) is not int for f in IDENTITY[4:]):
            errors.append(f"{label}: invalid identity type")
            return []
        if identity(h) not in lookup or (require_scope and not scoped(h)):
            errors.append(f"{label}: foreign or stale retained identity")
        elif not all(t in lookup[identity(h)]["text"].split() for t in terms):
            errors.append(f"{label}: native AND/literal token mismatch")
        if key(h) in seen:
            errors.append(f"{label}: duplicate scoped identity")
        seen.add(key(h))
        if not finite(h["score"]) or h["score"] <= 0:
            errors.append(f"{label}: nonpositive/nonfinite native BM25")
            return []
    return hits


def audit_authority(result, inputs, rows, errors, label):
    mutation = result.get("mutation")
    expected_status = "OK" if mutation == "none" else "AUTHORITY_REVOKED" if mutation == "revoked_key" else "SNAPSHOT_CHANGED"
    if result.get("authority_kind") != "in_memory_fixture_only" or result.get("authority_native_calls") != 0:
        errors.append(f"{label}: authority fixture misrepresented as native")
    if result.get("batch_size") != BATCH or result.get("fixture_batch_calls") != math.ceil(len(inputs)/BATCH) or result.get("checked_rows") != len(inputs):
        errors.append(f"{label}: authority batch/N+1 count mismatch")
    if not finite(result.get("elapsed_ms")) or result["elapsed_ms"] < 0:
        errors.append(f"{label}: authority timing missing")
    if result.get("status") != expected_status:
        errors.append(f"{label}: wrong authority outcome")
    retained = result.get("hits")
    if mutation == "none":
        if retained != inputs:
            errors.append(f"{label}: clean fresh check changed retained evidence")
        allowed = {identity(r) for r in rows if scoped(r) and r["eligible"]}
        if any(identity(h) not in allowed for h in inputs):
            errors.append(f"{label}: ineligible evidence escaped authority")
    elif retained != [] or not inputs:
        errors.append(f"{label}: stale/revoked evidence did not fail closed")


def audit_case(event, rows):
    errors, blockers = [], []
    label = f"{event.get('phase')}/{event.get('name')}"
    expected = EXPECTED.get((event.get("phase"), event.get("name")))
    if expected is None or event.get("terms") != expected[0] or event.get("k") != expected[1]:
        return {"label": label, "errors": ["unexpected case parameters"], "blockers": []}
    terms, k = expected
    prefiltered = event["name"] == "authority_prefilter"
    for field, value in {"scope_filter": authority_filter(rows) if prefiltered else SCOPE,
                         "prefilter_mode": "fixture_full_identity" if prefiltered else "scope_only",
                         "enumerated_authority_rows": len(rows) if prefiltered else 0, "prefilter": True, "fast_search": False,
                         "operator": "AND", "fuzziness": 0, "wand_factor": 1.0, "oracle_only": True}.items():
        if event.get(field) != value:
            errors.append(f"{label}: changed {field}")
    raw = audit_hits(event.get("candidate_raw"), rows, terms, label + "/raw", errors)
    oracle = audit_hits(event.get("oracle_raw"), rows, terms, label + "/oracle", errors)
    matched = {identity(r) for r in rows if scoped(r) and all(t in r["text"].split() for t in terms)}
    complete = {identity(h) for h in oracle} == matched and len(oracle) == len(matched)
    if not complete or event.get("oracle_complete") is not True or event.get("expected_scoped_matches") != len(matched):
        errors.append(f"{label}: complete native oracle missing/truncated")
    # Audit BOTH raw streams before sorting the independent oracle copy.
    raw_order = all(a["score"] >= b["score"] for a, b in zip(raw, raw[1:]))
    oracle_order = all(a["score"] >= b["score"] for a, b in zip(oracle, oracle[1:]))
    if not raw_order or not oracle_order:
        errors.append(f"{label}: raw native score stream is not descending")
    score_map = {identity(h): h["score"] for h in oracle}
    if any(identity(h) not in score_map or not math.isclose(h["score"], score_map[identity(h)], rel_tol=1e-6, abs_tol=1e-6) for h in raw):
        errors.append(f"{label}: candidate score provenance differs from complete native scores")
    ordered = sorted(oracle, key=rank_key)
    allowed_ids = {identity(r) for r in rows if scoped(r) and r["eligible"]}
    competition = [h for h in ordered if not prefiltered or identity(h) in allowed_ids]
    topk = competition[:k]
    cutoff = topk[-1]["score"] if topk else None
    actual_ids = {identity(h) for h in raw}
    bm25_valid = complete and raw_order and len(raw) == len(topk) and all(identity(h) in {identity(c) for c in competition} for h in raw)
    if cutoff is not None:
        bm25_valid = bm25_valid and all(h["score"] >= cutoff for h in raw) and all(identity(h) in actual_ids for h in competition if h["score"] > cutoff)
    if not bm25_valid:
        errors.append(f"{label}: invalid BM25 TopK excluding tie-equivalent substitutions")
    strict = same_hits(raw, topk)
    if not strict and event["name"] != "deep_authority":
        blockers.append(f"{label}: native score/ID cutoff compatibility is not preserved")
    eligible = {identity(r) for r in rows if scoped(r) and r["eligible"]}
    checked = [h for h in raw if identity(h) in eligible]
    expected_eligible = [h for h in ordered if identity(h) in eligible]
    filtered = valid_membership(checked, expected_eligible, k)
    if event.get("checked_candidates") != checked:
        errors.append(f"{label}: batch authority output disagrees with fixture")
    if not filtered and event["name"] != "deep_authority":
        blockers.append(f"{label}: scoped TopK then authority lacks exact filtered TopK completeness")
    audit_authority(event.get("fresh_check", {}), checked, rows, errors, label)
    for field, expected_value in {"native_bm25_topk": bm25_valid, "native_score_descending": raw_order,
                                  "native_strict_topk": strict, "native_filtered_topk": filtered}.items():
        if event.get(field) is not expected_value:
            errors.append(f"{label}: native scalar {field} disagrees with retained evidence")
    expected_status = "EXPECTED_NEGATIVE_CONTROL" if event["name"] == "deep_authority" and not filtered and bm25_valid else "PASS_BOUNDED_FIXTURE" if strict and filtered and bm25_valid else "BLOCKER"
    if event.get("candidate_status") != expected_status:
        errors.append(f"{label}: candidate status overclaims acceptance")
    calls = {"candidate": 1, "oracle": 1, "plan": 1, "authority_native": 0,
             "authority_fixture_batches": math.ceil(len(raw)/BATCH)}
    if event.get("calls") != calls:
        errors.append(f"{label}: native/authority call counts disagree")
    times = event.get("timing_ms", {})
    if set(times) != {"eligibility_fixture_enumeration", "candidate_native", "candidate_authority_fixture", "candidate_total", "oracle_native", "plan"} or any(not finite(v) or v < 0 for v in times.values()):
        errors.append(f"{label}: stage timing missing/invalid")
    elif times["candidate_total"] + .01 < times["candidate_native"] + times["candidate_authority_fixture"]:
        errors.append(f"{label}: candidate wall excludes measured work")
    plan = event.get("plan", "")
    if not isinstance(plan, str) or re.search(r"(?m)^\s*MatchQuery:", plan) is None:
        errors.append(f"{label}: native FTS MatchQuery plan missing")
    absolute = "NOT_RUN_FRESH_STATISTICS"
    max_error = None
    if event["phase"] == "indexed":
        expected_scores = reference_bm25(rows, terms)
        differences = [abs(h["score"] - expected_scores[identity(h)]) for h in oracle if identity(h) in expected_scores]
        max_error = max(differences, default=0)
        absolute = "PASS" if complete and all(math.isclose(h["score"], expected_scores.get(identity(h), -1), rel_tol=1e-5, abs_tol=2e-5) for h in oracle) else "FAIL"
        if absolute == "FAIL":
            errors.append(f"{label}: independent global BM25 reference mismatch")
    return dict(label=label, errors=errors, blockers=blockers, raw_score_descending=raw_order,
                bm25_valid_topk=bm25_valid, strict_id_topk=strict, exact_filtered_topk=filtered, negative_control=event["name"] == "deep_authority",
                absolute_bm25=absolute, max_absolute_bm25_error=max_error,
                cutoff_score=cutoff, cutoff_tie_count=sum(h["score"] == cutoff for h in oracle) if cutoff is not None else 0)


def audit(records):
    errors, blockers, results = [], [], []
    headers = [r for r in records if r.get("kind") == "header"]
    header = headers[0] if len(headers) == 1 else {}
    if header.get("schema") != SCHEMA or header.get("lancedb") != "0.40.0" or header.get("arrow") != "58":
        errors.append("missing/version-mismatched header")
    if header.get("max_rows") != CAP or header.get("deadline_seconds") != DEADLINE or header.get("tokenizer") != TOKENIZER:
        errors.append("bounded rung/tokenizer/deadline changed")
    if set(header.get("not_run", [])) != NOT_RUN or header.get("authority_kind") != "in_memory_fixture_only" or header.get("full_enumeration") != "diagnostic_oracle_only" or header.get("serving_strategy") != "native_scoped_topk_with_fixture_authority_prefilter_arm":
        errors.append("fixture/oracle/production boundaries changed")
    snapshots = [r for r in records if r.get("kind") == "snapshot"]
    if [r.get("phase") for r in snapshots] != list(PHASES):
        errors.append("missing/duplicate/out-of-order lifecycle snapshots")
    previous_version = 0
    for snapshot in snapshots:
        phase = snapshot.get("phase")
        if phase not in PHASES or snapshot.get("fixture") != fixture(phase):
            errors.append(f"{phase}: canonical fixture altered")
            continue
        version = snapshot.get("table_version")
        if type(version) is not int or version < 1 or version < previous_version:
            errors.append(f"{phase}: native version missing/regressed")
        else:
            if phase in ("fresh_before_refresh", "mutated_before_refresh") and version <= previous_version:
                errors.append(f"{phase}: write did not advance native snapshot")
            if phase == "reopened" and version != previous_version:
                errors.append("reopen changed same-version snapshot")
            previous_version = version
        if snapshot.get("native_calls") != 4:
            errors.append(f"{phase}: snapshot native call count wrong")
        indices = snapshot.get("indices", [])
        if len(indices) != 1 or indices[0].get("name") != "t0_chunk_terms" or indices[0].get("columns") != ["text"] or indices[0].get("type") != "FTS":
            errors.append(f"{phase}: physical FTS index evidence missing")
        stats = snapshot.get("stats", {})
        if stats.get("type") != "FTS" or any(type(stats.get(k)) is not int or stats[k] < 0 for k in ("indexed_rows", "unindexed_rows")):
            errors.append(f"{phase}: index coverage statistics missing")
        elif phase in ("indexed", "after_refresh", "after_compact", "reopened") and (stats["unindexed_rows"] != 0 or stats["indexed_rows"] != len(fixture(phase))):
            errors.append(f"{phase}: fully indexed lifecycle claim unsupported")
        elif phase == "fresh_before_refresh" and stats["unindexed_rows"] != 1:
            errors.append("fresh row was not demonstrably unindexed")
        if snapshot.get("layout", {}).get("rows") != len(fixture(phase)) or type(snapshot.get("layout", {}).get("fragments")) is not int or snapshot["layout"]["fragments"] < 1:
            errors.append(f"{phase}: native layout row count differs")
    setups = [r for r in records if r.get("kind") == "setup"]
    expected_params = {"base_tokenizer": "whitespace", "lower_case": False, "stem": False,
                       "remove_stop_words": False, "ascii_folding": False, "max_token_length": None}
    if len(setups) != 1 or setups[0].get("native_calls") != 3 or any(setups[0].get("index_parameters", {}).get(k, "absent") != v for k, v in expected_params.items()):
        errors.append("explicit tokenizer/index creation parameters missing or altered")
    reopen = [r for r in records if r.get("kind") == "reopen"]
    if len(reopen) != 1 or reopen[0].get("native_calls") != 2 or reopen[0].get("mode") != "new_connection_same_process":
        errors.append("actual new-connection reopen evidence missing")
    cases = [r for r in records if r.get("kind") == "case"]
    names = [(r.get("phase"), r.get("name")) for r in cases]
    if set(names) != set(EXPECTED) or len(names) != len(EXPECTED):
        errors.append("missing/duplicate/unexpected native case coverage")
    for event in cases:
        try:
            result = audit_case(event, fixture(event.get("phase")))
        except (KeyError, TypeError, ValueError, OverflowError) as exc:
            result = dict(label=f"{event.get('phase')}/{event.get('name')}", errors=[f"malformed raw evidence: {type(exc).__name__}"], blockers=[])
        results.append(result)
        errors.extend(result["errors"])
        blockers.extend(result["blockers"])
    empty = [r for r in records if r.get("kind") == "empty_terms"]
    if len(empty) != 1 or empty[0].get("native_calls") != 0 or empty[0].get("hits") != []:
        errors.append("empty tokens issued native work or missing evidence")
    mutations = [r for r in records if r.get("kind") == "fresh_authority"]
    if {r.get("result", {}).get("mutation") for r in mutations} != {"version", "generation", "source", "withdrawn", "revoked_key"} or len(mutations) != 5:
        errors.append("fresh authority adversary coverage incomplete")
    for event in mutations:
        inputs = audit_hits(event.get("input_hits"), fixture(), ["quartz"], "fresh input", errors)
        audit_authority(event.get("result", {}), inputs, fixture(), errors, "fresh adversary")
    controls = [r for r in records if r.get("kind") == "unscoped_control"]
    if len(controls) != 1:
        errors.append("unscoped adversarial control missing")
    else:
        control = controls[0]
        hits = audit_hits(control.get("hits"), fixture(), ["quartz"], "unscoped control", errors, False)
        expected = {identity(r) for r in fixture() if "quartz" in r["text"].split()}
        if {identity(h) for h in hits} != expected or sum(not scoped(h) for h in hits) != 120 or control.get("native_calls") != 1 or control.get("oracle_only") is not True:
            errors.append("unscoped native control does not expose all foreign competitors")
    ordered = [r for r in records if r.get("kind") == "ordered_api"]
    if len(ordered) != 1 or ordered[0].get("native_calls") != 1:
        errors.append("explicit native score/ID API diagnostic missing")
    elif ordered[0].get("status") == "UNSUPPORTED":
        if not ordered[0].get("error"):
            errors.append("ordered API error detail missing")
        blockers.append("explicit _score DESC/id ASC API unsupported; no bounded tie-complete refill proved")
    elif ordered[0].get("status") == "EXECUTED":
        ordered_hits = audit_hits(ordered[0].get("hits"), fixture(), ["tieanchor"], "ordered API", errors)
        tie_case = next((r for r in cases if r.get("phase") == "indexed" and r.get("name") == "ties"), {})
        expected_ties = sorted(tie_case.get("oracle_raw", []), key=rank_key)[:10]
        if not same_hits(ordered_hits, expected_ties):
            blockers.append("explicit order API does not preserve global score/ID cutoff")
    else:
        errors.append("invalid ordered API diagnostic outcome")
    actions = [r for r in records if r.get("kind") == "mutation"]
    if [r.get("action") for r in actions] != ["insert", "update", "delete", "OptimizeAction::Index", "OptimizeAction::Compact"] or any(r.get("native_calls") != 1 for r in actions):
        errors.append("insert/update/delete/Index/Compact lifecycle incomplete")
    for r in actions:
        if r["action"].startswith("OptimizeAction") and r.get("prune") is not False:
            errors.append("maintenance prune boundary missing")
    before = next((r for r in cases if (r["phase"], r["name"]) == ("mutated_before_refresh", "statistics_rank")), None)
    after = next((r for r in cases if (r["phase"], r["name"]) == ("after_refresh", "statistics_rank")), None)
    changes = {"status": "NOT_RUN"}
    if before and after:
        a = {identity(h): h["score"] for h in before["oracle_raw"]}
        b = {identity(h): h["score"] for h in after["oracle_raw"]}
        changes = {"status": "OBSERVED", "same_visible_corpus": fixture("mutated_before_refresh") == fixture("after_refresh"),
                   "score_deltas": [{"identity": list(k), "before": a[k], "after": b.get(k), "delta": b[k]-a[k] if k in b else None} for k in a],
                   "rank_changed": [identity(h) for h in before["candidate_raw"]] != [identity(h) for h in after["candidate_raw"]],
                   "stability_required": False}
    terminal = [r for r in records if r.get("kind") == "terminal"]
    if len(terminal) != 1 or terminal[0].get("status") != "COMPLETED_NATIVE_MATRIX":
        errors.append("native matrix incomplete; missing work NOT_RUN")
    elif not finite(terminal[0].get("elapsed_ms")) or not 0 <= terminal[0]["elapsed_ms"] < DEADLINE*1000:
        errors.append("native measurement deadline exceeded")
    # Every actual public native request is counted separately from fixture lookups.
    native_calls = sum(r.get("native_calls", 0) for r in records if type(r.get("native_calls", 0)) is int)
    native_calls += sum(sum(r.get("calls", {}).get(k, 0) for k in ("candidate", "oracle", "plan", "authority_native")) for r in cases)
    return {"schema": SCHEMA + "-audit", "status": "FAIL_INVALID_OR_FAILED_EVIDENCE" if errors else "BLOCKER" if blockers else "PASS_BOUNDED_DIAGNOSTIC",
            "errors": errors, "blockers": blockers, "cases": results, "native_calls": native_calls,
            "bm25_reference": {"source": SCORER_SOURCE, "k1": 1.2, "b": .75, "statistics": "physical_index_global", "scope": "initial_fully_indexed_fixture_only"},
            "refresh_observation": changes, "not_run": sorted(NOT_RUN),
            "production_acceptance": False, "oracle_is_serving_strategy": False}


def read_records(path):
    if path.stat().st_size > 16*1024*1024:
        raise ValueError("raw evidence exceeds 16 MiB")
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--report", type=Path, help="audit an existing native JSONL without execution")
    args = parser.parse_args(argv)
    if args.report:
        report = audit(read_records(args.report))
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return 0 if report["status"] == "PASS_BOUNDED_DIAGNOSTIC" else 1
    if not args.binary or not args.output:
        parser.error("--binary and --output are required unless --report is supplied")
    binary = args.binary.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error("native binary missing/not executable; this launcher never builds or installs")
    dependency_errors = lock_errors(HERE/"Cargo.lock")
    baseline = baseline_evidence()
    if dependency_errors or not baseline["matches"]:
        parser.error("NOT_RUN: locked dependency/source baseline preflight failed: " + "; ".join(dependency_errors))
    paths = [HERE/"Cargo.toml", HERE/"Cargo.lock", HERE/"src/main.rs", HERE/"src/fixture.rs", HERE/"src/policy.rs", HERE/"src/fts.rs", Path(__file__), HERE/"probe.py"]
    source_before = {str(p.relative_to(HERE)): digest(p) for p in paths}
    binary_before = digest(binary)
    args.output.mkdir(parents=True, exist_ok=False)
    (args.output/"environment.json").write_text(json.dumps({"baseline": baseline, "source_sha256": source_before,
        "binary_sha256": binary_before, "build_verification": "Hash identity only; exact-head --locked build log must be retained by CI."}, indent=2)+"\n")
    raw = args.output.resolve()/"native.jsonl"
    started = time.monotonic()
    timed_out = False
    with (args.output/"native.stdout").open("xb") as out, (args.output/"native.stderr").open("xb") as err:
        proc = subprocess.Popen([str(binary), "fts-v1", str(raw)], stdout=out, stderr=err)
        try:
            rc = proc.wait(timeout=DEADLINE)
        except subprocess.TimeoutExpired:
            timed_out = True
            proc.kill()
            rc = proc.wait()
    elapsed = time.monotonic()-started
    try:
        records = read_records(raw) if raw.exists() else []
        report = audit(records)
    except (ValueError, TypeError, KeyError, OSError) as exc:
        report = {"schema": SCHEMA+"-audit", "status": "FAIL_INVALID_OR_FAILED_EVIDENCE", "errors": [f"unreadable native evidence: {exc}"], "blockers": [], "production_acceptance": False}
    if rc != 0 or timed_out or elapsed > DEADLINE:
        report["status"] = "FAIL_INVALID_OR_FAILED_EVIDENCE"
        report["errors"].append("native process failed/deadline; unrun work remains NOT_RUN")
    if digest(binary) != binary_before or any(digest(HERE/p) != value for p, value in source_before.items()):
        report["status"] = "FAIL_INVALID_OR_FAILED_EVIDENCE"
        report["errors"].append("binary/source changed during measurement")
    report["execution"] = {"exit_code": rc, "native_process_killed": timed_out, "native_subprocesses": False, "outer_timeout_shares_process_group": True, "hard_deadline_seconds": DEADLINE,
                           "wall_seconds": elapsed, "binary_sha256": digest(binary), "platform": platform.platform(),
                           "source_sha256": source_before, "build_verification": "Requires retained exact-head --locked build log; hashes do not prove compilation provenance",
                           "raw_sha256": digest(raw) if raw.exists() else None}
    (args.output/"report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2)+"\n")
    print(json.dumps({"status": report["status"], "report": str(args.output/"report.json"), "errors": len(report["errors"]), "blockers": len(report["blockers"])}))
    return 0 if report["status"] == "PASS_BOUNDED_DIAGNOSTIC" else 1


if __name__ == "__main__":
    raise SystemExit(main())
