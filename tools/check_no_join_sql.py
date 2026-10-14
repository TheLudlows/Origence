#!/usr/bin/env python3
"""Conservative, dependency-free lint of repository-owned Rust/Python SQL inputs.

This is a bounded regression lint, NOT a Rust/Python compiler, security proof,
or complete SQL parser. It checks modeled execution APIs in src/tests/examples/
benches/build.rs and Python tools/tests/examples; generated code and arbitrary
third-party/custom execution APIs require separate review. A PASS is scoped to
recognized inputs, not proof of every possible runtime execution.
Unsupported construction is UNRESOLVED (exit 2), never a successful audit. All
cfg branches are checked. SQL in Markdown/comments is historical evidence, not
an execution input. No SQL or application code is executed by this tool.

Default mode permits only fingerprinted debt from main 1d5e055. It says DEBT,
not JOIN-free. --zero PATH [PATH ...] ignores that debt for the exact paths.
There is deliberately no --bless/update-baseline option. Delete obsolete entries
as their sites are fixed; never add entries to accommodate new application SQL.
"""
from __future__ import annotations

import argparse
import ast
import hashlib
import itertools
import json
import re
import string
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

BASELINE_REVISION = "1d5e0554b38e12df094a894f11a23fb26a117568"
# Exact (relative path | enclosing function | SQL-call ordinal | fingerprint).
# Frozen from the source snapshot above, not from the developer's git HEAD.
BASELINE: frozenset[str] = frozenset({
    # The initial audit reported 57 sites; 20 real conversions preceded its
    # staged 37-entry baseline. A later grammar correction removed 3 false
    # positives: the same single-FROM workspace trigger at 3 schema includes.
    # NEW/OLD fields are scalar row values, not a second scanned relation.
    # Four A-domain queries were genuinely converted separately. Remaining
    # entries may only shrink; this correction adds no SQL exception.
    'src/storage/sqlite/app.rs|cleanup_plan#1|2|5d2127f6ce15ff5b342e1ab7f80a95e9c8957415873a7c4a9148a8080b49ce6f',
    'src/storage/sqlite/app.rs|cleanup_plan#1|3|1b4ac56888cf7b66781327110626b087ae0cce4c1e11c1392e12e1cb848fc98b',
    'src/storage/sqlite/app.rs|cleanup_plan#1|4|fe27b47c9948bb670cf92d838bf5736ca9e8a1a7953356d7831114583db95a1b',
    'src/storage/sqlite/app.rs|finish_cleanup#1|2|981d636eb0d7b19161b2886793b77610fcb713a84030874fd48ae91a2e94222b',
    'src/storage/sqlite/app.rs|finish_cleanup#1|3|ed9b8a81947098f2ff92461ed37cd23f51985d4e1ccbd5a855f7f513e26a6ce7',
    'src/storage/sqlite/app.rs|finish_cleanup#1|4|0d66789ce1cb4092f659c51e680073d1b6783c61d016cd1d8cc6ff43546885c8',
    'src/storage/sqlite/app.rs|finish_cleanup#1|5|eb4b4694e43b6566b325ddd380034ea92ac4539406f866a4383fb84cb230f4a0',
    'src/storage/sqlite/context.rs|context_recent_turns#1|1|e1751d0818e09cf44dd87f7b213e23f673e0f0195d7f9edd2961a30eb321b21e',
    'src/storage/sqlite/context.rs|context_session_turn#1|1|ed8bbdcea378f61a0d1149a659edf1d2abb224d5118d00635335a5d54365f313',
    'src/storage/sqlite/improve.rs|cancel_improve_job#1|2|01c3c787e946ec697a9d79634176aac45b068641f7f6534f754b2731e16256b1',
    'src/storage/sqlite/improve.rs|create_learning#1|2|169f2c1154206421570f6d1d1503f175c2821739d6fa4588c74b9708378d9af4',
    'src/storage/sqlite/improve.rs|mark_learning_published#1|2|f6fd7099d471d0244882305ebab40d584d65141f0c244b0dc7bc23e8fbc5cdac',
    'src/storage/sqlite/improve.rs|require_improve_job_retry_allowed#1|1|dce646566d147e597a6e824997247f23a4c95b980208b9c152353e0134728a80',
    'src/storage/sqlite/job_queue.rs|cancel_affected_jobs_inner#1|1|3efd8dd6087082d832aa31c9c65cb47cf10dc4ba057bbb500f3e934121e2f7af',
    'src/storage/sqlite/ledger.rs|confirm_committed_inner#1|1|d0dd7b0070a300e86738725204b06b6927e435886a157be04cb3c78f4abde341',
    'src/storage/sqlite/publication.rs|message_batch_status#1|1|4c89fe74655d27b9c6f1315eb6cf8cc1800b5f092a9d12e116c8f0ac49fb228d',
    'src/storage/sqlite/publication.rs|message_batch_status#1|4|48b816c6f4eccaa268f69fb47427ef209bdd1eefe1f411c05d4072272338eedd',
    'src/storage/sqlite/session.rs|archive_evidence_local#1|3|42e128ffb4812c510506df64653924676b35c90675644982dc1894aa3446cb24',
    'src/storage/sqlite/session.rs|available#1|1|873183ba7ccd1a2d4c1160230c5eba01ac658d714250837890886702f35eb5c2',
    'src/storage/sqlite/session.rs|require_turn_visible_local#1|1|7aacc553c8eb890f48f0242f2210c97249f7c0a0f6602a24a38d24eada76a6ff',
    'src/storage/sqlite/versions.rs|cleanup_derived_chunks_inner#1|1|8ad3297ffc17ddf20532ea716f797ee9e2ef57fccc522c9bd9a635661b382ca0',
    'src/storage/sqlite/versions.rs|insert_version_inner#1|1|9d507d786e6bd9eb52efbd801b520a7cfd9087bbfb6a08efaba35e169861c714',
    'src/storage/sqlite_graph.rs|snapshot#1|2|8bf6785986170bfc71289e2a2bac5f0596fd6a007ed6da03939302d87b873d26',
    'src/storage/sqlite_graph.rs|traverse#1|1|f509d280a082f0218cbe0ce684f111a78e92577347e87a0640a2b0a1e58123a3',
    'src/storage/sqlite_graph.rs|upsert_relations#1|1|4b8385fc303bb4048a88ec42b5a26c4fb31c5d1d941eb8c4f1a7addc28465685',
    'tests/learning_loop.rs|learning_hidden_candidates_do_not_exhaust_keyword_or_vector_result_budget#1|2|5e20718e801d2774448fb07ab6353ccc78ad98993480fa249c6033597496a670',
    'tests/message_batches.rs|live_receipt_status_requires_every_chunk_profile_generation_and_source#1|4|0f59e1cd6705b1fd4ed88552a19733b8f16c5d549038a34a7250b156f13cce94',
})

# These are SOURCE contracts, not permissions to execute forbidden SQL. The
# native adapter forwards already-audited query/raw_sql constructor inputs.
# Arbitrary SQL supplied by external library callers is outside repository-owned
# SQL. Any native adapter source edit invalidates these six forwarding proofs.
NATIVE_FORWARD_SHA = "9d96843ad2300df75daa8b515d6d7d61b66054f417c4f0305ba38f8651c3c63f"
NATIVE_FORWARD_SITES = {("begin_with#1", 1), ("query#1", 1), ("execute#1", 1),
                        ("fetch_all#1", 1), ("query_scalar#1", 1), ("execute#2", 1)}
