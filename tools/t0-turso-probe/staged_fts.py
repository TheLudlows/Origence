#!/usr/bin/env python3
"""Bounded, test-only staged Turso FTS. No production adapter or JOIN tuning.

SQLite tests use fabricated FTS values and are harness evidence only. Native
execution requires the existing pinned release bridge and an outer process-group
900-second SIGKILL cap. Historical probes/reports are imported read-only.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import re
import tempfile
import time

SPEC = importlib.util.spec_from_file_location("t0_frozen_fts", Path(__file__).with_name("fts_cost.py"))
fts = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fts)
PAGE = 256
LIMITS = {"candidates": 10000, "authority_rows": 60000, "native_calls": 512}
SECONDS = 900
ORDERS = [["staged", "full_scan"], ["full_scan", "staged"]]
POLICY = {
    "name": "t0-staged-scoped-fts-global-bm25-v1", "corpora": [1000, 10000],
    "queries": list(fts.QUERIES), "buckets": list(fts.BUCKETS), "ks": [10, 100],
    "orders": ORDERS, "page_size": PAGE, "limits": LIMITS,
    "score_kind": "native_global_index_bm25_higher_is_better",
    "score_tolerance": 1e-9, "tie_order": ["tenant_id", "workspace_id", "chunk_id"],
    "paging": "LIMIT page+1 OFFSET examined, stable total order in one read snapshot",
    "lookahead": "one extra returned row per page; separately counted, never ranked twice",
    "authority": "scoped batch reads: chunks/assets/exact versions/root events/whole-version local S2",
    "fresh_check": "separate snapshot; revoked key AUTHORITY_CHANGED; invalidated metadata SNAPSHOT_CHANGED, retry whole search; both return no evidence",
    "ranking_snapshot": "retrieval snapshot, not latest snapshot; final read revalidates evidence, not ranking",
    "candidate_sql_has_join": False, "authority_sql_has_join": False,
    "query_seconds": 30, "bridge_process_seconds": 180, "driver_seconds": SECONDS,
    "hard_cap": "caller must wrap the whole driver/process group in timeout --signal=KILL 900s",
    "logical_limits_are_hard_engine_memory_limits": False,
    "latency_gate": None, "production_adoption": False,
    "not_run": ["summary_union_and_ranking", "context_route_and_identity_pools",
                "full_service_permissions", "transitive_learning_closure", "vector_generation_ledger",
                "production_integration", "representative_relevance", "100k_1m_provider"],
    "native_focused": ["candidate_budget", "authority_budget", "call_budget", "snapshot_fts_update", "snapshot_insert", "snapshot_delete", "fresh_key_revocation", "fresh_root_revocation"],
    "harness_only_focused": ["adversarial_equal_score_ties", "exact_512_513_candidate_cap", "malformed_and_duplicate_json", "nonmatching_sibling_s2_withdrawal", "empty_and_shortage"],
    "stop": "one frozen matrix; fail fast on correctness/provenance/snapshot failure; no tuning",
}
SCOPED_SCORE = ("SELECT tenant_id,workspace_id,id,fts_score(search_terms,?1) AS score "
                "FROM oc_chunks WHERE fts_match(search_terms,?1) AND tenant_id=?2 AND workspace_id=?3 "
                "ORDER BY score DESC,tenant_id,workspace_id,id")


class CountedBridge(fts.compat.Bridge):
    def __init__(self, *args, **kwargs):
        self.request_count = 0
        self.query_count = 0
        self.execute_count = 0
        self.connection_names = set()
        super().__init__(*args, **kwargs)

    def raw(self, *args, **kwargs):
        self.request_count += 1
        self.connection_names.add(kwargs.get("connection", "default"))
        if kwargs.get("op", "query") == "query":
            self.query_count += 1
        else:
            self.execute_count += 1
        return super().raw(*args, **kwargs)

    def counts(self):
        return {"bridge_requests": self.request_count, "queries": self.query_count,
                "executes": self.execute_count, "implicit_foreign_keys_pragmas": len(self.connection_names),
                "connections": sorted(self.connection_names)}


class ResourceExhausted(Exception):
    pass


class SnapshotChanged(Exception):
    pass


class AuthorityChanged(Exception):
    pass


def key(row):
    return fts.scoped_key(row)


def rank(row):
    return (-row[3], key(row))


def blobkey(value):
    result = value["blob"]
    assert re.fullmatch("[0-9a-f]{32}", result), "invalid scoped UUID"
    return result


def chunks(values, size=PAGE):
    for offset in range(0, len(values), size):
        yield values[offset:offset + size]


def planned_cases():
    return [{"case": f"n{n}-{query}-{bucket}-k{k}", "corpus_rows": n,
             "query": query, "bucket": bucket, "k": k, "status": "NOT_RUN", "trials": []}
            for n in fts.CORPORA for query in fts.QUERIES for bucket in fts.BUCKETS for k in fts.KS]


def page_sql(take):
    assert 1 <= take <= PAGE
    return SCOPED_SCORE + f" LIMIT {take + 1} OFFSET ?4"


class Meter:
    def __init__(self, db, deadline, limits=None, connection="default"):
        self.db, self.deadline, self.connection = db, deadline, connection
        self.limits = dict(LIMITS if limits is None else limits)
        self.calls = []
        self.authority_rows = 0

    def call(self, label, sql, params=(), *, execute=False, authority=False, cleanup=False):
        fts.require_time(self.deadline, SECONDS)
        # Reserve the final request for rollback, even when a logical cap fires.
        if len(self.calls) >= self.limits["native_calls"] - (0 if cleanup else 1):
            raise ResourceExhausted("native_call_limit")
        remaining = self.limits["authority_rows"] - self.authority_rows
        if authority:
            if remaining < 0:
                raise ResourceExhausted("authority_row_limit")
            take = min(remaining, 9999)
            sql += f" LIMIT {take + 1}"
        request = {"sql": sql, "params": list(params), "op": "execute" if execute else "query",
                   "connection": self.connection, "timing": True}
        request_bytes = len((fts.encoded(request) + "\n").encode())
        if request_bytes > 4 * 1024 * 1024:
            raise ResourceExhausted("request_byte_limit")
        call = {"label": label, "op": request["op"], "sql": sql, "params": list(params),
                "connection": self.connection, "request_bytes": request_bytes, "authority": authority}
        self.calls.append(call)
        started = time.perf_counter()
        try:
            response = self.db.raw(sql, params, op=request["op"], connection=self.connection, timing=True)
            call["engine_elapsed_ms"] = response["engine_elapsed_ms"]
            assert math.isfinite(call["engine_elapsed_ms"]) and call["engine_elapsed_ms"] >= 0
            assert response.get("ok"), response
            rows = response["result"].get("rows", [])
            call.update(ok=True, rows=len(rows), changed=response["result"].get("changed"), response_json_bytes=len(fts.encoded(response).encode()))
            if authority:
                self.authority_rows += len(rows)
                if len(rows) > take:
                    raise ResourceExhausted("authority_row_limit")
            return rows
        except Exception as error:
            call.update(ok=False, error=str(error))
            raise
        finally:
            call["wall_ms"] = (time.perf_counter() - started) * 1000

    def authenticate(self, label):
        rows = self.call(label, fts.AUTH_SQL, [fts.TOKEN_HASH], authority=True)
        if rows != [[fts.T, fts.W, fts.KEY, "reader"]]:
            raise AuthorityChanged("key no longer authorizes exact fixture scope")


class JsonObject(list):
    """Retain duplicate object members as SQLite JSON does."""

    def first(self, name, default=None):
        return next((value for key, value in self if key == name), default)


def support_ids(locator):
    """Preserve local SQLite JSON path/json_each behavior, not Learning rules."""
    parsed = json.loads(locator, object_pairs_hook=JsonObject)
    if not isinstance(parsed, JsonObject) or parsed.first("parser") != "memory-semantics:v1":
        return []
    missing = object()
    sources = parsed.first("support_sources", missing)
    if sources is missing:
        return []
    if isinstance(sources, JsonObject):
        sources = [value for _name, value in sources]
    elif not isinstance(sources, list):
        sources = [sources]  # Explicit null iterates one NULL row; missing does not.
    result = []
    for source in sources:
        normalized = source.replace("-", "").lower() if isinstance(source, str) else "invalid"
        result.append(normalized if re.fullmatch("[0-9a-f]{32}", normalized) else None)
    return result


class Authority:
    """Scoped maps live for exactly one snapshot; table reads are bulk, not N+1."""
    def __init__(self, meter):
        self.meter = meter
        self.chunk_rows, self.assets, self.versions, self.events, self.supports = {}, {}, {}, {}, {}
        self.loaded_chunks, self.loaded_assets, self.loaded_versions, self.loaded_events = set(), set(), set(), set()

    def ids(self, label, table, columns, column, values):
        result = []
        for batch in chunks(sorted(set(values))):
            sql = (f"SELECT {columns} FROM {table} WHERE tenant_id=? AND workspace_id=? AND "
                   f"{column} IN (" + ",".join("?" for _ in batch) + ")")
            result.extend(self.meter.call(label, sql, [fts.T, fts.W] + [{"blob": v} for v in batch], authority=True))
        return result

    def pairs(self, label, table, columns, pairs, suffix=""):
        result = []
        for batch in chunks(sorted(set(pairs))):
            sql = (f"SELECT {columns} FROM {table} WHERE tenant_id=? AND workspace_id=? AND (" +
                   " OR ".join("(asset_id=? AND version=?)" for _ in batch) + ")" + suffix)
            params = [fts.T, fts.W]
            for asset, version in batch:
                params.extend([{"blob": asset}, version])
            result.extend(self.meter.call(label, sql, params, authority=True))
        return result

    def eligible(self, candidates, source=None):
        assert all(key(row)[:2] == (blobkey(fts.T), blobkey(fts.W)) for row in candidates), "foreign candidate"
        ids = {blobkey(row[2]) for row in candidates}
        missing = ids - self.loaded_chunks
        for row in self.ids("chunks", "oc_chunks", "id,asset_id,version,locator", "id", missing):
            self.chunk_rows[blobkey(row[0])] = row
        self.loaded_chunks.update(missing)
        asset_ids = {blobkey(self.chunk_rows[i][1]) for i in ids if i in self.chunk_rows}
        missing_assets = asset_ids - self.loaded_assets
        for row in self.ids("assets", "oc_assets", "id,kind,current_version,deleted", "id", missing_assets):
            self.assets[blobkey(row[0])] = row
        self.loaded_assets.update(missing_assets)
        pairs = {(blobkey(self.chunk_rows[i][1]), self.chunk_rows[i][2]) for i in ids if i in self.chunk_rows}
        missing_pairs = pairs - self.loaded_versions
        for row in self.pairs("versions", "oc_versions", "asset_id,version,source_event_id", missing_pairs):
            self.versions[(blobkey(row[0]), row[1])] = row
        for pair in missing_pairs:
            self.supports[pair] = []
        # Whole-version support matters: an unreturned sibling can invalidate it.
        for row in self.pairs("s2_supports", "oc_chunks", "asset_id,version,locator", missing_pairs,
                              " AND json_extract(locator,'$.parser')='memory-semantics:v1'"):
            self.supports[(blobkey(row[0]), row[1])].extend(support_ids(row[2]))
        self.loaded_versions.update(missing_pairs)
        event_ids = {blobkey(self.versions[p][2]) for p in pairs if p in self.versions}
        event_ids.update(s for p in pairs for s in self.supports.get(p, []) if s is not None)
        missing_events = event_ids - self.loaded_events
        for row in self.ids("events", "oc_events", "id,state,kind", "id", missing_events):
            self.events[blobkey(row[0])] = row
        self.loaded_events.update(missing_events)
        result = []
        for candidate in candidates:
            chunk = self.chunk_rows.get(blobkey(candidate[2]))
            if chunk is None:
                continue
            pair = (blobkey(chunk[1]), chunk[2])
            asset, version = self.assets.get(pair[0]), self.versions.get(pair)
            if asset is None or version is None or asset[3] or asset[2] != chunk[2]:
                continue
            root = self.events.get(blobkey(version[2]))
            if root is None or root[1] != "active":
                continue
            if root[2] == "learning":
                raise RuntimeError("transitive Learning authority is outside this raw-chunk fixture")
            if source is not None and version[2] != source:
                continue
            if any(s is None or s not in self.events or self.events[s][1] != "active" for s in self.supports[pair]):
                continue
            result.append(candidate[:3] + [chunk[1], chunk[2], version[2], candidate[3]])
        return result


def validate_page(rows, previous=None):
    assert len({key(row) for row in rows}) == len(rows), "duplicate scoped candidate"
    for row in rows:
        assert len(row) == 4 and isinstance(row[3], (int, float))
        assert math.isfinite(row[3]) and row[3] > 0, "invalid native BM25 score"
        assert key(row)[:2] == (blobkey(fts.T), blobkey(fts.W)), "scope preconstraint failed"
    assert rows == sorted(rows, key=rank), "native page total order failed"
    if previous is not None and rows:
        assert rank(previous) < rank(rows[0]), "paging repeated/skipped ordering boundary"


def measured(db, data, case, arm, deadline, *, limits=None, page_size=PAGE, hooks=None):
    assert arm in ("staged", "full_scan") and 1 <= page_size <= PAGE and 1 <= case["k"] <= 100
    meter = Meter(db, deadline, limits)
    result = {"arm": arm, "outcome": "FAIL", "hits": [], "calls": meter.calls,
              "rank_status": "NOT_RUN", "recall": None, "pages": [], "examined": 0,
              "requested_k": case["k"], "page_size": page_size,
              "lookahead_rows": 0, "final_verified_metadata": []}
    began, started = False, time.perf_counter()
    hooks = hooks or {}
    try:
        meter.call("begin_search", "BEGIN", execute=True)
        began = True
        meter.authenticate("authority")  # Also establishes the read snapshot.
        authority = Authority(meter)
        query = fts.compat.literal_fts_query(fts.QUERIES[case["query"]])
        source = fts.bucket_source(data, case["bucket"])
        all_scores = None
        if arm == "full_scan":
            all_scores = meter.call("full_index_scores", fts.SCORE_SQL, [query])
            result["full_index_rows"] = len(all_scores)
            assert len({key(r) for r in all_scores}) == len(all_scores), "duplicate full-index rows"
            all_scores = sorted([r for r in all_scores if key(r)[:2] == (blobkey(fts.T), blobkey(fts.W))], key=rank)
        hits, seen, previous, offset = [], set(), None, 0
        while True:
            remaining = meter.limits["candidates"] - offset
            if remaining <= 0:
                raise ResourceExhausted("candidate_limit")
            take = min(page_size, remaining)
            rows = (meter.call("fts_page", page_sql(take), [query, fts.T, fts.W, offset]) if all_scores is None
                    else all_scores[offset:offset + take + 1])
            result["pending_page"] = {"offset": offset, "take": take, "rows": rows}
            validate_page(rows, previous)
            page, more = rows[:take], len(rows) > take
            assert not (set(map(key, page)) & seen), "candidate repeated across pages"
            seen.update(map(key, page))
            result["lookahead_rows"] += int(more)
            result["pages"].append({"offset": offset, "take": take, "rows": len(rows),
                                    "more": more, "first": page[0] if page else None,
                                    "last": page[-1] if page else None})
            result.pop("pending_page")
            hits.extend(authority.eligible(page, source))
            offset += len(page)
            result["examined"] = offset
            if page:
                previous = page[-1]
            if "after_page" in hooks:
                hooks["after_page"](db, len(result["pages"]), result)
            if len(hits) >= case["k"] or not more:
                break
        hits = hits[:case["k"]]
        assert len({key(row) for row in hits}) == len(hits)
        meter.call("commit_search", "COMMIT", execute=True)
        began = False
        if "before_verify" in hooks:
            hooks["before_verify"](db, result, hits)
        meter.call("begin_verify", "BEGIN", execute=True)
        began = True
        meter.authenticate("fresh_authority")
        fresh = Authority(meter).eligible([row[:3] + [row[6]] for row in hits], source)
        if sorted(map(fts.metadata_key, fresh)) != sorted(map(fts.metadata_key, hits)):
            raise SnapshotChanged("fresh authority changed retained metadata/visibility; retry whole search")
        meter.call("commit_verify", "COMMIT", execute=True)
        began = False
        result.update(outcome="OK", hits=hits, final_verified_metadata=[r[:6] for r in fresh],
                      rank_status="PENDING_AUDIT")
    except (ResourceExhausted, SnapshotChanged, AuthorityChanged) as error:
        outcome = {ResourceExhausted: "RESOURCE_EXHAUSTED", SnapshotChanged: "SNAPSHOT_CHANGED", AuthorityChanged: "AUTHORITY_CHANGED"}[type(error)]
        result.update(outcome=outcome, error=str(error))
    except Exception as error:
        result["error"] = str(error) or type(error).__name__
    finally:
        if began:
            try:
                meter.call("rollback", "ROLLBACK", execute=True, cleanup=True)
            except Exception as error:
                result.update(outcome="FAIL", rollback_error=str(error))
        result.update(wall_ms=(time.perf_counter() - started) * 1000,
                      native_calls=len(meter.calls), native_queries=sum(c["op"] == "query" for c in meter.calls),
                      native_executes=sum(c["op"] == "execute" for c in meter.calls),
                      authority_rows=meter.authority_rows,
                      returned_sql_rows=sum(c.get("rows", 0) for c in meter.calls),
                      native_call_wall_sum_ms=sum(c["wall_ms"] for c in meter.calls))
    return result


def audit(db, data, query_name, deadline, result=None):
    meter = Meter(db, deadline)
    query = fts.compat.literal_fts_query(fts.QUERIES[query_name])
    result = {} if result is None else result
    result.update(calls=meter.calls, position="after all measured pairs")
    scores = meter.call("audit_full_scores", fts.SCORE_SQL, [query])
    result["scores"] = scores
    expected = {key(r["meta"]) for r in data["rows"] if fts.QUERIES[query_name] in r["search_terms"].split()}
    assert len(scores) == len(expected) and {key(r) for r in scores} == expected
    assert all(math.isfinite(r[3]) and r[3] > 0 for r in scores)
    result["scores"] = scores
    # Independent existing SQL is audit-only, never in either measured pipeline.
    result["eligible_metadata"] = {}
    for bucket in fts.BUCKETS:
        sql, params = fts.visibility_sql(source=fts.bucket_source(data, bucket))
        rows = meter.call("audit_legacy_" + bucket, "SELECT " + fts.META_COLUMNS + " " + sql, params)
        result["eligible_metadata"][bucket] = rows
        assert sorted(map(fts.metadata_key, rows)) == sorted(fts.metadata_key(r["meta"]) for r in fts.bucket_rows(data, bucket))
    result["physical_index"] = meter.call("audit_index", "SELECT name,sql FROM sqlite_master WHERE name='t0_cost_chunks_fts'")
    assert len(result["physical_index"]) == 1 and "USING fts" in result["physical_index"][0][1]
    result["plans"] = {}
    result["plans"]["scoped_page"] = meter.call("plan_scoped_page", "EXPLAIN QUERY PLAN " + page_sql(PAGE), [query, fts.T, fts.W, 0])
    result["plans"]["direct"] = meter.call("plan_direct", "EXPLAIN QUERY PLAN " + fts.SCORE_SQL, [query])
    assert all("QUERY INDEX METHOD fts" in fts.encoded(p) for p in result["plans"].values()), "native index plan missing"
    return result


def check_run(run, expected, *, allow_rejection=False):
    assert run["native_calls"] == len(run["calls"]) <= LIMITS["native_calls"]
    assert run["native_queries"] == sum(c["op"] == "query" for c in run["calls"])
    assert run["native_executes"] == sum(c["op"] == "execute" for c in run["calls"])
    assert run["returned_sql_rows"] == sum(c.get("rows", 0) for c in run["calls"])
    assert run["authority_rows"] == sum(c.get("rows", 0) for c in run["calls"] if c["authority"])
    assert abs(run["native_call_wall_sum_ms"] - sum(c["wall_ms"] for c in run["calls"])) < 1e-6
    assert math.isfinite(run["wall_ms"]) and run["wall_ms"] >= run["native_call_wall_sum_ms"] >= 0
    assert all(not re.search(r"\bJOIN\b", c["sql"], re.I) for c in run["calls"]), "JOIN in measured path"
    for call in run["calls"]:
        assert 0 < call["request_bytes"] <= 4 * 1024 * 1024
        assert math.isfinite(call["engine_elapsed_ms"]) and call["engine_elapsed_ms"] >= 0
        assert math.isfinite(call["wall_ms"]) and call["wall_ms"] >= 0
    assert run["outcome"] == "OK" or (allow_rejection and run["outcome"] == "RESOURCE_EXHAUSTED"), run.get("error", run["outcome"])
    if run["outcome"] != "OK":
        assert not run["hits"] and not run["final_verified_metadata"]
        assert run["rank_status"] == "NOT_RUN" and run["recall"] is None
        return
    assert all(c["ok"] for c in run["calls"])
    labels = [c["label"] for c in run["calls"]]
    assert labels[:2] == ["begin_search", "authority"]
    assert "commit_search" in labels and "begin_verify" in labels and "fresh_authority" in labels and labels[-1] == "commit_verify"
    assert labels.index("commit_search") < labels.index("begin_verify") < labels.index("fresh_authority")
    assert 1 <= run["requested_k"] <= 100 and 1 <= run["page_size"] <= PAGE
    assert run["pages"], "missing paging proof"
    offset = 0
    for page in run["pages"]:
        assert page["offset"] == offset and 1 <= page["take"] <= run["page_size"]
        assert 0 <= page["rows"] <= page["take"] + 1
        assert page["more"] == (page["rows"] > page["take"])
        consumed = min(page["rows"], page["take"])
        assert (page["first"] is None) == (consumed == 0)
        assert (page["last"] is None) == (consumed == 0)
        offset += consumed
    assert run["examined"] == offset
    assert run["lookahead_rows"] == sum(p["more"] for p in run["pages"])
    assert len(run["hits"]) >= run["requested_k"] or not run["pages"][-1]["more"], "neither top-K nor EOF proven"
    fts_calls = [c for c in run["calls"] if c["label"] == "fts_page"]
    if run["arm"] == "staged":
        assert len(fts_calls) == len(run["pages"])
        for call, page in zip(fts_calls, run["pages"]):
            assert call["sql"] == page_sql(page["take"]) and call["rows"] == page["rows"]
            assert call["params"][1:] == [fts.T, fts.W, page["offset"]]
    else:
        assert not fts_calls
    fts.assert_hits(run["hits"], expected)
    assert sorted(map(fts.metadata_key, run["final_verified_metadata"])) == sorted(map(fts.metadata_key, expected))
    assert run["examined"] <= LIMITS["candidates"] and run["authority_rows"] <= LIMITS["authority_rows"]


def validate_counts(counts, expected=None):
    assert counts["bridge_requests"] == counts["queries"] + counts["executes"]
    assert counts["bridge_requests"] > 0
    if expected is not None:
        assert counts["bridge_requests"] == expected, "unaccounted native bridge request"
    assert counts["implicit_foreign_keys_pragmas"] == len(counts["connections"]) >= 1


def validate_audit(audited, data, query):
    assert audited["position"] == "after all measured pairs"
    expected_keys = {key(r["meta"]) for r in data["rows"] if fts.QUERIES[query] in r["search_terms"].split()}
    scores = audited["scores"]
    assert len(scores) == len(expected_keys) and {key(r) for r in scores} == expected_keys
    assert all(math.isfinite(r[3]) and r[3] > 0 for r in scores)
    assert set(audited["plans"]) == {"scoped_page", "direct"}
    assert all("QUERY INDEX METHOD fts" in fts.encoded(p) for p in audited["plans"].values())
    indexes = audited["physical_index"]
    assert len(indexes) == 1 and indexes[0][0] == "t0_cost_chunks_fts" and "USING fts" in indexes[0][1]
    assert set(audited["eligible_metadata"]) == set(fts.BUCKETS)
    for bucket, rows in audited["eligible_metadata"].items():
        assert sorted(map(fts.metadata_key, rows)) == sorted(fts.metadata_key(r["meta"]) for r in fts.bucket_rows(data, bucket))


def validate_focused(focused):
    expected = {"candidate_budget": "RESOURCE_EXHAUSTED", "authority_budget": "RESOURCE_EXHAUSTED",
                "call_budget": "RESOURCE_EXHAUSTED", "snapshot_fts_update": "OK", "snapshot_insert": "OK", "snapshot_delete": "OK",
                "fresh_key_revocation": "AUTHORITY_CHANGED", "fresh_root_revocation": "SNAPSHOT_CHANGED"}
    assert len(focused) == len(expected) and {r["name"] for r in focused} == set(expected)
    for item in focused:
        assert item["status"] == "PASS"
        run = item["run"]
        assert run["outcome"] == expected[item["name"]]
        assert run["native_calls"] == len(run["calls"]) and run["calls"]
        if run["outcome"] != "OK":
            assert not run["hits"] and not run["final_verified_metadata"]
            assert run["rank_status"] == "NOT_RUN" and run["recall"] is None
            assert run["calls"][-1]["label"] == "rollback" and run["calls"][-1]["ok"]
        if item["name"].endswith("budget"):
            changed = {"candidate_budget": ("candidates", 1), "authority_budget": ("authority_rows", 0), "call_budget": ("native_calls", 3)}[item["name"]]
            assert item["limits"] == {**LIMITS, changed[0]: changed[1]}
        if item["name"].startswith("snapshot_"):
            assert len(item["writer_calls"]) == 1 and item["writer_calls"][0]["ok"] and item["writer_calls"][0]["changed"] == 1
            deleted = item["name"] == "snapshot_delete"
            assert item["old_snapshot_rows"] == ([[[item["target_id"]]]] if deleted else [[]])
            assert item["new_snapshot_rows"] == ([] if deleted else [[item["target_id"]]])
            assert len(run["pages"]) > 1
            data = fts.fixture(1000)
            assert len(item["before_scores"]) == 1000 and {key(r) for r in item["before_scores"]} == {key(r["meta"]) for r in data["rows"]}
            expected_hits = fts.reference_hits(data, {"scores": item["before_scores"]})[("dense", 100)]
            fts.assert_hits(item["before_expected"], expected_hits)
            check_run(run, expected_hits)
        if "auxiliary_calls" in item:
            assert item["auxiliary_calls"] and all(c["ok"] for c in item["auxiliary_calls"])
            assert any(c["connection"] == "writer" and c["changed"] == 1 for c in item["auxiliary_calls"])
        if item["name"].startswith("fresh_"):
            labels = [c["label"] for c in run["calls"]]
            assert labels.index("commit_search") < labels.index("begin_verify") < labels.index("fresh_authority")
            assert item["auxiliary_calls"] and item["auxiliary_calls"][0]["changed"] == 1


def provenance(binary=None):
    result = fts.provenance(binary)
    result["staged_fts_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    return result


def new_report():
    return {"policy": POLICY, "provenance": provenance(), "completed": False, "fatal_errors": [],
            "cases": planned_cases(), "phases": [], "focused": [], "fixtures": [], "not_run": POLICY["not_run"]}


def save(report, output):
    path = output / "report.json.tmp"
    path.write_text(json.dumps(report, indent=2) + "\n")
    path.replace(output / "report.json")


def report_errors(report):
    errors = list(report.get("fatal_errors", []))
    try:
        assert report["policy"] == POLICY and report["completed"]
        assert report["not_run"] == POLICY["not_run"]
        assert 0 <= report["driver_wall_seconds"] <= SECONDS
        assert report["provenance"]["resolved_versions"] == {"turso": "0.8.2", "turso_core": "0.8.2", "turso_sdk_kit": "0.8.2"}
        assert re.fullmatch("[0-9a-f]{64}", report["provenance"]["binary_sha256"])
        assert re.fullmatch("[0-9a-f]{64}", report["provenance"]["staged_fts_sha256"])
        assert len(report["cases"]) == 60 and len(report["phases"]) == 6
        phases = {(p["corpus_rows"], p["query"]): p for p in report["phases"]}
        assert set(phases) == {(n, q) for n in fts.CORPORA for q in fts.QUERIES}
        for phase in phases.values():
            assert phase["status"] == "PASS"
            assert phase["measured_finished_ms"] <= phase["audit_started_ms"]
            validate_audit(phase["audit"], fts.fixture(phase["corpus_rows"]), phase["query"])
        for expected_case, case in zip(planned_cases(), report["cases"]):
            assert all(case[k] == expected_case[k] for k in ("case", "corpus_rows", "query", "bucket", "k"))
            assert case["status"] == "PASS" and len(case["trials"]) == 2
            data = fts.fixture(case["corpus_rows"])
            reference = fts.reference_hits(data, phases[(case["corpus_rows"], case["query"])]["audit"])[(case["bucket"], case["k"])]
            for trial, order in zip(case["trials"], ORDERS):
                assert trial["order"] == order and [r["arm"] for r in trial["runs"]] == order
                for run in trial["runs"]:
                    check_run(run, reference)
                    assert run["requested_k"] == case["k"] and run["page_size"] == PAGE
                    if run["arm"] == "full_scan":
                        full_calls = [c for c in run["calls"] if c["label"] == "full_index_scores"]
                        assert len(full_calls) == 1 and full_calls[0]["sql"] == fts.SCORE_SQL
                        assert run["full_index_rows"] == full_calls[0]["rows"] == len(phases[(case["corpus_rows"], case["query"])]["audit"]["scores"])
                    assert run["rank_status"] == "PASS" and run["recall"] == (1.0 if reference else None)
        validate_focused(report["focused"])
        assert {f["corpus_rows"] for f in report["fixtures"]} == set(fts.CORPORA) and len(report["fixtures"]) == 2
        for fixture in report["fixtures"]:
            assert fixture["fixture_sha256"] == fts.digest(fts.fixture(fixture["corpus_rows"]))
            validate_counts(fixture["counts"])
        for phase in phases.values():
            measured_count = sum(r["native_calls"] for c in report["cases"] if c["corpus_rows"] == phase["corpus_rows"] and c["query"] == phase["query"] for trial in c["trials"] for r in trial["runs"])
            validate_counts(phase["counts"], 1 + measured_count + len(phase["audit"]["calls"]))
        focused_count = sum(f["run"]["native_calls"] + len(f.get("auxiliary_calls", [])) for f in report["focused"])
        validate_counts(report["focused_counts"], focused_count)
    except (AssertionError, KeyError, ValueError, TypeError) as error:
        errors.append(str(error) or "incomplete or invalid staged FTS evidence")
    return errors


def focused_cases(db, data, deadline, results=None):
    """Controlled 1k snapshot mutations; auxiliary/writer calls are counted apart."""
    results = [] if results is None else results
    case = dict(next(c for c in planned_cases() if c["corpus_rows"] == data["size"] and c["query"] == "all" and c["bucket"] == "source_1" and c["k"] == 100))
    for name, limit in [("candidate_budget", {**LIMITS, "candidates": 1}),
                        ("authority_budget", {**LIMITS, "authority_rows": 0}),
                        ("call_budget", {**LIMITS, "native_calls": 3})]:
        item = {"name": name, "status": "NOT_RUN", "limits": limit}
        results.append(item)
        item["run"] = measured(db, data, case, "staged", deadline, limits=limit)
        assert item["run"]["outcome"] == "RESOURCE_EXHAUSTED" and not item["run"]["hits"], item["run"]
        item["status"] = "PASS"
    meter = Meter(db, deadline)
    def aux(label, sql, params=(), *, execute=False, writer=False):
        meter.connection = "writer" if writer else "default"
        return meter.call(label, sql, params, execute=execute)
    snapshot_case = dict(case, bucket="dense")
    target = next(r for r in data["rows"] if r["state"] == "eligible" and r["group"] == 0)
    deleted = next(r for r in data["rows"] if r["state"] == "withdrawn_root")
    new_id = fts.compat.blob(999999)
    new_row = target["meta"][:2] + [new_id] + target["meta"][3:5] + [1, "synthetic", "{}", "commontoken " * 30 + "snapshotinsert"]
    restore_row = deleted["meta"][:5] + [deleted["ordinal"], "synthetic", deleted["locator"], deleted["search_terms"]]
    insert_sql = f"INSERT INTO oc_chunks({fts.compat.CHUNK_COLUMNS}) VALUES(?,?,?,?,?,?,?,?,?)"
    lookup = "SELECT id FROM oc_chunks WHERE fts_match(search_terms,?1) AND tenant_id=?2 AND workspace_id=?3"
    for mutation in ("fts_update", "insert", "delete"):
        start = len(meter.calls)
        item = {"name": "snapshot_" + mutation, "status": "NOT_RUN", "old_snapshot_rows": [], "writer_calls": []}
        results.append(item)
        before_scores = aux("before_full_scores", fts.SCORE_SQL, [fts.compat.literal_fts_query(fts.QUERIES["all"])])
        item["before_scores"] = before_scores
        before_expected = fts.reference_hits(data, {"scores": before_scores})[(snapshot_case["bucket"], snapshot_case["k"])]
        item["before_expected"] = before_expected
        target_id = new_id if mutation == "insert" else (deleted["meta"][2] if mutation == "delete" else target["meta"][2])
        item["target_id"] = target_id
        def write_after_page(_database, number, _result):
            if number != 1:
                return
            if mutation == "fts_update":
                aux("writer_update", "UPDATE oc_chunks SET search_terms=search_terms || ' snapshotnewtoken' WHERE tenant_id=? AND workspace_id=? AND id=?", target["meta"][:3], execute=True, writer=True)
            elif mutation == "insert":
                aux("writer_insert", insert_sql, new_row, execute=True, writer=True)
            else:
                aux("writer_delete", "DELETE FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND id=?", deleted["meta"][:3], execute=True, writer=True)
            item["writer_calls"].append(meter.calls[-1])
            assert meter.calls[-1]["changed"] == 1, "concurrent writer did not change one row"
            if mutation == "delete":
                old = aux("old_snapshot_lookup", lookup + " AND id=?4", ["commontoken", fts.T, fts.W, target_id])
                assert old == [[target_id]], "deleted candidate disappeared inside old snapshot"
            else:
                old = aux("old_snapshot_lookup", lookup, ["snapshotinsert" if mutation == "insert" else "snapshotnewtoken", fts.T, fts.W])
                assert old == [], "new FTS entry appeared inside old snapshot"
            item["old_snapshot_rows"].append(old)
        run = item["run"] = measured(db, data, snapshot_case, "staged", deadline, page_size=8, hooks={"after_page": write_after_page})
        item["auxiliary_calls"] = meter.calls[start:]
        assert run["outcome"] == "OK", run
        fts.assert_hits(run["hits"], before_expected)
        if mutation == "delete":
            fresh = aux("new_snapshot_lookup", lookup + " AND id=?4", ["commontoken", fts.T, fts.W, target_id])
            assert fresh == []
            aux("restore_deleted", insert_sql, restore_row, execute=True)
        elif mutation == "insert":
            fresh = aux("new_snapshot_lookup", lookup, ["snapshotinsert", fts.T, fts.W])
            assert fresh == [[target_id]]
            aux("remove_inserted", "DELETE FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND id=?", [fts.T, fts.W, target_id], execute=True)
        else:
            fresh = aux("new_snapshot_lookup", lookup, ["snapshotnewtoken", fts.T, fts.W])
            assert fresh == [[target_id]]
            aux("restore_updated", "UPDATE oc_chunks SET search_terms=? WHERE tenant_id=? AND workspace_id=? AND id=?", [target["search_terms"]] + target["meta"][:3], execute=True)
        item.update(status="PASS", new_snapshot_rows=fresh, auxiliary_calls=meter.calls[start:])
    dense = dict(case, bucket="dense", k=10)
    for kind in ("key", "root"):
        start = len(meter.calls)
        item = {"name": f"fresh_{kind}_revocation", "status": "NOT_RUN"}
        results.append(item)
        withdrawn = []
        def revoke(_database, _result, hits):
            assert hits
            if kind == "key":
                aux("revoke_key", "UPDATE oc_api_keys SET revoked=1 WHERE id=?", [fts.KEY], execute=True, writer=True)
            else:
                withdrawn.append(hits[0][5])
                aux("revoke_root", "UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?", [fts.T, fts.W, hits[0][5]], execute=True, writer=True)
        run = item["run"] = measured(db, data, dense, "staged", deadline, hooks={"before_verify": revoke})
        item["auxiliary_calls"] = meter.calls[start:]
        assert run["outcome"] == ("AUTHORITY_CHANGED" if kind == "key" else "SNAPSHOT_CHANGED") and not run["hits"], run
        if kind == "key":
            aux("restore_key", "UPDATE oc_api_keys SET revoked=0 WHERE id=?", [fts.KEY], execute=True)
        else:
            aux("restore_root", "UPDATE oc_events SET state='active' WHERE tenant_id=? AND workspace_id=? AND id=?", [fts.T, fts.W, withdrawn[0]], execute=True)
        item.update(status="PASS", auxiliary_calls=meter.calls[start:])
    return results


def execute(binary, output):
    output.mkdir(parents=True, exist_ok=False)
    report = new_report()
    report["provenance"] = provenance(binary)
    started, active = time.monotonic(), [None]
    active_case = active_phase = None
    deadline = started + SECONDS
    guard = fts.OverallDeadline(SECONDS, active)
    save(report, output)
    temporary = tempfile.TemporaryDirectory(prefix="origence-staged-fts-")
    root = Path(temporary.name)
    try:
        with guard:
            for size in fts.CORPORA:
                data = fts.fixture(size)
                directory = root / str(size)
                directory.mkdir()
                path = directory / "context.db"
                active[0] = CountedBridge(binary, path)
                fts.populate(active[0], data)
                report["fixtures"].append({"corpus_rows": size, "fixture_sha256": fts.digest(data), "counts": active[0].counts()})
                active[0].close()
                active[0] = None
                for query in fts.QUERIES:
                    phase = {"corpus_rows": size, "query": query, "status": "NOT_RUN", "fixture_sha256": fts.digest(data)}
                    report["phases"].append(phase)
                    active_phase = phase
                    active[0] = CountedBridge(binary, path, create=False)
                    setup = Meter(active[0], deadline)
                    assert setup.call("connection_setup", "SELECT 1") == [[1]]
                    phase["setup"] = {"calls": setup.calls, "implicit_foreign_keys_pragmas": 1}
                    phase["measured_started_ms"] = (time.monotonic() - started) * 1000
                    cases = [c for c in report["cases"] if c["corpus_rows"] == size and c["query"] == query]
                    for case in cases:
                        active_case = case
                        for order in ORDERS:
                            trial = {"order": order, "runs": []}
                            case["trials"].append(trial)
                            for arm in order:
                                run = measured(active[0], data, case, arm, deadline)
                                trial["runs"].append(run)
                                save(report, output)
                                assert run["outcome"] == "OK", f"{case['case']} {arm}: {run.get('error')}"
                    active_case = None
                    phase["measured_finished_ms"] = (time.monotonic() - started) * 1000
                    phase["audit_started_ms"] = (time.monotonic() - started) * 1000
                    phase["audit"] = {}
                    audit(active[0], data, query, deadline, phase["audit"])
                    phase["audit_finished_ms"] = (time.monotonic() - started) * 1000
                    references = fts.reference_hits(data, phase["audit"])
                    for case in cases:
                        active_case = case
                        expected = references[(case["bucket"], case["k"])]
                        for trial in case["trials"]:
                            for run in trial["runs"]:
                                check_run(run, expected)
                                run.update(rank_status="PASS", recall=1.0 if expected else None)
                        case["status"] = "PASS"
                    active_case = None
                    phase["status"] = "PASS"
                    phase["counts"] = active[0].counts()
                    save(report, output)
                    active[0].close()
                    active[0] = None
                    active_phase = None
                    print(fts.encoded({"phase": f"{size}-{query}", "status": "PASS"}), flush=True)
                if size == 1000:
                    active[0] = CountedBridge(binary, path, create=False)
                    focused_cases(active[0], data, deadline, report["focused"])
                    report["focused_counts"] = active[0].counts()
                    active[0].close()
                    active[0] = None
                    save(report, output)
            report["completed"] = True
    except Exception as error:
        message = str(error) or type(error).__name__
        report["fatal_errors"].append(message)
        if active_case is not None:
            active_case.update(status="FAIL", error=message)
        if active_phase is not None:
            active_phase.update(status="FAIL", error=message)
        if report["focused"] and report["focused"][-1]["status"] == "NOT_RUN":
            report["focused"][-1].update(status="FAIL", error=message)
    finally:
        if active[0] is not None:
            active[0].kill()
        report["bridge_diagnostics"] = []
        for path in root.rglob("bridge.stderr"):
            target = output / "bridge-stderr" / path.relative_to(root)
            target.parent.mkdir(parents=True, exist_ok=True)
            with path.open("rb") as stream:
                retained = stream.read(1024 * 1024)
            target.write_bytes(retained)
            report["bridge_diagnostics"].append({"path": str(target.relative_to(output)), "source_bytes": path.stat().st_size, "retained_bytes": len(retained)})
        temporary.cleanup()
        report["driver_wall_seconds"] = time.monotonic() - started
        save(report, output)
    errors = report_errors(report)
    (output / "verification.json").write_text(json.dumps({"accepted": not errors, "errors": errors}, indent=2) + "\n")
    print(fts.encoded({"accepted": not errors, "errors": errors}), flush=True)
    return int(bool(errors))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if args.report:
        assert args.binary is None and args.output is None
        errors = report_errors(json.loads(args.report.read_text()))
        print(fts.encoded({"accepted": not errors, "errors": errors}))
        return int(bool(errors))
    if args.binary is None:
        assert args.output is None
        print(json.dumps({"policy": POLICY, "configurations": 60, "native_status": "NOT_RUN"}, indent=2))
        return 0
    assert args.output is not None
    return execute(args.binary.resolve(strict=True), args.output)


if __name__ == "__main__":
    raise SystemExit(main())
