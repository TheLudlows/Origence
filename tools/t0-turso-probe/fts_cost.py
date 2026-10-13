#!/usr/bin/env python3
"""Frozen, synthetic Turso 0.8.2 scored-CTE FTS cost experiment.

No production adapter, existing database, model, or scale-up is opened. Python
wall timings include bridge IPC/serialization/instrumentation; they are not a
production Rust end-to-end benchmark. Run without arguments to inspect the plan.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import re
import signal
import tempfile
import time
import tomllib

SPEC = importlib.util.spec_from_file_location("t0_compatibility", Path(__file__).with_name("verify.py"))
compat = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(compat)
ROOT = compat.ROOT
CORPORA = (1000, 10000)
QUERIES = {"low": "lowtoken", "medium": "mediumtoken", "all": "commontoken"}
BUCKETS = ("source_1", "source_8", "source_64", "source_512", "dense")
KS = (10, 100)
BUDGET = 512
DEADLINE_SECONDS = 900
SCORE_TOLERANCE = 1e-9
POLICY = {
    "name": "t0-fts-materialized-global-bm25-cost-v1",
    "corpora": list(CORPORA), "query_strides": {"low": 100, "medium": 10, "all": 1},
    "buckets": list(BUCKETS), "branch_top_k": list(KS),
    "modes": ["unbudgeted", "guard_512"], "rounds": 2,
    "budget_matrix": "dense bucket/K100 only: one guard configuration per corpus/query",
    "orders": [["candidate", "exact"], ["exact", "candidate"]],
    "score_kind": "native_global_index_bm25_higher_is_better",
    "score_absolute_tolerance": SCORE_TOLERANCE,
    "tie_order": ["tenant_id", "workspace_id", "chunk_id"],
    "guard_read_rows": BUDGET, "guard_sentinel_rows": BUDGET + 1,
    "guard_is_hard_engine_cpu_or_memory_budget": False,
    "candidate_materialization": "all lexical matches before visibility and final branch LIMIT",
    "original_raw_join_failure": "retained in verify.py and historical evidence; not replaced",
    "authority": "actual key lookup plus current chunk/root/local-S2 SQL; no full service/Learning closure claim",
    "reference_and_explain_order": "after all measured pairs in each corpus/query phase",
    "wall_timing": "Python driver including IPC, serialization, instrumentation, authority and fresh verification",
    "native_timing": "bridge request timing: parameter conversion + SDK + row drain/JSON Value build; excludes connection setup, wire serialization/IPC; not pure engine CPU",
    "cache_state": "first measured execution retained; subsequent shared-store queries naturally warm caches",
    "request_seconds": 30, "process_seconds": 180, "driver_seconds": DEADLINE_SECONDS,
    "native_call_unit": "bridge requests; internal Connection::query/execute calls counted separately for setup",
    "connection_setup": "recorded SELECT 1 plus one implicit foreign_keys pragma per new process, outside measured pairs",
    "hard_driver_cap": "CI timeout --signal=KILL 900s covers process group; standalone SIGALRM stops native work with bounded cleanup",
    "max_bridge_rows": 10000, "max_request_bytes": 4 * 1024 * 1024,
    "scale_up": "not supported", "production_policy_selected": False,
    "latency_acceptance": "none; two ordered repetitions are descriptive, not an SLO",
}
T, W, KEY, PRINCIPAL = (compat.blob(n) for n in (11001, 11002, 11003, 11004))
TOKEN_HASH = "synthetic-t0-cost-key-not-a-secret"
META_COLUMNS = "c.tenant_id,c.workspace_id,c.id,c.asset_id,c.version,v.source_event_id"
SCORE_SQL = ("SELECT tenant_id,workspace_id,id,fts_score(search_terms,?1) AS score "
             "FROM oc_chunks WHERE fts_match(search_terms,?1)")
GUARD_SQL = ("SELECT tenant_id,workspace_id,id FROM oc_chunks "
             "WHERE fts_match(search_terms,?1) LIMIT 513")
AUTH_SQL = compat.actual_sql("src/storage/sqlite/relational.rs", "SELECT tenant_id, workspace_id, id, role FROM oc_api_keys WHERE")
CURRENT_SQL = compat.actual_sql("src/storage/sqlite/visibility.rs", "FROM oc_chunks c JOIN")
SUPPORT_SQL = compat.actual_sql("src/storage/sqlite/visibility.rs", "(json_extract(c.locator")


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def digest(value):
    return hashlib.sha256(encoded(value).encode()).hexdigest()


def scoped_key(row):
    assert len(row) >= 3, "missing full scoped key"
    values = tuple(value["blob"] for value in row[:3])
    assert all(re.fullmatch(r"[0-9a-f]{32}", value) for value in values), values
    return values


def metadata_key(row):
    assert len(row) >= 6, "missing authoritative asset/version/source metadata"
    return (*scoped_key(row), row[3]["blob"], row[4], row[5]["blob"])


def fixture(size):
    assert size in CORPORA, "frozen 1k/10k fixture only"
    eligible = size * 9 // 10
    groups = []
    counts = [1, 8, 64, 512, eligible - 585]
    for number, count in enumerate(counts):
        groups.append({"number": number, "count": count, "state": "eligible", "tenant": T, "workspace": W})
    invalid = ["wrong_tenant", "wrong_workspace", "withdrawn_root", "deleted_asset",
               "stale_version", "unpublished", "withdrawn_support"]
    remaining = size - eligible
    for number, state in enumerate(invalid, 5):
        count = remaining // len(invalid) + (number - 5 < remaining % len(invalid))
        groups.append({"number": number, "count": count, "state": state,
                       "tenant": compat.blob(12001) if state == "wrong_tenant" else T,
                       "workspace": compat.blob(12002 + number) if state in {"wrong_tenant", "wrong_workspace"} else W})
    rows = []
    for group in groups:
        number = group["number"]
        group["asset"] = compat.blob(20000 + number)
        group["source"] = compat.blob(30000 + number)
        group["support"] = compat.blob(40000 + number)
        for ordinal in range(group["count"]):
            # Wrong-scope groups reuse real eligible chunk IDs. ID-only checks
            # would accept collisions; full scoped identity and metadata cannot.
            identifier = 50000 + (ordinal if group["state"] in {"wrong_tenant", "wrong_workspace"} else number * 20000 + ordinal)
            terms = ["commontoken"] * (12 if group["state"] != "eligible" else 1 + ordinal % 4)
            if ordinal % 10 == 0:
                terms += ["mediumtoken"] * (5 if group["state"] != "eligible" else 1 + ordinal % 3)
            if ordinal % 100 == 0:
                terms += ["lowtoken"] * (5 if group["state"] != "eligible" else 1 + ordinal % 3)
            terms += ["filler"] * (0 if group["state"] != "eligible" else 8 + ordinal % 17)
            locator = {"parser": "memory-semantics:v1", "support_sources": [group["support"]["blob"]]} if group["state"] == "withdrawn_support" else {}
            rows.append({"meta": [group["tenant"], group["workspace"], compat.blob(identifier), group["asset"], 1, group["source"]],
                         "ordinal": ordinal, "search_terms": " ".join(terms), "locator": encoded(locator),
                         "state": group["state"], "group": number})
    assert len(rows) == size and len({scoped_key(row["meta"]) for row in rows}) == size
    return {"size": size, "eligible": eligible, "groups": groups, "rows": rows}


def bucket_rows(data, bucket):
    group = BUCKETS.index(bucket) if bucket != "dense" else None
    return [row for row in data["rows"] if row["state"] == "eligible" and (group is None or row["group"] == group)]


def bucket_source(data, bucket):
    return None if bucket == "dense" else data["groups"][BUCKETS.index(bucket)]["source"]


def visibility_sql(start_slot=1, source=None):
    slots = iter([f"?{start_slot}", f"?{start_slot + 1}"])
    sql = re.sub(r"\?", lambda _: next(slots), CURRENT_SQL) + " AND " + SUPPORT_SQL
    params = [T, W]
    if source is not None:
        sql += f" AND v.source_event_id=?{start_slot + 2}"
        params.append(source)
    return sql, params


def candidate_sql(data, bucket, k):
    assert k in KS
    visibility, params = visibility_sql(2, bucket_source(data, bucket))
    joined = visibility.replace("FROM oc_chunks c", "FROM oc_chunks c JOIN scored s ON s.tenant_id=c.tenant_id AND s.workspace_id=c.workspace_id AND s.id=c.id", 1)
    sql = (f"WITH scored AS MATERIALIZED ({SCORE_SQL}) SELECT {META_COLUMNS},s.score "
           f"{joined} ORDER BY s.score DESC,c.tenant_id,c.workspace_id,c.id LIMIT {k}")
    return sql, params


def planned_cases():
    return [{"case": f"n{n}-{query}-{bucket}-k{k}-{mode}", "corpus_rows": n,
             "query": query, "bucket": bucket, "k": k, "mode": mode,
             "status": "NOT_RUN", "reason": "not executed", "trials": []}
            for n in CORPORA for query in QUERIES for bucket in BUCKETS for k in KS
            for mode in POLICY["modes"] if mode == "unbudgeted" or (bucket == "dense" and k == 100)]


def populate(db, data, fts=True):
    assert compat.initialize(db) == 47
    scopes = {(group["tenant"]["blob"], group["workspace"]["blob"]) for group in data["groups"]}
    db.execute("BEGIN IMMEDIATE")
    try:
        for tenant, workspace in sorted(scopes):
            db.execute("INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES(?,?,?,1)", [{"blob": tenant}, {"blob": workspace}, "synthetic-cost"])
        db.execute("INSERT INTO oc_api_keys(id,tenant_id,workspace_id,token_hash,role,created_at) VALUES(?,?,?,?,?,1)", [KEY, T, W, TOKEN_HASH, "reader"])
        for group in data["groups"]:
            scope = [group["tenant"], group["workspace"]]
            state = group["state"]
            db.execute("INSERT INTO oc_events(tenant_id,workspace_id,id,content,kind,state,created_by,created_at) VALUES(?,?,?,'synthetic','text',?,?,1)", scope + [group["source"], "retracted" if state == "withdrawn_root" else "active", PRINCIPAL])
            if state == "withdrawn_support":
                db.execute("INSERT INTO oc_events(tenant_id,workspace_id,id,content,kind,state,created_by,created_at) VALUES(?,?,?,'synthetic','text','retracted',?,1)", scope + [group["support"], PRINCIPAL])
            current = None if state == "unpublished" else (2 if state == "stale_version" else 1)
            db.execute("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,current_version,deleted,created_at) VALUES(?,?,?,'knowledge','synthetic',?,?,1)", scope + [group["asset"], current, int(state == "deleted_asset")])
            for version in ([1, 2] if state == "stale_version" else [1]):
                db.execute("INSERT INTO oc_versions(tenant_id,workspace_id,asset_id,version,content,content_hash,source_event_id,title,created_by,created_at) VALUES(?,?,?,?,'synthetic','synthetic',?,'synthetic',?,1)", scope + [group["asset"], version, group["source"], PRINCIPAL])
        for start in range(0, len(data["rows"]), 100):
            params = []
            batch = data["rows"][start:start + 100]
            for row in batch:
                meta = row["meta"]
                params.extend(meta[:5] + [row["ordinal"], "synthetic", row["locator"], row["search_terms"]])
            db.execute(f"INSERT INTO oc_chunks({compat.CHUNK_COLUMNS}) VALUES " + ",".join(["(?,?,?,?,?,?,?,?,?)"] * len(batch)), params)
        db.execute("COMMIT")
    except Exception:
        db.raw("ROLLBACK", op="execute")
        raise
    if fts:
        db.execute("CREATE INDEX t0_cost_chunks_fts ON oc_chunks USING fts(search_terms) WITH (tokenizer = 'whitespace')")


class OverallDeadline:
    """Interrupt startup as well as query waits in the Linux native runner.

    CI additionally wraps the entire process group in timeout --signal=KILL.
    A timeout is FAIL/NOT_RUN, never a successful budget rejection.
    """
    def __init__(self, seconds, active):
        self.seconds, self.active = seconds, active
        self.previous = None

    def __enter__(self):
        if not hasattr(signal, "setitimer") or not hasattr(signal, "SIGALRM"):
            raise RuntimeError("native cost execution requires Unix SIGALRM deadline support")
        self.previous = signal.getsignal(signal.SIGALRM)
        signal.signal(signal.SIGALRM, self.expire)
        signal.setitimer(signal.ITIMER_REAL, self.seconds)
        return self

    def expire(self, _signal, _frame):
        if self.active[0] is not None:
            self.active[0].kill()
        raise TimeoutError("overall 900-second driver deadline exceeded")

    def __exit__(self, *_exception):
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, self.previous)


def require_time(deadline):
    if time.monotonic() >= deadline:
        raise TimeoutError("overall 900-second driver deadline exceeded")


class Meter:
    """Counts every native invocation; outer wall time includes Python work."""
    def __init__(self, db, deadline):
        self.db, self.deadline, self.calls = db, deadline, []

    def call(self, label, sql, params=(), execute=False):
        require_time(self.deadline)
        request = {"sql": sql, "params": list(params), "op": "execute" if execute else "query", "connection": "default", "timing": True}
        request_bytes = len((json.dumps(request, ensure_ascii=False) + "\n").encode())
        assert request_bytes < POLICY["max_request_bytes"], "request exceeds frozen bridge input budget"
        start = time.perf_counter()
        call = {"label": label, "op": "execute" if execute else "query", "request_bytes": request_bytes}
        self.calls.append(call)
        try:
            response = self.db.raw(sql, params, op=call["op"], timing=True)
            call["wall_ms"] = (time.perf_counter() - start) * 1000
            call["engine_elapsed_ms"] = response["engine_elapsed_ms"]
            assert math.isfinite(call["engine_elapsed_ms"]) and call["engine_elapsed_ms"] >= 0, "missing/invalid bridge request timing"
            assert response.get("ok"), response
            rows = response["result"].get("rows", [])
            call.update({"ok": True, "rows": len(rows), "response_json_bytes": len(encoded(response).encode())})
            return rows
        except Exception as error:
            call.update({"ok": False, "wall_ms": (time.perf_counter() - start) * 1000, "error": str(error)})
            raise


def require_authority(meter, label):
    rows = meter.call(label, AUTH_SQL, [TOKEN_HASH])
    assert rows == [[T, W, KEY, "reader"]], "synthetic key not authorized for exact target scope"


def measured(db, data, case, arm, deadline):
    meter = Meter(db, deadline)
    started = time.perf_counter()
    result = {"arm": arm, "outcome": "FAIL", "calls": meter.calls, "hits": [], "final_verified_metadata": []}
    transaction = False
    try:
        meter.call("begin_search", "BEGIN", execute=True)
        transaction = True
        require_authority(meter, "authority")
        query = compat.literal_fts_query(QUERIES[case["query"]])
        if arm == "candidate" and case["mode"] == "guard_512":
            guarded = meter.call("guard", GUARD_SQL, [query])
            result["guard_rows"] = len(guarded)
            if len(guarded) > BUDGET:
                meter.call("commit_search", "COMMIT", execute=True)
                transaction = False
                result.update({"outcome": "RESOURCE_EXHAUSTED", "rank_status": "NOT_RUN", "recall": None})
                return result
        if arm == "candidate":
            sql, params = candidate_sql(data, case["bucket"], case["k"])
            hits = meter.call("candidate_cte", sql, [query] + params)
        else:
            visibility, params = visibility_sql(source=bucket_source(data, case["bucket"]))
            eligible_rows = meter.call("eligibility", "SELECT " + META_COLUMNS + " " + visibility, params)
            eligible = {scoped_key(row): row for row in eligible_rows}
            assert len(eligible) == len(eligible_rows), "duplicate authority rows"
            result["enumerated_eligible_rows"] = len(eligible_rows)
            scores = meter.call("direct_full_index_scores", SCORE_SQL, [query])
            result["full_index_score_rows"] = len(scores)
            selection_start = time.perf_counter()
            hits = [eligible[scoped_key(row)] + [row[3]] for row in scores if scoped_key(row) in eligible]
            hits.sort(key=lambda row: (-row[6], scoped_key(row)))
            hits = hits[:case["k"]]
            result["python_selection_ms"] = (time.perf_counter() - selection_start) * 1000
        result["hits"] = hits
        meter.call("commit_search", "COMMIT", execute=True)
        transaction = False
        # A new transaction refreshes authority/visibility after native I/O.
        meter.call("begin_verify", "BEGIN", execute=True)
        transaction = True
        require_authority(meter, "fresh_authority")
        visibility, params = visibility_sql(source=bucket_source(data, case["bucket"]))
        first = len(params) + 1
        predicate = "0" if not hits else "c.id IN (" + ",".join(f"?{first + i}" for i in range(len(hits))) + ")"
        verified = meter.call("fresh_visibility", "SELECT " + META_COLUMNS + " " + visibility + " AND " + predicate, params + [row[2] for row in hits])
        result["final_verified_metadata"] = verified
        assert sorted(metadata_key(row) for row in verified) == sorted(metadata_key(row) for row in hits), "fresh authority/visibility mismatch"
        assert len({scoped_key(row) for row in hits}) == len(hits), "duplicate native result"
        meter.call("commit_verify", "COMMIT", execute=True)
        transaction = False
        result.update({"outcome": "OK", "rank_status": "PENDING_AUDIT", "recall": None})
    except Exception as error:
        result["error"] = str(error)
        if transaction:
            try:
                meter.call("rollback_after_failure", "ROLLBACK", execute=True)
            except Exception as rollback_error:
                result["rollback_error"] = str(rollback_error)
    finally:
        result["wall_ms"] = (time.perf_counter() - started) * 1000
        result["native_calls"] = len(meter.calls)
        result["native_queries"] = sum(call["op"] == "query" for call in meter.calls)
        result["native_executes"] = sum(call["op"] == "execute" for call in meter.calls)
        result["returned_sql_rows"] = sum(call.get("rows", 0) for call in meter.calls)
        result["native_call_wall_sum_ms"] = sum(call["wall_ms"] for call in meter.calls)
    return result


def collect_audit(db, data, query_name, deadline, result=None):
    if result is None:
        result = {}
    meter = Meter(db, deadline)
    start = time.perf_counter()
    result.update({"corpus_rows": data["size"], "query": query_name, "calls": meter.calls,
                   "position": "after all measured pairs in this phase"})
    query = compat.literal_fts_query(QUERIES[query_name])
    scores = meter.call("audit_direct_scores", SCORE_SQL, [query])
    scores.sort(key=scoped_key)
    result.update({"scores": scores, "matching_rows": len(scores), "actual_match_density": len(scores) / data["size"]})
    expected_match = {scoped_key(row["meta"]) for row in data["rows"] if QUERIES[query_name] in row["search_terms"].split()}
    assert {scoped_key(row) for row in scores} == expected_match and len(scores) == len(expected_match), "direct index match set differs from fixture terms"
    assert all(isinstance(row[3], (int, float)) and math.isfinite(row[3]) and row[3] > 0 for row in scores), "native BM25 score missing/nonpositive/nonfinite"
    eligible = result["eligible_metadata"] = {}
    plans = result["plans"] = {}
    plans.update({"direct": meter.call("plan_direct", "EXPLAIN QUERY PLAN " + SCORE_SQL, [query]),
             "guard": meter.call("plan_guard", "EXPLAIN QUERY PLAN " + GUARD_SQL, [query])})
    for bucket in BUCKETS:
        visibility, params = visibility_sql(source=bucket_source(data, bucket))
        rows = meter.call("audit_eligibility_" + bucket, "SELECT " + META_COLUMNS + " " + visibility, params)
        eligible[bucket] = sorted(rows, key=scoped_key)
        assert sorted(map(metadata_key, rows)) == sorted(metadata_key(row["meta"]) for row in bucket_rows(data, bucket)), "actual visibility differs from independent fixture authority"
        for k in KS:
            sql, params = candidate_sql(data, bucket, k)
            plans[f"{bucket}-k{k}"] = meter.call("plan_" + bucket + f"_k{k}", "EXPLAIN QUERY PLAN " + sql, [query] + params)
    indexes = meter.call("audit_physical_index", "SELECT name,sql FROM sqlite_master WHERE name='t0_cost_chunks_fts'")
    result["physical_index"] = indexes
    assert len(indexes) == 1 and re.search(r"\bUSING\s+fts\b", indexes[0][1], re.I), indexes
    assert all("QUERY INDEX METHOD fts" in encoded(plan) for plan in plans.values()), plans
    result["wall_ms"] = (time.perf_counter() - start) * 1000
    return result


def reference_hits(data, audit):
    scores = {scoped_key(row): row[3] for row in audit["scores"]}
    result = {}
    for bucket in BUCKETS:
        hits = [row["meta"] + [scores[scoped_key(row["meta"])]] for row in bucket_rows(data, bucket)
                if scoped_key(row["meta"]) in scores]
        hits.sort(key=lambda row: (-row[6], scoped_key(row)))
        for k in KS:
            result[(bucket, k)] = hits[:k]
    return result


def oracle_hits(data, audit, bucket, k):
    return reference_hits(data, audit)[(bucket, k)]


def assert_hits(actual, expected):
    assert len(actual) == len(expected), "result count differs"
    for got, want in zip(actual, expected):
        assert metadata_key(got) == metadata_key(want), "rank/scoped identity/asset/version/source differs"
        assert isinstance(got[6], (int, float)) and math.isfinite(got[6]) and got[6] > 0, "invalid native BM25 score"
        assert abs(got[6] - want[6]) <= SCORE_TOLERANCE, "native BM25 score differs"


def expected_labels(arm, rejected=False, guarded=False):
    labels = ["begin_search", "authority"]
    if arm == "candidate" and guarded:
        labels.append("guard")
    if rejected:
        return labels + ["commit_search"]
    labels += ["candidate_cte"] if arm == "candidate" else ["eligibility", "direct_full_index_scores"]
    return labels + ["commit_search", "begin_verify", "fresh_authority", "fresh_visibility", "commit_verify"]


def check_measurement(run, case, audit, data, expected=None):
    if expected is None:
        expected = oracle_hits(data, audit, case["bucket"], case["k"])
    rejected = run["arm"] == "candidate" and case["mode"] == "guard_512" and audit["matching_rows"] > BUDGET
    guarded = run["arm"] == "candidate" and case["mode"] == "guard_512"
    assert run["outcome"] == ("RESOURCE_EXHAUSTED" if rejected else "OK"), "incorrect outcome or execution failure"
    calls = run["calls"]
    assert [call["label"] for call in calls] == expected_labels(run["arm"], rejected, guarded), "missing/extra/reordered measured native call"
    assert all(call.get("ok") for call in calls), "failed native call"
    assert run["native_calls"] == len(calls)
    assert run["native_queries"] == sum(call["op"] == "query" for call in calls)
    assert run["native_executes"] == sum(call["op"] == "execute" for call in calls)
    assert run["returned_sql_rows"] == sum(call["rows"] for call in calls)
    assert abs(run["native_call_wall_sum_ms"] - sum(call["wall_ms"] for call in calls)) < 1e-6
    assert math.isfinite(run["wall_ms"]) and run["wall_ms"] >= run["native_call_wall_sum_ms"] >= 0
    for call in calls:
        assert math.isfinite(call["wall_ms"]) and call["wall_ms"] >= 0
        assert 0 < call["request_bytes"] < POLICY["max_request_bytes"]
        assert 0 <= call["rows"] <= POLICY["max_bridge_rows"]
        assert math.isfinite(call["engine_elapsed_ms"]) and call["engine_elapsed_ms"] >= 0
        assert call["op"] == ("execute" if call["label"].startswith(("begin_", "commit_")) else "query"), "call type differs"
    if guarded:
        assert run["guard_rows"] == min(audit["matching_rows"], BUDGET + 1)
        assert next(call for call in calls if call["label"] == "guard")["rows"] == run["guard_rows"]
    if rejected:
        assert not run["hits"] and not run["final_verified_metadata"]
        assert run["rank_status"] == "NOT_RUN" and run["recall"] is None, "budget rejection mislabeled as successful ranking/empty recall"
    else:
        assert_hits(run["hits"], expected)
        assert sorted(map(metadata_key, run["final_verified_metadata"])) == sorted(map(metadata_key, expected)), "fresh verification omitted or invalid"
        assert next(call for call in calls if call["label"] == "fresh_visibility")["rows"] == len(expected)
        assert run["rank_status"] == "PASS" and run["recall"] == (1.0 if expected else None)
        if run["arm"] == "exact":
            assert run["enumerated_eligible_rows"] == len(bucket_rows(data, case["bucket"]))
            assert run["full_index_score_rows"] == audit["matching_rows"]
            assert next(call for call in calls if call["label"] == "direct_full_index_scores")["rows"] == audit["matching_rows"]
        else:
            assert next(call for call in calls if call["label"] == "candidate_cte")["rows"] == len(expected)


def adjudicate_case(case, audit, data, expected=None):
    errors = []
    if expected is None:
        expected = oracle_hits(data, audit, case["bucket"], case["k"])
    if len(case["trials"]) != POLICY["rounds"]:
        errors.append("missing paired trial")
    for i, trial in enumerate(case["trials"]):
        try:
            assert trial["order"] == POLICY["orders"][i]
            assert [run["arm"] for run in trial["runs"]] == trial["order"]
            for run in trial["runs"]:
                if run["outcome"] == "OK":
                    # Only audit comparison can set PASS; the verifier recomputes
                    # it independently from stored raw hits and reference rows.
                    assert_hits(run["hits"], expected)
                    run["rank_status"] = "PASS"
                    run["recall"] = 1.0 if run["hits"] else None
                check_measurement(run, case, audit, data, expected)
        except (AssertionError, KeyError, TypeError, ValueError) as error:
            errors.append(str(error))
    if errors:
        case.update({"status": "FAIL", "errors": errors})
    else:
        allowed = {scoped_key(item["meta"]) for item in bucket_rows(data, case["bucket"])}
        rejected = case["mode"] == "guard_512" and audit["matching_rows"] > BUDGET
        case.update({"status": "RESOURCE_EXHAUSTED" if rejected else "PASS",
                     "ranking": "NOT_RUN" if rejected else "PASS", "invalid_rows": 0,
                     "eligible_rows": len(bucket_rows(data, case["bucket"])),
                     "matching_rows": audit["matching_rows"], "eligible_matching_rows": sum(scoped_key(row) in allowed for row in audit["scores"])})
    case.pop("reason", None)


def report_errors(report):
    """Fail closed on omissions, false success, altered rank/scores or call counts."""
    errors = ["fatal: " + str(error) for error in report.get("fatal_errors", [])]
    fixtures = {n: fixture(n) for n in CORPORA}
    references = {}
    try:
        assert report["policy"] == POLICY, "policy changed"
        assert report["engine"] == "turso=0.8.2" and report["production_backend_changed"] is False
        assert math.isfinite(report["driver_wall_seconds"]) and 0 <= report["driver_wall_seconds"] <= DEADLINE_SECONDS, "driver deadline missing/exceeded"
        assert report["provenance"]["resolved_versions"] == {"turso": "0.8.2", "turso_core": "0.8.2", "turso_sdk_kit": "0.8.2"}
        assert re.fullmatch(r"[0-9a-f]{64}", report["provenance"]["binary_sha256"]), "binary hash missing"
        expected_sources = set(provenance()["source_sha256"])
        assert set(report["provenance"]["source_sha256"]) == expected_sources
        assert all(re.fullmatch(r"[0-9a-f]{64}", value) for value in report["provenance"]["source_sha256"].values())
        assert len(report["corpora"]) == 2, "missing corpus setup evidence"
        for corpus, n in zip(report["corpora"], CORPORA):
            assert corpus["status"] == "PASS" and corpus["rows"] == n and corpus["fixture_sha256"] == digest(fixtures[n])
            assert corpus["original_schema_files"] == 6 and corpus["original_schema_statements"] == 47
            assert corpus["eligible_rows"] == n * 9 // 10
        expected_cases = planned_cases()
        assert [row["case"] for row in report["cases"]] == [row["case"] for row in expected_cases], "missing/duplicate/reordered matrix case"
        assert len(report["phases"]) == len(CORPORA) * len(QUERIES), "missing phase"
        phase_map = {(phase["corpus_rows"], phase["query"]): phase for phase in report["phases"]}
        assert len(phase_map) == 6, "duplicate phase"
        for n in CORPORA:
            data = fixtures[n]
            for query in QUERIES:
                phase = phase_map[(n, query)]
                assert phase["status"] == "PASS", "phase did not execute/audit completely"
                setup = phase["connection_setup"]
                assert setup["bridge_requests"] == setup["implicit_foreign_keys_pragma_calls"] == 1
                assert setup["position"] == "before measured pairs; no FTS query"
                assert len(setup["calls"]) == 1 and setup["calls"][0]["label"] == "connection_setup" and setup["calls"][0]["ok"] and setup["calls"][0]["rows"] == 1
                audit = phase["audit"]
                assert 0 <= phase["measured_started_ms"] <= phase["measured_finished_ms"] <= phase["audit_started_ms"] <= phase["audit_finished_ms"]
                assert audit["position"] == "after all measured pairs in this phase"
                assert audit["corpus_rows"] == n and audit["query"] == query
                expected_matches = {scoped_key(row["meta"]) for row in data["rows"] if QUERIES[query] in row["search_terms"].split()}
                assert len(audit["scores"]) == audit["matching_rows"] == len(expected_matches)
                assert {scoped_key(row) for row in audit["scores"]} == expected_matches
                assert all(math.isfinite(row[3]) and row[3] > 0 for row in audit["scores"])
                assert audit["actual_match_density"] == len(expected_matches) / n
                references[(n, query)] = reference_hits(data, audit)
                assert set(audit["plans"]) == {"direct", "guard"} | {f"{bucket}-k{k}" for bucket in BUCKETS for k in KS}
                assert all("QUERY INDEX METHOD fts" in encoded(plan) for plan in audit["plans"].values()), "missing genuine FTS plan"
                assert len(audit["physical_index"]) == 1 and audit["physical_index"][0][0] == "t0_cost_chunks_fts" and re.search(r"\bUSING\s+fts\b", audit["physical_index"][0][1], re.I)
                for bucket in BUCKETS:
                    assert sorted(map(metadata_key, audit["eligible_metadata"][bucket])) == sorted(metadata_key(row["meta"]) for row in bucket_rows(data, bucket))
        for case, template in zip(report["cases"], expected_cases):
            try:
                assert all(case[key] == template[key] for key in ("case", "corpus_rows", "query", "bucket", "k", "mode")), "case configuration mutated"
                audit = phase_map[(case["corpus_rows"], case["query"])]["audit"]
                data = fixtures[case["corpus_rows"]]
                rejected = case["mode"] == "guard_512" and audit["matching_rows"] > BUDGET
                assert case["status"] == ("RESOURCE_EXHAUSTED" if rejected else "PASS"), "failed/unexecuted/mislabeled case"
                assert case["ranking"] == ("NOT_RUN" if rejected else "PASS") and case["invalid_rows"] == 0
                assert case["eligible_rows"] == len(bucket_rows(data, case["bucket"]))
                assert case["matching_rows"] == audit["matching_rows"]
                allowed = {scoped_key(row["meta"]) for row in bucket_rows(data, case["bucket"])}
                assert case["eligible_matching_rows"] == sum(scoped_key(row) in allowed for row in audit["scores"])
                assert len(case["trials"]) == 2
                for i, trial in enumerate(case["trials"]):
                    assert trial["order"] == POLICY["orders"][i]
                    assert [run["arm"] for run in trial["runs"]] == trial["order"]
                    for run in trial["runs"]:
                        check_measurement(run, case, audit, data, references[(case["corpus_rows"], case["query"])][(case["bucket"], case["k"])])
            except (AssertionError, KeyError, TypeError, ValueError) as error:
                errors.append(f"{case.get('case')}: {error}")
        expected_summary = {status: sum(case["status"] == status for case in report["cases"]) for status in ("PASS", "FAIL", "NOT_RUN", "RESOURCE_EXHAUSTED")}
        assert report["summary"] == expected_summary, "summary count mismatch"
        assert expected_summary["RESOURCE_EXHAUSTED"] > 0 and expected_summary["PASS"] > 0
        assert report["completed"] is True and not report.get("fatal_errors"), "run incomplete"
    except (AssertionError, KeyError, TypeError, ValueError) as error:
        errors.append(str(error) or "report invariant failed")
    return errors


def provenance(binary=None):
    paths = [Path(__file__), Path(__file__).with_name("verify.py"),
             Path(__file__).with_name("Cargo.toml"), Path(__file__).with_name("Cargo.lock"),
             Path(__file__).parent / "src/main.rs", ROOT / "src/storage/sqlite/visibility.rs",
             ROOT / "src/storage/sqlite/relational.rs"]
    paths += [ROOT / "src/storage" / name for name in compat.SCHEMAS]
    lock = tomllib.loads(Path(__file__).with_name("Cargo.lock").read_text())
    versions = {package["name"]: package["version"] for package in lock["package"] if package["name"] in {"turso", "turso_core", "turso_sdk_kit"}}
    assert versions == {"turso": "0.8.2", "turso_core": "0.8.2", "turso_sdk_kit": "0.8.2"}, versions
    result = {"baseline": compat.BASELINE, "resolved_versions": versions,
              "source_sha256": {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}}
    if binary is not None:
        result["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        result["build_provenance_required"] = "retain exact-head release --locked CI build log; hash alone does not establish binary source"
    return result


def new_report():
    return {"policy": POLICY, "engine": "turso=0.8.2", "production_backend_changed": False,
            "completed": False, "cases": planned_cases(),
            "phases": [{"corpus_rows": n, "query": query, "status": "NOT_RUN"} for n in CORPORA for query in QUERIES],
            "corpora": [], "fatal_errors": [], "limitations": [
                "Chunk branch only; summary aggregation and cross-scope corpus perturbation NOT_RUN.",
                "Actual relational key/current-root/local-S2 checks do not exercise the public service or full Learning closure.",
                "Read LIMIT513 limits guard output, not engine CPU/memory. Fixed 10k fixture and subprocess deadlines are separate limits.",
                "Global BM25 relevance policy is still unselected. Historical raw-JOIN zero-score failure is retained.",
                "Two paired observations per cell are descriptive; no latency SLO, production speedup or hardware acceptance."]}


def save_report(report, output):
    report["summary"] = {status: sum(case["status"] == status for case in report["cases"]) for status in ("PASS", "FAIL", "NOT_RUN", "RESOURCE_EXHAUSTED")}
    temporary = output / "report.json.tmp"
    temporary.write_text(json.dumps(report, indent=2) + "\n")
    temporary.replace(output / "report.json")


def execute(binary, output):
    output.mkdir(parents=True, exist_ok=False)
    report = new_report()
    report["provenance"] = provenance(binary)
    save_report(report, output)
    start = time.monotonic()
    deadline = start + DEADLINE_SECONDS
    active = [None]
    guard = OverallDeadline(DEADLINE_SECONDS, active)
    guard.__enter__()
    try:
        with tempfile.TemporaryDirectory(prefix="origence-t0-fts-cost-") as temporary:
            base = Path(temporary)
            try:
                for n in CORPORA:
                    require_time(deadline)
                    data = fixture(n)
                    directory = base / str(n)
                    directory.mkdir()
                    path = directory / "context.db"
                    setup_start = time.perf_counter()
                    active[0] = compat.Bridge(binary, path)
                    require_time(deadline)
                    try:
                        populate(active[0], data)
                        require_time(deadline)
                        active[0].close()
                        active[0] = None
                        report["corpora"].append({"rows": n, "fixture_sha256": digest(data), "eligible_rows": data["eligible"],
                                                  "setup_ms": (time.perf_counter() - setup_start) * 1000,
                                                  "original_schema_statements": 47, "original_schema_files": 6,
                                                  "status": "PASS", "invalid_state_counts": {state: sum(row["state"] == state for row in data["rows"]) for state in sorted({row["state"] for row in data["rows"]})}})
                    except Exception as error:
                        report["corpora"].append({"rows": n, "status": "FAIL", "error": str(error)})
                        raise
                    for query_name in QUERIES:
                        require_time(deadline)
                        phase = next(row for row in report["phases"] if row["corpus_rows"] == n and row["query"] == query_name)
                        phase_cases = [case for case in report["cases"] if case["corpus_rows"] == n and case["query"] == query_name]
                        try:
                            active[0] = compat.Bridge(binary, path, create=False)
                            require_time(deadline)
                            connection_meter = Meter(active[0], deadline)
                            assert connection_meter.call("connection_setup", "SELECT 1") == [[1]]
                            phase["connection_setup"] = {"calls": connection_meter.calls, "bridge_requests": 1, "implicit_foreign_keys_pragma_calls": 1, "position": "before measured pairs; no FTS query"}
                            phase["measured_started_ms"] = (time.monotonic() - start) * 1000
                            for case in phase_cases:
                                for order in POLICY["orders"]:
                                    trial = {"order": order, "runs": []}
                                    case["trials"].append(trial)
                                    for arm in order:
                                        run = measured(active[0], data, case, arm, deadline)
                                        trial["runs"].append(run)
                                        save_report(report, output)
                                        if run["outcome"] == "FAIL":
                                            case.update({"status": "FAIL", "errors": [run.get("error", "measured path failed")]})
                                            raise RuntimeError(f"{case['case']} {arm}: {run.get('error')}")
                            phase["measured_finished_ms"] = (time.monotonic() - start) * 1000
                            phase["audit_started_ms"] = (time.monotonic() - start) * 1000
                            phase["audit"] = {}
                            collect_audit(active[0], data, query_name, deadline, phase["audit"])
                            phase["audit_finished_ms"] = (time.monotonic() - start) * 1000
                            phase["status"] = "PASS"
                            references = reference_hits(data, phase["audit"])
                            for case in phase_cases:
                                adjudicate_case(case, phase["audit"], data, references[(case["bucket"], case["k"])])
                            active[0].close()
                            active[0] = None
                        except Exception as error:
                            phase.update({"status": "FAIL", "error": str(error)})
                            for case in phase_cases:
                                if case["status"] == "NOT_RUN":
                                    case["reason"] = "phase failed before independent audit/completion; partial measured trials retained"
                            if active[0] is not None:
                                active[0].kill()
                                active[0] = None
                        save_report(report, output)
                        print(encoded({"phase": f"{n}-{query_name}", "status": phase["status"], "summary": report["summary"]}), flush=True)
            finally:
                if active[0] is not None:
                    active[0].kill()
                    active[0] = None
                report["bridge_diagnostics"] = []
                for path in base.rglob("bridge.stderr"):
                    target = output / "bridge-stderr" / path.relative_to(base)
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with path.open("rb") as stream:
                        retained = stream.read(1024 * 1024)
                    target.write_bytes(retained)
                    report["bridge_diagnostics"].append({"path": str(target.relative_to(output)), "source_bytes": path.stat().st_size, "retained_bytes": len(retained)})
    except Exception as error:
        report["fatal_errors"].append(str(error))
    finally:
        guard.__exit__()
        report["driver_wall_seconds"] = time.monotonic() - start
        report["completed"] = not report["fatal_errors"] and all(phase["status"] == "PASS" for phase in report["phases"])
        save_report(report, output)
    errors = report_errors(report)
    (output / "verification.json").write_text(json.dumps({"accepted": not errors, "errors": errors}, indent=2) + "\n")
    print(encoded({"summary": report["summary"], "accepted": not errors, "errors": errors}), flush=True)
    return int(bool(errors))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="existing release-mode pinned Turso bridge")
    parser.add_argument("--output", type=Path, help="new output directory; existing path rejected")
    parser.add_argument("--report", type=Path, help="independently revalidate a saved report, without native execution")
    args = parser.parse_args()
    if args.report:
        assert args.binary is None and args.output is None
        errors = report_errors(json.loads(args.report.read_text()))
        print(encoded({"accepted": not errors, "errors": errors}))
        return int(bool(errors))
    if args.binary is None:
        assert args.output is None, "--output requires --binary"
        print(json.dumps({"policy": POLICY, "configurations": len(planned_cases()), "paired_trials": 2 * len(planned_cases()), "native_status": "NOT_RUN"}, indent=2))
        return 0
    assert args.output is not None, "--binary requires --output"
    return execute(args.binary.resolve(strict=True), args.output)


if __name__ == "__main__":
    raise SystemExit(main())