# scoped_filter accepts only Scope(Uuid, Uuid), i64 and &[Uuid]. Uuid Display
# emits hex/hyphens, i64 emits signed digits, and the only list separator is ','.
# The exact constructor plus its Scope definition establish a predicate-only
# language with no relation, subquery or injectable SQL syntax.
FILTER_SOURCE_SHAS = {
    "src/storage/lancedb/ann.rs": "a0f9f2aeb7384d91438cbdd1e6bd237bd85d9cd517f6cfd585d60a1e8d1c40cc",
    "src/storage/lancedb/full_text.rs": "137349feb1f39e5bdb7595369fbce8d194dbf333796c1e79149173e6a553ddc0",
    "src/storage/lancedb.rs": "fd13d098719af04ed33ff12dcd0cf75a11c5f132b7c9bcabe066ced855f66343",
    "src/storage/scope.rs": "98b958abaa15c09528abc260dfc8877ba6e0c83bb739100737745ff26f37cb87",
}

SINKS = {"query", "query_as", "query_scalar", "raw_sql", "begin_with",
         "execute_batch", "prepare", "prepare_cached", "only_if", "filter"}
MAX_VARIANTS = 4096


@dataclass(frozen=True)
class Token:
    kind: str
    text: str
    start: int
    end: int


class Unknown(ValueError):
    pass


def rust_tokens(source: str) -> list[Token]:
    """Skip nested Rust comments; retain strings without interpreting their SQL."""
    out: list[Token] = []
    i = 0
    while i < len(source):
        if source[i].isspace():
            i += 1
            continue
        if source.startswith("//", i):
            j = source.find("\n", i)
            i = len(source) if j < 0 else j
            continue
        if source.startswith("/*", i):
            depth, j = 1, i + 2
            while j < len(source) and depth:
                if source.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif source.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            if depth:
                raise Unknown("unterminated Rust comment")
            i = j
            continue
        raw = re.match(r'(?:br|r)(#{0,255})"', source[i:])
        if raw:
            end = '"' + raw.group(1)
            j = source.find(end, i + raw.end())
            if j < 0:
                raise Unknown("unterminated Rust raw string")
            out.append(Token("str", source[i + raw.end():j], i, j + len(end)))
            i = j + len(end)
            continue
        if source[i] == '"' or source.startswith('b"', i):
            start, j = i, i + (2 if source[i] == "b" else 1)
            value = []
            while j < len(source) and source[j] != '"':
                if source[j] != "\\":
                    value.append(source[j]); j += 1
                    continue
                j += 1
                if j >= len(source):
                    raise Unknown("unterminated Rust escape")
                c = source[j]
                if c in "\r\n":
                    while j < len(source) and source[j].isspace():
                        j += 1
                elif c in {'n', 'r', 't', '0', '\\', '"', "'"}:
                    value.append({'n': '\n', 'r': '\r', 't': '\t', '0': '\0',
                                  '\\': '\\', '"': '"', "'": "'"}[c]); j += 1
                elif c == "x" and re.fullmatch(r"[0-9a-fA-F]{2}", source[j+1:j+3]):
                    value.append(chr(int(source[j+1:j+3], 16))); j += 3
                elif c == "u" and source[j+1:j+2] == "{":
                    k = source.find("}", j+2)
                    if k < 0:
                        raise Unknown("unterminated Rust Unicode escape")
                    value.append(chr(int(source[j+2:k].replace("_", ""), 16))); j = k+1
                else:
                    raise Unknown("unsupported Rust escape")
            if j >= len(source):
                raise Unknown("unterminated Rust string")
            out.append(Token("str", "".join(value), start, j+1)); i = j+1
            continue
        # Char literals, including escaped quotes, must not create fake SQL tokens.
        char = re.match(r"'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'", source[i:])
        if char:
            out.append(Token("char", char.group(), i, i+char.end())); i += char.end()
            continue
        word = re.match(r"[A-Za-z_][A-Za-z_0-9]*|[0-9]+", source[i:])
        if word:
            out.append(Token("id", word.group(), i, i+word.end())); i += word.end()
        else:
            pair = source[i:i+2]
            text = pair if pair in {"::", "=>", "+=", "==", "!=", "->"} else source[i]
            out.append(Token("punct", text, i, i+len(text))); i += len(text)
    return out


def pairs(tokens: list[Token]) -> dict[int, int]:
    stack, result = [], {}
    for i, token in enumerate(tokens):
        if token.kind != "punct":
            continue
        if token.text in {"(", "[", "{"}:
            stack.append(i)
        elif token.text in {")", "]", "}"}:
            if not stack or tokens[stack[-1]].text != {")": "(", "]": "[", "}": "{"}[token.text]:
                raise Unknown("unbalanced Rust delimiter")
            a = stack.pop(); result[a] = i; result[i] = a
    if stack:
        raise Unknown("unbalanced Rust delimiter")
    return result


def split_top(tokens: list[Token], separator: str = ",") -> list[list[Token]]:
    mapping = pairs(tokens)
    result, start, i = [], 0, 0
    while i < len(tokens):
        if tokens[i].kind == "punct" and tokens[i].text in {"(", "[", "{"}:
            i = mapping[i] + 1; continue
        if tokens[i].kind == "punct" and tokens[i].text == separator:
            result.append(tokens[start:i]); start = i+1
        i += 1
    if start < len(tokens):
        result.append(tokens[start:])
    return result


@dataclass
class Value:
    texts: set[str] = field(default_factory=set)
    unknown: set[str] = field(default_factory=set)
    dependencies: set[str] = field(default_factory=set)
    escaped_quote: str | None = None

    @classmethod
    def missing(cls, reason: str) -> "Value":
        return cls({"__UNRESOLVED__"}, {reason})

    def merge(self, other: "Value") -> "Value":
        if not self.texts:
            return other
        if not other.texts:
            return Value(self.texts, self.unknown | other.unknown, self.dependencies | other.dependencies, self.escaped_quote)
        return Value(self.texts | other.texts, self.unknown | other.unknown,
                     self.dependencies | other.dependencies,
                     self.escaped_quote if self.escaped_quote == other.escaped_quote else None)


@dataclass
class Binding:
    name: str
    exprs: list[list[Token]]
    scope: tuple[int, ...]
    position: int
    constant: bool = False


