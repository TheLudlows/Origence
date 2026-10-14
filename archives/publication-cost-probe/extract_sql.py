#!/usr/bin/env python3
"""Extract immutable external SQL evidence; never alter either checkout.

Historical associations remain only in generated benchmark input. They must not
be copied into the product tree or added as an exception to its no-JOIN guard.
"""
import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

BASELINE = '1d5e0554b38e12df094a894f11a23fb26a117568'
CURRENT = '765d67190b02e01bcf78a756a5ae185b1027971c'
BASELINE_HASH = '573218c2db2702aa528c0dbc8e62e5d2c73b68d12f49710fbd91c19db51e88e9'
CURRENT_HASH = '42f18a0e46301322123e49c6a2306d41ec64c30627944d252af72ce0b2f451de'
# Updated only after independent review of the source-bound v1 runner.
RECEIPT_V1_HASH = '1d56bbfcbc28793a0f603b9ce881e792aae6687786fb15e2619b3638c6fc2053'
RECEIPT_MODE = 'reviewed_receipt_metadata_v1'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def rust_string(raw):
    # The selected literals contain only ordinary JSON-compatible escapes and
    # Rust's escaped line continuations. Unsupported syntax must fail closed.
    return json.loads('"' + re.sub(r'\\\n\s*', '', raw) + '"')


def literal_starting(source, prefix):
    found = [rust_string(m.group(1)) for m in re.finditer(r'"((?:\\.|[^"\\])*)"', source, re.S)
             if m.group(1).startswith(prefix)]
    if len(found) != 1:
        raise ValueError(f'expected one {prefix!r} SQL literal, got {len(found)}')
    return found[0]


def constant(source, name):
    match = re.search(r'const ' + re.escape(name) + r': &str = "((?:\\.|[^"\\])*)";', source, re.S)
    if not match:
        raise ValueError(f'missing fixed literal {name}')
    return rust_string(match.group(1))


def read_source(root, expected_hash):
    path = Path(root) / 'src/storage/sqlite/publication.rs'
    raw = path.read_bytes()
    if digest(raw) != expected_hash:
        raise ValueError(f'source hash mismatch: {path}; refusing an unreviewed algorithm')
    return path, raw.decode()


def section(source, first, last):
    if source.count(first) != 1 or source.count(last) != 1:
        raise ValueError('reviewed source boundary changed')
    return source[source.index(first):source.index(last)]


def receipt_variant(path, source, commit, source_hash):
    if source_hash != RECEIPT_V1_HASH or digest(source.encode()) != RECEIPT_V1_HASH:
        raise ValueError('unreviewed receipt_metadata source; independent review required')
    if not re.fullmatch('[0-9a-f]{40}', commit):
        raise ValueError('receipt candidate requires exact source commit')
    flow = {
        'typed_metadata': section(source, 'const RECEIPT_METADATA_FORMAT:', '/// Project metadata'),
        'status': section(source, '    pub async fn message_batch_status(', 'const PUBLICATION_SLOTS:'),
        'distinct': section(source, '    async fn publication_distinct_ids(', '    async fn publication_expected_chunks('),
        'expected': section(source, '    async fn publication_expected_chunks(', '    /// Message receipt membership'),
        'save': section(source, '    pub(super) async fn save_message_publication_inner(', '    async fn derive_receipt_metadata('),
        'derive': section(source, '    async fn derive_receipt_metadata(', '    async fn publication_visible_count('),
    }
    copied = Path(__file__).with_name('src') / 'receipt_types.rs'
    if copied.read_text() != flow['typed_metadata']:
        raise ValueError('typed receipt copy differs from exact reviewed product source')
    variant = {
        'label': 'receipt_metadata_v1', 'commit': commit, 'source': str(path),
        'source_sha256': source_hash, 'mode': RECEIPT_MODE,
        'flow_sha256': {key: digest(value.encode()) for key, value in flow.items()},
        'header_sql': literal_starting(flow['status'], 'SELECT operation,state,generation,asset_id,source_event_id,'),
        'validate_tokens_sql': literal_starting(flow['distinct'], 'SELECT count(*) FROM json_each(?) WHERE'),
        'distinct_sql': literal_starting(flow['distinct'], 'SELECT count(DISTINCT value)'),
        'dimensions_sql': literal_starting(flow['expected'], 'SELECT key,upper('),
        'saved_sql': literal_starting(flow['save'], 'SELECT json_extract(payload,'),
        'save_sql': literal_starting(flow['save'], 'UPDATE oc_jobs SET payload='),
        'derive_kind_sql': literal_starting(flow['derive'], 'SELECT json_type(?1,'),
        'derive_chunks_sql': literal_starting(flow['derive'], 'SELECT json_quote(json_extract(value,'),
    }
    return variant


def verify_candidate_commit(root, commit, source_hash):
    # Bind the source bytes to the actual committed checkout, not just a label.
    actual = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
    if actual != commit:
        raise ValueError('candidate checkout commit differs from requested source commit')
    raw = subprocess.check_output(['git', '-C', str(root), 'show', commit + ':src/storage/sqlite/publication.rs'])
    if digest(raw) != source_hash:
        raise ValueError('candidate source differs from committed source')


