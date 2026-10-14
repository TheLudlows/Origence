#!/usr/bin/env python3
"""Check the Cargo-generated isolated lock; never manufacture lock contents."""
import argparse
import json
import tomllib
from pathlib import Path


def verify(packages, metadata=None):
    # Validate every row before any display map can collapse duplicate names.
    native = [p for p in packages if p['name'].startswith('turso')]
    if not any(p['name'] == 'turso' for p in native):
        raise ValueError('missing native Turso SDK')
    if any(p['version'] != '0.8.2' for p in native):
        raise ValueError(f'unexpected native Turso version: {native}')
    if any(p['name'] == 'lancedb' or p['name'].startswith(('lance', 'arrow-')) for p in packages):
        raise ValueError('unexpected Lance/Arrow graph')
    features = None
    if metadata is not None:
        native_metadata = [p for p in metadata['packages'] if p['name'].startswith('turso')]
        if any(p['version'] != '0.8.2' for p in native_metadata):
            raise ValueError('unexpected Cargo metadata native package version')
        ids = {p['id'] for p in native_metadata if p['name'] == 'turso'}
        features = {n['id']: n['features'] for n in metadata['resolve']['nodes'] if n['id'] in ids}
        if not ids or set(features) != ids:
            raise ValueError('missing native Turso SDK resolve nodes')
        if any(set(v).intersection({'default', 'fts', 'sync', 'mimalloc'}) for v in features.values()):
            raise ValueError(f'unexpected native SDK features: {features}')
    return {'native_versions': [{'name': p['name'], 'version': p['version']} for p in native],
            'turso_features': features, 'status': 'PASS'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('lock')
    parser.add_argument('--metadata')
    args = parser.parse_args()
    packages = tomllib.loads(Path(args.lock).read_text())['package']
    metadata = json.loads(Path(args.metadata).read_text()) if args.metadata else None
    print(json.dumps(verify(packages, metadata)))


if __name__ == '__main__':
    main()