class RustFile:
    def __init__(self, path: Path, root: Path):
        self.path, self.relative = path, path.relative_to(root).as_posix()
        self.source = path.read_text(encoding="utf-8")
        self.tokens = rust_tokens(self.source)
        self.match = pairs(self.tokens)
        self.scopes, stack = [], []
        for i, token in enumerate(self.tokens):
            self.scopes.append(tuple(stack))
            if token.text == "{" and token.kind == "punct":
                stack.append(i)
            elif token.text == "}" and token.kind == "punct":
                stack.pop()
        self.bindings: list[Binding] = []
        self.functions: list[tuple[int, int, str]] = []
        self.parameters: dict[str, list[str]] = {}
        self.return_types: dict[str, str] = {}
        self.external: set[str] = set()
        seen = Counter()
        ts = self.tokens
        for i, token in enumerate(ts):
            if token.text == "fn" and i+1 < len(ts) and ts[i+1].kind == "id":
                name = ts[i+1].text
                j = i+2
                while j < len(ts) and ts[j].text not in {"{", ";"}:
                    if ts[j].text in {"(", "["}:
                        j = self.match[j]
                    j += 1
                if j < len(ts) and ts[j].text == "{":
                    seen[name] += 1
                    owner = f"{name}#{seen[name]}"
                    self.functions.append((j, self.match[j], owner))
                    paren = next((k for k in range(i+2, j) if ts[k].text == "("), None)
                    if paren is not None:
                        args = split_top(ts[paren+1:self.match[paren]])
                        self.parameters[owner] = [next((t.text for t in a if t.kind == "id" and t.text != "mut"), "") for a in args]
                        after = self.match[paren]+1
                        if after+1 < j and ts[after].text == "->" and ts[after+1].kind == "id":
                            self.return_types[owner] = ts[after+1].text
                    # Public library input cannot be proven finite from this repository.
                    k = i-1
                    while k >= 0 and ts[k].text not in {";", "{", "}"}:
                        if ts[k].text == "extern" or (ts[k].text == "pub" and ts[k+1].text != "("):
                            self.external.add(owner)
                            break
                        k -= 1
            if token.text in {"let", "const", "static"}:
                j = i+1
                if j < len(ts) and ts[j].text == "mut":
                    j += 1
                if j >= len(ts) or ts[j].kind != "id":
                    continue
                name, k = ts[j].text, j+1
                while k < len(ts) and ts[k].text not in {"=", ";", "{"}:
                    k += 1
                if k >= len(ts) or ts[k].text != "=":
                    continue
                end = k+1
                while end < len(ts) and ts[end].text != ";":
                    if ts[end].text in {"(", "[", "{"} and ts[end].kind == "punct":
                        end = self.match[end]
                    end += 1
                self.bindings.append(Binding(name, [ts[k+1:end]], self.scopes[i],
                                             token.start, token.text != "let"))
            if token.text == "for":
                # Finite array loops, including (break_sql, restore_sql, id).
                j = i+1
                while j < len(ts) and ts[j].text not in {"in", "{", ";"}:
                    j += 1
                if j+1 >= len(ts) or ts[j].text != "in":
                    continue
                body = j+1
                while body < len(ts) and ts[body].text not in {"{", ";"}:
                    if ts[body].kind == "punct" and ts[body].text in {"[", "("}:
                        body = self.match[body]
                    body += 1
                if body >= len(ts) or ts[body].text != "{":
                    continue
                pattern = ts[i+1:j]
                if len(pattern) > 1 and pattern[0].text == "(":
                    pattern = pattern[1:-1]
                names = [x[0].text for x in split_top(pattern) if len(x) == 1]
                inline = ts[j+1].text == "["
                items = split_top(ts[j+2:self.match[j+1]]) if inline else []
                for n, name in enumerate(names):
                    exprs = []
                    if not inline:
                        # First component of array.into_iter().zip(other) comes
                        # from that finite array; other components stay unknown.
                        if n == 0 and ts[j+1].kind == "id":
                            exprs = [[ts[j+1]]]
                    for item in items:
                        if len(names) == 1:
                            exprs.append(item)
                        elif item and item[0].text == "(" and item[-1].text == ")":
                            cols = split_top(item[1:-1])
                            if n < len(cols):
                                exprs.append(cols[n])
                    self.bindings.append(Binding(name, exprs,
                        self.scopes[body] + (body,), ts[body].start))

    def owner(self, index: int) -> str:
        owners = [(end-start, name) for start, end, name in self.functions if start < index < end]
        return min(owners)[1] if owners else "<module>"

    def snippet(self, tokens: list[Token]) -> str:
        return self.source[tokens[0].start:tokens[-1].end] if tokens else ""


