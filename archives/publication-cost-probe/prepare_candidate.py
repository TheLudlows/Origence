#!/usr/bin/env python3
"""Create a temporary, constants-only JSONB candidate from frozen 765d source.

The complete generated publication source belongs outside the product checkout.
No input source is edited and no candidate algorithm is selected dynamically.
"""
import argparse
import json
import re
from pathlib import Path

from extract_sql import CURRENT, CURRENT_HASH, constant, digest, read_source


def materialized_sql(original, path_count):
    suffix = ' FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?'
    if not original.startswith('SELECT ') or not original.endswith(suffix):
        raise ValueError('unexpected frozen projection shape')
    projection = original[:-len(suffix)]
    if projection.count('?') != path_count:
        raise ValueError('unexpected frozen path parameter count')
    parameters = iter(range(1, path_count + 1))
    projection = re.sub(r'\?', lambda _: '?' + str(next(parameters)), projection)
    prefix = (
        'WITH publication_payload AS MATERIALIZED '
        '(SELECT jsonb(payload) AS payload FROM oc_jobs '
        f'WHERE tenant_id=?{path_count + 1} AND workspace_id=?{path_count + 2} '
        f'AND id=?{path_count + 3}) '
    )
    return prefix + projection + ' FROM publication_payload'


def candidate_source(source):
    result = source
    for name, count in [('PUBLICATION_IDS', 8), ('PUBLICATION_DIMENSIONS', 16)]:
        original = constant(source, name)
        candidate = materialized_sql(original, count)
        pattern = r'const ' + name + r': &str = "((?:\\.|[^"\\])*)";'
        matches = list(re.finditer(pattern, result, re.S))
        if len(matches) != 1:
            raise ValueError('unexpected duplicate or missing frozen constant')
        match = matches[0]
        # Only the literal payload changes. No functions, header projection,
        # number of slots, or other constants are replaced.
        encoded = json.dumps(candidate, ensure_ascii=False)[1:-1]
        result = result[:match.start(1)] + encoded + result[match.end(1):]
    return result


def prepare(current_root, output):
    source_path, source = read_source(current_root, CURRENT_HASH)
    candidate = candidate_source(source)
    output = Path(output)
    output.mkdir()
    target = output / 'src/storage/sqlite/publication.rs'
    target.parent.mkdir(parents=True)
    target.write_text(candidate)
    evidence = {
        'kind': 'temporary benchmark-only constants candidate, not a product commit',
        'source_commit': CURRENT, 'source': str(source_path), 'source_sha256': CURRENT_HASH,
        'candidate_source': str(target), 'candidate_sha256': digest(target.read_bytes()),
        'generator_sha256': digest(Path(__file__).read_bytes()),
        'changed_constants': ['PUBLICATION_IDS', 'PUBLICATION_DIMENSIONS'],
    }
    (output / 'candidate-generation.json').write_text(json.dumps(evidence, indent=2) + '\n')
    return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--current-root', required=True)
    parser.add_argument('--output', required=True, help='new temporary candidate root')
    args = parser.parse_args()
    print(json.dumps(prepare(args.current_root, args.output)))


if __name__ == '__main__':
    main()