def extract(baseline_root, current_root, candidate_root=None, candidate_commit=None, candidate_hash=None, candidate_mode="constants_only"):
    bp, before = read_source(baseline_root, BASELINE_HASH)
    cp, current = read_source(current_root, CURRENT_HASH)
    old_header = literal_starting(before, 'SELECT operation,state,generation,asset_id,source_event_id,')
    old_visibility = literal_starting(before, 'WITH expected AS MATERIALIZED ( ')
    marker = ') SELECT count(*) '
    if old_visibility.count(marker) != 1:
        raise ValueError('historical CTE boundary changed')
    # Exact original expected CTE, changing ONLY its terminal consumer. The
    # separate production fixture measures actual message_batch_status.
    old_expected = old_visibility.split(marker, 1)[0] + ') SELECT id,dimension FROM expected'
    header = literal_starting(current, 'SELECT operation,state,generation,asset_id,source_event_id,')
    ids = constant(current, 'PUBLICATION_IDS')
    dims = constant(current, 'PUBLICATION_DIMENSIONS')
    assert re.search(r'const PUBLICATION_SLOTS: usize = 8;', current)
    assert old_header.count('?') == old_expected.count('?') == header.count('?') == 3
    assert ids.count('?') == 11 and dims.count('?') == 19
    assert '.step_by(PUBLICATION_SLOTS)' in current
    assert 'SELECT count(DISTINCT value) FROM json_each(?)' in current
    # Label what was extracted rather than suggesting this metadata benchmark
    # executes authorization, source/readiness association, or the adapter.
    variants = [
        {'label': 'historical_main', 'commit': BASELINE, 'source': str(bp),
         'source_sha256': BASELINE_HASH, 'mode': 'historical_metadata',
         'header_sql': old_header, 'dimensions_sql': old_expected,
         'terminal_consumer_change': 'Original expected CTE; terminal visible-count body replaced with SELECT id,dimension FROM expected'},
        {'label': 'current_765d', 'commit': CURRENT, 'source': str(cp),
         'source_sha256': CURRENT_HASH, 'mode': 'eight_slots', 'slots': 8,
         'header_sql': header, 'ids_sql': ids, 'dimensions_sql': dims,
         'distinct_sql': 'SELECT count(DISTINCT value) FROM json_each(?)'},
    ]
    if candidate_root is not None:
        if not candidate_commit or not candidate_hash:
            raise ValueError('candidate needs a frozen commit and reviewed source hash')
        path, candidate = read_source(candidate_root, candidate_hash)
        if candidate_mode == RECEIPT_MODE:
            variants.append(receipt_variant(path, candidate, candidate_commit, candidate_hash))
            verify_candidate_commit(candidate_root, candidate_commit, candidate_hash)
        elif candidate_mode == 'constants_only':
            # A constants-only candidate can reuse this runner. A different Rust
            # algorithm needs a separately reviewed native runner, not a false label.
            for first, last in [('publication_distinct_ids', 'publication_expected_chunks'),
                                ('publication_expected_chunks', 'publication_visible_count')]:
                start = '    async fn ' + first
                end = '    async fn ' + last
                original_flow = current[current.index(start):current.index(end)]
                candidate_flow = candidate[candidate.index(start):candidate.index(end)]
                if original_flow != candidate_flow:
                    raise ValueError('candidate Rust flow differs; add/review its runner first')
            candidate_ids = constant(candidate, 'PUBLICATION_IDS')
            candidate_dims = constant(candidate, 'PUBLICATION_DIMENSIONS')
            if candidate_ids.count('?') != 11 or candidate_dims.count('?') != 19:
                raise ValueError('candidate parameter contract differs')
            variants.append({'label': 'reviewed_candidate', 'commit': candidate_commit,
                             'source': str(path), 'source_sha256': candidate_hash,
                             'mode': 'eight_slots', 'slots': 8,
                             'header_sql': literal_starting(candidate, 'SELECT operation,state,generation,asset_id,source_event_id,'),
                             'ids_sql': candidate_ids, 'dimensions_sql': candidate_dims,
                             'distinct_sql': 'SELECT count(DISTINCT value) FROM json_each(?)'})
        else:
            raise ValueError('unknown candidate mode')
    for variant in variants:
        variant['sql_sha256'] = {k: digest(v.encode()) for k, v in variant.items() if k.endswith('_sql')}
    return {'schema_version': 1, 'scope': 'metadata only, not complete receipt latency',
            'counts': [50, 100, 200], 'dimension': 4096, 'variants': variants}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--baseline-root', required=True)
    p.add_argument('--current-root', required=True)
    p.add_argument('--output', required=True)
    p.add_argument('--candidate-root')
    p.add_argument('--candidate-commit')
    p.add_argument('--candidate-sha256')
    p.add_argument('--candidate-mode', choices=['constants_only', RECEIPT_MODE], default='constants_only')
    args = p.parse_args()
    output = Path(args.output)
    bundle = extract(args.baseline_root, args.current_root, args.candidate_root, args.candidate_commit, args.candidate_sha256, args.candidate_mode)
    with output.open('x', encoding='utf-8') as stream:
        json.dump(bundle, stream, indent=2)
        stream.write('\n')
    print(json.dumps({'output': str(output), 'sha256': digest(output.read_bytes()),
                      'variants': [v['label'] for v in bundle['variants']]}))


if __name__ == '__main__':
    main()