class Evaluator:
    def __init__(self, root: Path, files: list[RustFile]):
        self.root, self.files = root, files

    def name(self, file: RustFile, name: str, at: int, seen: frozenset) -> Value:
        key = (file.relative, name, at)
        if key in seen:
            return Value.missing(f"cyclic binding {name}")
        seen = seen | {key}
        short = name.split("::")[-1]
        index = next((i for i, t in enumerate(file.tokens) if t.start >= at), len(file.tokens)-1)
        scope = file.scopes[index] if file.tokens else ()
        candidates = [(file, b) for b in file.bindings if b.name == short
                      and (b.constant or b.position < at)
                      and scope[:len(b.scope)] == b.scope]
        if "::" in name:
            module = name.split("::")[-2]
            candidates = [(f, b) for f in self.files for b in f.bindings
                          if b.constant and b.name == short and f.path.stem == module]
        elif short in file.parameters.get(file.owner(index), []):
            # Parameters shadow any outer/module constants. An actual lexical
            # let binding inside this function may shadow the parameter.
            start = next(a for a, _, owner in file.functions if owner == file.owner(index))
            local = [(f, b) for f, b in candidates if start in b.scope]
            if not local:
                return self.parameter(file, file.owner(index), short, seen)
            candidates = [max(local, key=lambda fb: (len(fb[1].scope), fb[1].position))]
        elif candidates:
            candidates.sort(key=lambda fb: (len(fb[1].scope), fb[1].position), reverse=True)
            candidates = candidates[:1]
        else:
            # Lexical parameters shadow constants from other modules. Never
            # turn an unconstrained public SQL input into an unrelated constant.
            owner = file.owner(index)
            if short in file.parameters.get(owner, []):
                return self.parameter(file, owner, short, seen)
            candidates = [(f, b) for f in self.files for b in f.bindings
                          if b.constant and b.name == short]
        if not candidates and "::" not in name:
            owner = file.owner(index)
            if short in file.parameters.get(owner, []):
                return self.parameter(file, owner, short, seen)
        if len(candidates) != 1:
            return Value.missing(f"unresolved/ambiguous binding {name}")
        f, binding = candidates[0]
        result = Value()
        for expr in binding.exprs:
            result = result.merge(self.eval(f, expr, seen))
        if not binding.exprs:
            result = Value.missing(f"empty binding {name}")
        if not binding.constant and f is file:
            # Never silently evaluate an initializer after later mutation.
            initialized = max((e[-1].end for e in binding.exprs if e), default=binding.position)
            between = [t for t in f.tokens if initialized < t.start < at]
            for i, t in enumerate(between[:-2]):
                if t.text == short and between[i+1].text in {"+=", "="}:
                    result.unknown.add(f"mutated binding {name}")
                if t.text == "&" and between[i+1].text == "mut" and between[i+2].text == short:
                    result.unknown.add(f"mutable alias/borrow of SQL binding {name}")
                if (t.text == short and between[i+1].text == "."
                        and between[i+2].text not in {"as_str", "len", "is_empty", "clone", "to_owned", "to_string"}):
                    if between[i+2].text == "push_str" and i+3 < len(between) and between[i+3].text == "(":
                        opening = next(k for k, token in enumerate(f.tokens) if token.start == between[i+3].start)
                        closing = f.match.get(opening)
                        if closing is not None:
                            suffix = self.eval(f, f.tokens[opening+1:closing], seen)
                            # Conservatively include both paths of an enclosing if.
                            appended = self.combine([result, suffix], lambda xs: "".join(xs))
                            result = result.merge(appended)
                            continue
                    result.unknown.add(f"mutated binding {name}")
        return result

    def parameter(self, file: RustFile, owner: str, name: str, seen: frozenset) -> Value:
        """Finite internal call-site overapproximation, not public-input inference."""
        key = (file.relative, owner, "parameter", name)
        if key in seen or owner in file.external:
            return Value.missing(f"unbounded public/recursive SQL parameter {name}")
        seen = seen | {key}
        function = owner.split("#")[0]
        position = file.parameters[owner].index(name)
        definitions = [(f, n) for f in self.files for _, _, n in f.functions if n.split("#")[0] == function]
        # Repeated private fixture helper names resolve only in their own module.
        sources = self.files
        result, found, counters = Value(), False, Counter()
        for caller in sources:
            ts = caller.tokens
            for i, token in enumerate(ts):
                if token.kind != "id" or token.text != function or i and ts[i-1].text == "fn":
                    continue
                if len(definitions) != 1 and caller is not file:
                    if not (i > 1 and ts[i-1].text == "::" and ts[i-2].text == file.path.stem):
                        continue
                if i+1 >= len(ts) or ts[i+1].text != "(":
                    scope = caller.scopes[i]
                    if any(b.name == function and b.position < token.start
                           and scope[:len(b.scope)] == b.scope for b in caller.bindings):
                        continue  # same-spelled local value, not a function pointer
                    # Imports are not calls; aliases/function pointers are unsupported.
                    start = i-1
                    while start >= 0 and ts[start].text not in {";", "}"}:
                        if ts[start].text == "use": break
                        start -= 1
                    if start >= 0 and ts[start].text == "use" and not (i+1 < len(ts) and ts[i+1].text == "as"):
                        continue
                    result.unknown.add(f"non-call reference to SQL helper {function}")
                    continue
                if i and ts[i-1].text == ".":
                    continue
                args = split_top(ts[i+2:caller.match[i+1]])
                if position >= len(args):
                    result.unknown.add(f"unresolved arguments of SQL helper {function}"); continue
                found = True
                owner_at_call = caller.owner(i)
                counters[(caller.relative, owner_at_call)] += 1
                value = self.eval(caller, args[position], seen)
                value.dependencies.add(f"call:{caller.relative}:{owner_at_call}:{counters[(caller.relative, owner_at_call)]}:{caller.snippet(args[position])}")
                result = result.merge(value)
        return result if found else result.merge(Value.missing(f"no finite callers for SQL helper {function}"))

    def eval(self, file: RustFile, ts: list[Token], seen: frozenset = frozenset()) -> Value:
        source = file.snippet(ts)
        dependency = file.relative + ":" + source
        result = self._eval(file, ts, seen)
        result.dependencies.add(dependency)
        return result

    def _eval(self, file: RustFile, ts: list[Token], seen: frozenset) -> Value:
        while ts and ts[0].text == "&":
            ts = ts[1:]
        if not ts:
            return Value.missing("empty expression")
        if ts[0].text == "scoped_filter" and len(ts) > 2 and ts[1].text == "(":
            if file.relative in {"src/storage/lancedb.rs", "src/storage/lancedb/ann.rs", "src/storage/lancedb/full_text.rs"} and all((self.root / path).is_file() and hashlib.sha256((self.root / path).read_bytes()).hexdigest() == sha
                   for path, sha in FILTER_SOURCE_SHAS.items()):
                return Value({"tenant='00000000-0000-0000-0000-000000000000' AND workspace='00000000-0000-0000-0000-000000000000' AND generation=0 AND id IN ('00000000-0000-0000-0000-000000000000')"},
                             dependencies={"typed scoped_filter source contract:"+json.dumps(FILTER_SOURCE_SHAS, sort_keys=True)})
            return Value.missing("scoped_filter source/type contract changed or unavailable")
        mapping = pairs(ts)
        if ts[0].text == "(" and mapping.get(0) == len(ts)-1:
            return self.eval(file, ts[1:-1], seen)
        if ts[0].text == "[" and mapping.get(0) == len(ts)-1:
            result = Value()
            for expr in split_top(ts[1:-1]):
                result = result.merge(self.eval(file, expr, seen))
            return result
        if len(ts) == 1 and re.fullmatch(r"[0-9][0-9_]*", ts[0].text):
            return Value({str(int(ts[0].text.replace("_", "")))})
        if len(ts) == 1 and ts[0].kind == "str":
            return Value({ts[0].text})
        if len(ts) == 1 and ts[0].kind == "char":
            try:
                return Value({ast.literal_eval(ts[0].text)})
            except (ValueError, SyntaxError):
                return Value.missing("unsupported Rust char escape")
        if len(ts) == 3 and ts[0].kind == "id" and ts[1].text == "." and ts[2].kind == "id":
            # A field of a locally-bound struct or a function with an explicit
            # struct return type. Only uuid::Uuid has a constrained SQL spelling.
            base, field_name = ts[0].text, ts[2].text
            index = next(i for i, t in enumerate(file.tokens) if t.start == ts[0].start)
            scope = file.scopes[index]
            bindings = [b for b in file.bindings if b.name == base and b.position < ts[0].start
                        and scope[:len(b.scope)] == b.scope]
            if bindings:
                binding = max(bindings, key=lambda b: (len(b.scope), b.position))
                if len(binding.exprs) == 1:
                    expression = binding.exprs[0]
                    typ = None
                    if len(expression) > 1 and expression[1].text == "{":
                        typ = expression[0].text
                    elif len(expression) > 1 and expression[1].text == "(":
                        returns = [kind for owner, kind in file.return_types.items()
                                   if owner.split("#")[0] == expression[0].text]
                        if len(returns) == 1: typ = returns[0]
                    definitions = []
                    for f in self.files:
                        for n, token in enumerate(f.tokens[:-3]):
                            if token.text == "struct" and f.tokens[n+1].text == typ and f.tokens[n+2].text == "{":
                                definitions.append((f, n+2, f.match[n+2]))
                    if len(definitions) == 1:
                        f, begin, end = definitions[0]
                        for n in range(begin+1, end-2):
                            if (f.tokens[n].text == field_name and f.tokens[n+1].text == ":"
                                    and f.tokens[n+2].text == "Uuid" and re.search(r"use\s+uuid::Uuid\s*;", f.source)):
                                return Value({"00000000-0000-0000-0000-000000000000"}, dependencies={
                                    "uuid field type:"+f.relative+":"+hashlib.sha256(f.source.encode()).hexdigest(),
                                    "local typed value:"+file.snippet(expression)})
        # Only identity string conversions and literal replace are interpreted.
        for i, token in enumerate(ts):
            if token.text == "." and i+2 < len(ts) and ts[i+2].text == "(" and mapping.get(i+2) == len(ts)-1:
                method = ts[i+1].text
                args = split_top(ts[i+3:-1])
                if method == "join" and len(args) == 1:
                    base_tokens = ts[:i]
                    if (len(base_tokens) >= 6 and base_tokens[0].text == "vec"
                            and base_tokens[1].text == "!" and base_tokens[2].text == "["
                            and base_tokens[-1].text == "]"):
                        repeated = split_top(base_tokens[3:-1], ";")
                        if len(repeated) == 2:
                            atom, separator = self.eval(file, repeated[0], seen), self.eval(file, args[0], seen)
                            if not atom.unknown and not separator.unknown:
                                valid = all(re.fullmatch(r"\?|\(\w+=\? AND \w+=\?\)", x) for x in atom.texts)
                                if valid and separator.texts <= {",", " OR "}:
                                    # Regular-language proof: only bind placeholders/
                                    # fixed local predicates repeat, with balanced
                                    # parentheses; repetition cannot add a relation.
                                    result = self.combine([atom, separator], lambda xs: xs[0]+xs[1]+xs[0])
                                    result.dependencies.add("certified bind-predicate repetition:"+file.snippet(base_tokens))
                                    return result
                    return Value.missing("unsupported dynamic join construction")
                base = self.eval(file, ts[:i], seen)
                if method in {"as_str", "to_owned", "to_string"} and not args:
                    return base
                if method == "replace" and len(args) == 2:
                    old, new = (self.eval(file, a, seen) for a in args)
                    if len(old.texts) == len(new.texts) == 1 and not old.unknown and not new.unknown:
                        before, after = next(iter(old.texts)), next(iter(new.texts))
                        if base.unknown and before in {'"', "'"} and after == before * 2:
                            return Value({"__ENCODED_VALUE__"}, base.unknown,
                                         base.dependencies | old.dependencies | new.dependencies,
                                         before)
                        return Value({x.replace(next(iter(old.texts)), next(iter(new.texts))) for x in base.texts},
                                     base.unknown, base.dependencies | old.dependencies | new.dependencies)
                return Value.missing(f"unsupported string method {method}")
        if len(ts) >= 3 and ts[1].text == "!" and ts[2].text in {"(", "{"} and mapping.get(2) == len(ts)-1:
            macro = ts[0].text
            args = split_top(ts[3:-1])
            if macro == "include_str" and len(args) == 1:
                path = self.eval(file, args[0], seen)
                if path.unknown or len(path.texts) != 1:
                    return Value.missing("dynamic include_str path")
                target = (file.path.parent / next(iter(path.texts))).resolve()
                if not target.is_relative_to(self.root) or not target.is_file():
                    return Value.missing("missing/outside-root include_str")
                text = target.read_text(encoding="utf-8")
                return Value({text}, dependencies={target.relative_to(self.root).as_posix()+":"+text})
            if macro == "concat":
                values = [self.eval(file, a, seen) for a in args]
                return self.combine(values, lambda xs: "".join(xs))
            if macro == "format" and args:
                template = self.eval(file, args[0], seen)
                if template.unknown or len(template.texts) != 1:
                    return Value.missing("dynamic format template")
                positional, named = [], {}
                for arg in args[1:]:
                    if len(arg) >= 3 and arg[0].kind == "id" and arg[1].text == "=":
                        named[arg[0].text] = self.eval(file, arg[2:], seen)
                    else:
                        positional.append(self.eval(file, arg, seen))
                parts, values, next_pos = [], [], 0
                try:
                    for literal, field_name, spec, conversion in string.Formatter().parse(next(iter(template.texts))):
                        parts.append(literal)
                        if field_name is None:
                            continue
                        if spec or conversion:
                            return Value.missing("unsupported format conversion")
                        if field_name == "":
                            value = positional[next_pos] if next_pos < len(positional) else Value.missing("missing format argument")
                            next_pos += 1
                        elif field_name.isdigit():
                            n = int(field_name)
                            value = positional[n] if n < len(positional) else Value.missing("missing format argument")
                        else:
                            value = named.get(field_name) or self.name(file, field_name, ts[0].start, seen)
                        parts.append(len(values)); values.append(value)
                    for n, value in enumerate(values):
                        if value.escaped_quote:
                            place = parts.index(n)
                            quote = value.escaped_quote
                            prefix_parts = parts[:place]
                            prefix = self.combine([values[p] if isinstance(p, int) else Value({p}) for p in prefix_parts], "".join)
                            if (not prefix.unknown and prefix.texts and place > 0 and place+1 < len(parts)
                                    and isinstance(parts[place-1], str) and isinstance(parts[place+1], str)
                                    and parts[place-1].endswith(quote) and parts[place+1].startswith(quote)
                                    and all(sql_prefix_state(text) == quote for text in prefix.texts)):
                                values[n] = Value(value.texts, dependencies=value.dependencies | {"proved SQL quote doubling and surrounding delimiters"})
                    combined = self.combine(values, lambda xs: "".join(xs[p] if isinstance(p, int) else p for p in parts))
                    combined.dependencies |= template.dependencies
                    return combined
                except ValueError:
                    return Value.missing("unsupported format template")
            return Value.missing(f"unsupported macro {macro}")
        if ts[0].text == "if":
            # Evaluate both result branches; conditions are not executed.
            start = next((i for i, t in enumerate(ts) if t.text == "{"), None)
            if start is not None:
                end = mapping[start]
                first = self.eval(file, ts[start+1:end], seen)
                tail = ts[end+1:]
                if tail and tail[0].text == "else":
                    tail = tail[1:]
                    if tail and tail[0].text == "{" and pairs(tail).get(0) == len(tail)-1:
                        tail = tail[1:-1]
                    return first.merge(self.eval(file, tail, seen))
            return Value.missing("unsupported conditional SQL")
        if all(t.kind == "id" or t.text == "::" for t in ts):
            name = "".join(t.text for t in ts)
            return self.name(file, name, ts[0].start, seen)
        return Value.missing("unsupported expression: " + file.snippet(ts)[:100].replace("\n", " "))

    @staticmethod
    def combine(values: list[Value], render) -> Value:
        count = 1
        for value in values:
            count *= len(value.texts)
        if count > MAX_VARIANTS:
            return Value.missing("static expansion exceeds variant limit")
        return Value({render(xs) for xs in itertools.product(*(v.texts for v in values))},
                     set().union(*(v.unknown for v in values)),
                     set().union(*(v.dependencies for v in values)))


def sql_prefix_state(sql: str) -> str:
    """State at an interpolation boundary; a closing quote is NOT an opener."""
    state, i = "code", 0
    while i < len(sql):
        if state == "line-comment":
            if sql[i] in "\r\n": state = "code"
            i += 1
        elif state == "block-comment":
            if sql.startswith("*/", i): state, i = "code", i+2
            else: i += 1
        elif state != "code":
            close = "]" if state == "[" else state
            if sql[i] == close:
                if state != "[" and sql[i:i+2] == close*2: i += 2
                else: state, i = "code", i+1
            else: i += 1
        elif sql.startswith("--", i): state, i = "line-comment", i+2
        elif sql.startswith("/*", i): state, i = "block-comment", i+2
        elif sql[i] in "'\"`[": state, i = sql[i], i+1
        else: i += 1
    return state


def sql_tokens(sql: str) -> list[Token]:
    """SQL quotes are data/identifiers, never keywords. Comments carry no SQL."""
    out, i = [], 0
    while i < len(sql):
        if sql[i].isspace():
            i += 1; continue
        if sql.startswith("--", i):
            end = sql.find("\n", i); i = len(sql) if end < 0 else end; continue
        if sql.startswith("/*", i):
            end = sql.find("*/", i+2)
            if end < 0:
                raise Unknown("unterminated SQL comment")
            i = end+2; continue
        if sql[i] in "'\"`[":
            start, quote, value = i, sql[i], []
            close = "]" if quote == "[" else quote
            i += 1
            while i < len(sql):
                if sql[i] == close:
                    if quote != "[" and sql[i:i+2] == close*2:
                        value.append(close); i += 2; continue
                    i += 1; break
                value.append(sql[i]); i += 1
            else:
                raise Unknown("unterminated SQL quote")
            out.append(Token("str" if quote == "'" else "quoted", "".join(value), start, i)); continue
        word = re.match(r"[A-Za-z_][A-Za-z_0-9$]*|[0-9]+", sql[i:])
        if word:
            out.append(Token("word", word.group().lower(), i, i+word.end())); i += word.end()
        else:
            out.append(Token("punct", sql[i], i, i+1)); i += 1
    return out


def sql_issues(sql: str) -> set[str]:
    ts = sql_tokens(sql)
    if not ts:
        return set()
    issues: set[str] = set()
    # Each executable statement is separate. Trigger NEW/OLD fields are scalar
    # values of the triggering row, not extra scanned relations. The trigger's
    # declaration is not a query; WHEN and each body DML are checked separately.
    segments, start = [], 0
    for i, t in enumerate(ts):
        if t.text == ";" and t.kind == "punct":
            segments.append(ts[start:i]); start = i+1
    if start < len(ts):
        segments.append(ts[start:])
    queries = []
    for segment in segments:
        words = [t.text if t.kind == "word" else None for t in segment]
        if words and words[0] == "create" and "trigger" in words[:4]:
            # CREATE [TEMP|TEMPORARY] TRIGGER ... [WHEN expr] BEGIN statement;
            # Ignore declaration operations such as INSTEAD OF UPDATE ON a.
            # Neither that ON target nor NEW/OLD.column is a FROM relation.
            depth, when, begin = 0, None, None
            for i, token in enumerate(segment):
                if token.kind == "punct" and token.text == "(": depth += 1
                elif token.kind == "punct" and token.text == ")": depth -= 1
                elif depth == 0 and token.kind == "word" and not (i and segment[i-1].text == "."):
                    if token.text == "when" and when is None: when = i
                    elif token.text == "begin":
                        begin = i
                        break
            if begin is None:
                raise Unknown("unsupported CREATE TRIGGER without BEGIN")
            if when is not None: queries.append(segment[when+1:begin])
            queries.append(segment[begin+1:])
        else:
            queries.append(segment)
    for segment in queries:
        if not segment:
            continue
        words = [t.text if t.kind == "word" else None for t in segment]
        if "join" in words:
            issues.add("explicit JOIN")
        ctes = set()
        if words[0] == "with":
            depth = 0
            for i, t in enumerate(segment[:-2]):
                if t.text == "(": depth += 1
                elif t.text == ")": depth -= 1
                if depth == 0 and t.kind in {"word", "quoted"}:
                    j = i+1
                    if segment[j].text == "as" or (segment[j].text == "(" and t.text not in {"with", "recursive"}):
                        ctes.add(t.text.lower())
        refs, cte_uses = [], Counter()
        depth, from_levels = 0, set()
        for i, t in enumerate(segment):
            kw = t.text if t.kind == "word" else None
            if t.text == "(" and t.kind == "punct":
                depth += 1
            elif t.text == ")" and t.kind == "punct":
                from_levels.discard(depth); depth -= 1
                if depth < 0:
                    raise Unknown("unbalanced SQL parentheses")
            elif t.text == "," and t.kind == "punct" and depth in from_levels:
                issues.add("comma FROM / implicit JOIN")
            if kw in {"where", "group", "order", "having", "limit", "union", "except", "intersect", "window", "returning", "set", "values"}:
                from_levels.discard(depth)
            relation = False
            j = i+1
            if kw in {"from", "join"}:
                relation = True
                if kw == "from": from_levels.add(depth)
            elif kw == "update" and (i == 0 or words[i-1] not in {"on", "do", "after", "before"}):
                relation = True
                if j < len(segment) and segment[j].text == "or": j += 2
            elif kw == "into" and any(x in {"insert", "replace"} for x in words[:i]):
                relation = True
            if relation and j < len(segment) and segment[j].kind in {"word", "quoted"}:
                name = segment[j].text.lower()
                if j+2 < len(segment) and segment[j+1].text == ".":
                    name += "." + segment[j+2].text.lower()
                if name in ctes:
                    cte_uses[name] += 1
                else:
                    refs.append(name)
        if depth:
            raise Unknown("unbalanced SQL parentheses")
        if any(n > 1 for n in cte_uses.values()):
            issues.add("reused CTE / self JOIN")
        if len(set(refs)) > 1:
            issues.add("cross-relation statement (including subquery/CTE)")
        elif len(refs) > 1:
            issues.add("self-relation statement (including subquery)")
    return issues


@dataclass
class Finding:
    path: str
    line: int
    owner: str
    ordinal: int
    sink: str
    fingerprint: str
    issues: set[str]
    unresolved: set[str]
    variants: int

    @property
    def key(self) -> str:
        return f"{self.path}|{self.owner}|{self.ordinal}|{self.fingerprint}"


def calls(file: RustFile):
    ts = file.tokens
    ordinals = Counter()
    aliases = {}
    for i, token in enumerate(ts[:-2]):
        if token.text in SINKS and ts[i+1].text == "as" and ts[i+2].kind == "id":
            aliases[ts[i+2].text] = token.text
    for binding in file.bindings:
        if len(binding.exprs) == 1:
            expr = binding.exprs[0]
            if expr and expr[-1].text in SINKS and all(t.kind == "id" or t.text == "::" for t in expr):
                aliases[binding.name] = expr[-1].text
    for i, token in enumerate(ts):
        name = aliases.get(token.text, token.text)
        if token.kind != "id" or (name not in SINKS and name != "execute"):
            continue
        if i and ts[i-1].text == "fn":
            continue
        qualified = i > 0 and (ts[i-1].text == "::" or ts[i-1].text == "." and not (i > 1 and ts[i-2].text == "."))
        ufcs = qualified and ts[i-1].text == "::" and name in {"execute", "execute_batch", "prepare", "prepare_cached", "query"} and i > 1 and (name in {"execute", "execute_batch"} or ts[i-2].text in {"Connection", "Transaction", "Executor", "RawConnection"})
        if not ufcs and (not qualified or ts[i-1].text == "::") and name in {"prepare", "prepare_cached", "filter", "only_if", "execute", "execute_batch"}:
            continue  # e.g. Worker::prepare is a domain function, not a SQL API.
        if not qualified and token.text == "query":
            declarations = [k for k in range(len(ts)-3)
                            if ts[k].text == "fn" and ts[k+1].text == "query" and ts[k+2].text == "("]
            if declarations:
                signatures = [split_top(ts[k+3:file.match[k+2]]) for k in declarations]
                if not any(sig and any(t.text in {"str", "String"} for t in sig[0]) for sig in signatures):
                    continue  # typed VectorQuery/FullTextQuery fixture constructor
        j = i+1
        macro = j < len(ts) and ts[j].text == "!"
        if macro: j += 1
        if j+1 < len(ts) and ts[j].text == "::" and ts[j+1].text == "<":
            j += 2; depth = 1
            while j < len(ts) and depth:
                if ts[j].text == "<": depth += 1
                elif ts[j].text == ">": depth -= 1
                j += 1
        if j >= len(ts) or ts[j].text != "(":
            continue
        end = file.match[j]
        args = split_top(ts[j+1:end])
        if not args:
            continue
        if token.text == "execute" and len(args) < 2:
            continue  # adapter .execute(executor) has no SQL argument
        if token.text == "filter":
            # Rust iterators use .filter(closure); they are not SQL APIs.
            if args[0] and args[0][0].text in {"|", "move"}:
                continue
        owner = file.owner(i); ordinals[owner] += 1
        argument = args[1] if (ufcs or macro and name == "query_as") and len(args) > 1 else args[0]
        yield i, name, argument, owner, ordinals[owner]


def selected_site(path: str, owner: str, selected: set[str] | None) -> bool:
    return selected is None or path in selected or f"{path}::{owner.split('#')[0]}" in selected


class PythonInputs:
    """Bounded AST audit of actual Python DB calls, not strings containing code.

    Literal SQL, finite assignments/loops/comprehensions and string concatenation
    are modeled. Runtime formatting, imports, aliases and mutation not proven by
    these rules remain UNRESOLVED. No Python module is imported or executed.
    """
    METHODS = {"execute", "executemany", "executescript"}

    def __init__(self, path: Path, root: Path):
        self.path, self.relative = path, path.relative_to(root).as_posix()
        self.source = path.read_text()
        self.tree = ast.parse(self.source, filename=self.relative)
        self.parents = {child: parent for parent in ast.walk(self.tree) for child in ast.iter_child_nodes(parent)}
        self.nodes = list(ast.walk(self.tree))
        self.owners = {}
        counts = Counter()
        for node in self.nodes:
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                counts[node.name] += 1
                self.owners[node] = f"{node.name}#{counts[node.name]}"

    def scope(self, node):
        while node in self.parents:
            node = self.parents[node]
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda)):
                return node
        return self.tree

    def assignments(self, name, at):
        scope = self.scope(at)
        result = []
        for node in self.nodes:
            if self.scope(node) is not scope or getattr(node, "lineno", 0) > at.lineno:
                continue
            if isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == name for t in node.targets):
                result.append(node.value)
            elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name) and node.target.id == name and node.value:
                result.append(node.value)
        return result

    def finite_elements(self, node, at, seen):
        if isinstance(node, ast.Name):
            key = (node.id, node.lineno)
            if key in seen: raise Unknown("cyclic Python finite collection")
            values = self.assignments(node.id, at)
            if len(values) != 1: raise Unknown("unresolved Python finite collection " + node.id)
            for other in self.nodes:
                if self.scope(other) is not self.scope(at) or not values[0].lineno <= getattr(other, "lineno", 0) <= at.lineno: continue
                if isinstance(other, ast.Call):
                    mutated = (isinstance(other.func, ast.Attribute) and isinstance(other.func.value, ast.Name)
                               and other.func.value.id == node.id and other.func.attr not in {"items", "keys", "values"})
                    escaped = any(isinstance(arg, ast.Name) and arg.id == node.id for arg in other.args)
                    if mutated or escaped: raise Unknown("mutated/escaped Python SQL collection " + node.id)
                if isinstance(other, (ast.Assign, ast.AugAssign)):
                    targets = other.targets if isinstance(other, ast.Assign) else [other.target]
                    if any(isinstance(t, ast.Subscript) and isinstance(t.value, ast.Name) and t.value.id == node.id for t in targets):
                        raise Unknown("mutated Python SQL collection " + node.id)
            return self.finite_elements(values[0], at, seen | {key})
        if isinstance(node, (ast.List, ast.Tuple, ast.Set)):
            return node.elts
        if isinstance(node, ast.Dict):
            if any(k is None for k in node.keys): raise Unknown("unresolved Python dictionary expansion")
            return node.values
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and node.func.attr in {"items", "keys", "values"} and not node.args:
            base = node.func.value
            if isinstance(base, ast.Name):
                # Validate mutable collection provenance before projecting values.
                dictionary_name = base
                values = self.assignments(base.id, at)
                if len(values) != 1: raise Unknown("unresolved Python dictionary")
                self.finite_elements(dictionary_name, at, seen)
                base = values[0]
            if not isinstance(base, ast.Dict) or any(k is None for k in base.keys):
                raise Unknown("unresolved Python dictionary expansion")
            if node.func.attr == "keys": return base.keys
            if node.func.attr == "values": return base.values
            return [ast.Tuple(elts=[k, v], ctx=ast.Load()) for k, v in zip(base.keys, base.values)]
        raise Unknown("unsupported Python finite iterable")

    def eval(self, node, at=None, seen=frozenset()):
        at = at or node
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            return Value({node.value}, dependencies={ast.dump(node, include_attributes=False)})
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
            return Evaluator.combine([self.eval(node.left, at, seen), self.eval(node.right, at, seen)], "".join)
        if isinstance(node, ast.IfExp):
            return self.eval(node.body, at, seen).merge(self.eval(node.orelse, at, seen))
        if isinstance(node, ast.Name):
            key = (node.id, node.lineno)
            if key in seen: return Value.missing("cyclic Python SQL binding " + node.id)
            seen = seen | {key}
            cursor = at
            while cursor in self.parents:
                cursor = self.parents[cursor]
                generators = cursor.generators if isinstance(cursor, (ast.DictComp, ast.ListComp, ast.SetComp, ast.GeneratorExp)) else [cursor] if isinstance(cursor, (ast.For, ast.AsyncFor)) else []
                for generator in generators:
                    target = generator.target
                    names = [target.id] if isinstance(target, ast.Name) else [t.id if isinstance(t, ast.Name) else "" for t in target.elts] if isinstance(target, (ast.Tuple, ast.List)) else []
                    if node.id not in names: continue
                    result = Value()
                    for item in self.finite_elements(generator.iter, at, seen):
                        if len(names) > 1:
                            if not isinstance(item, (ast.Tuple, ast.List)) or len(item.elts) != len(names):
                                return Value.missing("unsupported Python tuple binding")
                            item = item.elts[names.index(node.id)]
                        result = result.merge(self.eval(item, at, seen))
                    return result
                if isinstance(cursor, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda)): break
            values = self.assignments(node.id, at)
            if not values:
                return Value.missing("unresolved Python SQL binding " + node.id)
            result = Value()
            for value in values:
                result = result.merge(self.eval(value, value, seen))
            earliest = min(v.lineno for v in values)
            for other in self.nodes:
                if self.scope(other) is not self.scope(at) or not earliest <= getattr(other, "lineno", 0) <= at.lineno: continue
                if isinstance(other, ast.AugAssign) and isinstance(other.target, ast.Name) and other.target.id == node.id:
                    result.unknown.add("mutated Python SQL binding " + node.id)
                if isinstance(other, ast.Call) and isinstance(other.func, ast.Attribute) and isinstance(other.func.value, ast.Name) and other.func.value.id == node.id:
                    result.unknown.add("unmodeled Python SQL binding method " + node.id)
            result.dependencies.add("Python binding:" + node.id + ":" + ";".join(ast.dump(v, include_attributes=False) for v in values))
            return result
        return Value.missing("unsupported Python SQL expression: " + ast.dump(node, include_attributes=False)[:160])

    def findings(self, selected):
        ordinals = Counter()
        aliases = {}
        for node in self.nodes:
            if isinstance(node, ast.Assign) and isinstance(node.value, ast.Attribute) and node.value.attr in self.METHODS:
                for target in node.targets:
                    if isinstance(target, ast.Name): aliases[target.id] = node.value.attr
        for node in sorted((n for n in self.nodes if isinstance(n, ast.Call)), key=lambda n: (n.lineno, n.col_offset)):
            sink = node.func.attr if isinstance(node.func, ast.Attribute) and node.func.attr in self.METHODS else aliases.get(node.func.id) if isinstance(node.func, ast.Name) else None
            if not sink: continue
            owner = self.owners.get(self.scope(node), "<module>")
            ordinals[owner] += 1
            if not selected_site(self.relative, owner, selected): continue
            expr = node.args[0] if node.args else next((k.value for k in node.keywords if k.arg in {"sql", "sql_script"}), None)
            try:
                value = self.eval(expr) if expr else Value.missing("missing Python SQL argument")
                issues = set().union(*(sql_issues(sql) for sql in value.texts))
            except (Unknown, ValueError, RecursionError) as error:
                value, issues = Value.missing(str(error)), set()
            content = json.dumps([sink, ast.dump(expr, include_attributes=False) if expr else "", sorted(value.texts), sorted(value.dependencies), sorted(value.unknown)])
            yield Finding(self.relative, node.lineno, owner, ordinals[owner], sink, hashlib.sha256(content.encode()).hexdigest(), issues, value.unknown, len(value.texts))


def audit(root: Path, selected: set[str] | None = None) -> list[Finding]:
    root = root.resolve()
    paths = []
    for dirname in ("src", "tests", "examples", "benches"):
        paths.extend(sorted((root / dirname).rglob("*.rs")))
    if (root / "build.rs").is_file(): paths.append(root / "build.rs")
    files, findings = [], []
    for path in paths:
        try:
            files.append(RustFile(path, root))
        except (Unknown, UnicodeError, ValueError) as error:
            relative = path.relative_to(root).as_posix()
            if selected is None or any(x.split("::", 1)[0] == relative for x in selected):
                findings.append(Finding(relative, 1, "<lex>", 0, "lex", "", set(), {str(error)}, 0))
    evaluator = Evaluator(root, files)
    for file in files:
        if selected is not None and not any(x.split("::", 1)[0] == file.relative for x in selected):
            continue
        for i, sink, expr, owner, ordinal in calls(file):
            if not selected_site(file.relative, owner, selected):
                continue
            try:
                if (file.relative == "src/storage/native_turso.rs" and (owner, ordinal) in NATIVE_FORWARD_SITES
                        and hashlib.sha256(file.source.encode()).hexdigest() == NATIVE_FORWARD_SHA):
                    value = Value(set(), dependencies={"verified query/raw_sql forwarding contract:"+NATIVE_FORWARD_SHA})
                else:
                    value = evaluator.eval(file, expr)
                issues = set()
                for sql in value.texts:
                    issues |= sql_issues(sql)
            except (Unknown, ValueError, RecursionError) as error:
                value, issues = Value.missing(str(error)), set()
            content = json.dumps([sink, file.snippet(expr), sorted(value.texts),
                                  sorted(value.dependencies), sorted(value.unknown)], ensure_ascii=False)
            fingerprint = hashlib.sha256(content.encode()).hexdigest()
            findings.append(Finding(file.relative, file.source.count("\n", 0, file.tokens[i].start)+1,
                                    owner, ordinal, sink, fingerprint, issues, value.unknown, len(value.texts)))
    python_files = []
    for dirname in ("tools", "tests", "examples"):
        for path in sorted((root / dirname).rglob("*.py")):
            try:
                file = PythonInputs(path, root)
                python_files.append(file)
                findings.extend(file.findings(selected))
            except (SyntaxError, UnicodeError, ValueError) as error:
                relative = path.relative_to(root).as_posix()
                if selected is None or any(x.split("::", 1)[0] == relative for x in selected):
                    findings.append(Finding(relative, 1, "<ast>", 0, "ast", "", set(), {str(error)}, 0))
    if selected:
        scanned = {f.relative for f in files}
        scanned |= {f"{f.relative}::{name.split('#')[0]}" for f in files for _, _, name in f.functions}
        scanned |= {f.relative for f in python_files}
        scanned |= {f"{f.relative}::{name.split('#')[0]}" for f in python_files for name in f.owners.values()}
        for missing in selected-scanned:
            findings.append(Finding(missing, 1, "<scope>", 0, "scope", "", set(),
                                    {"selected path/function is not a scanned source scope"}, 0))
    return findings


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--zero", nargs="+", metavar="PATH[::FUNCTION]",
                        help="require zero debt for exact files/functions; constants/includes are still expanded")
    parser.add_argument("--json", action="store_true", help="emit machine-readable audit records")
    args = parser.parse_args(argv)
    selected = set(args.zero) if args.zero else None
    findings = audit(args.root, selected)
    uncovered = new = debt = 0
    for item in findings:
        baseline = not selected and item.key in BASELINE
        if item.unresolved:
            status = "UNRESOLVED"; uncovered += 1
        elif item.issues and not baseline:
            status = "NEW_VIOLATION"; new += 1
        elif item.issues:
            status = "BASELINE_DEBT"; debt += 1
        else:
            status = "CHECKED"
        if args.json:
            print(json.dumps({**item.__dict__, "issues": sorted(item.issues),
                              "unresolved": sorted(item.unresolved), "status": status,
                              "baseline_key": item.key}, ensure_ascii=False))
        elif status != "CHECKED":
            details = "; ".join(sorted(item.issues | item.unresolved))
            print(f"{status} {item.path}:{item.line} {item.owner}/{item.ordinal}: {details}")
    if uncovered:
        print(f"INCOMPLETE: {uncovered} unresolved inputs; {new} new violations; {debt} baseline debts. NOT a pass.")
        return 2
    if new:
        print(f"FAIL: {new} new violations; {debt} baseline debts.")
        return 1
    if debt:
        print(f"BASELINE ONLY: {debt} remaining exact-site debts; no new violations. NOT JOIN-free.")
    else:
        print(f"PASS (bounded lint): {len(findings)} recognized SQL inputs resolved; zero JOIN/association debt in selected scope.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
