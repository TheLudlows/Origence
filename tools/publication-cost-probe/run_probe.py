#!/usr/bin/env python3
"""Run an already-built local benchmark with an external hard process deadline.

No compilation, network, production databases, or workflow dispatch occurs.
Complete stdout/stderr survive failure. A timeout is explicitly not a sample.
"""
import argparse
import hashlib
import json
import platform
import subprocess
import time
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary', required=True)
p.add_argument('--input', required=True, help='SQL evidence JSON for metadata, label for production')
p.add_argument('--production', action='store_true')
p.add_argument('--output', required=True)
p.add_argument('--warmups', type=int, default=2)
p.add_argument('--repeats', type=int, default=5)
p.add_argument('--deadline-seconds', type=int, default=1200)
args = p.parse_args()
assert 1 <= args.warmups <= 10 and 3 <= args.repeats <= 21
assert 1 <= args.deadline_seconds <= 3600
binary = Path(args.binary).resolve()
assert binary.is_file()
output = Path(args.output)
output.mkdir()
command = [str(binary), str(Path(args.input).resolve()) if not args.production else args.input,
           str((output / 'database').resolve()), str(args.warmups), str(args.repeats)]
manifest = {'engine': 'native Turso 0.8.2 (lock verification also required)', 'command': command,
            'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
            'host': platform.platform(), 'warmups': args.warmups, 'repeats': args.repeats,
            'deadline_seconds': args.deadline_seconds, 'production': args.production}
if not args.production:
    manifest['input_sha256'] = hashlib.sha256(Path(args.input).read_bytes()).hexdigest()
(output / 'run-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
started = time.perf_counter()
with (output / 'stdout.jsonl').open('wb') as out, (output / 'stderr.txt').open('wb') as err:
    try:
        completed = subprocess.run(command, stdout=out, stderr=err, timeout=args.deadline_seconds, check=False)
        status = 'PASS' if completed.returncode == 0 else 'FAIL'
        result = {'status': status, 'exit_code': completed.returncode}
    except subprocess.TimeoutExpired:
        result = {'status': 'TIMEOUT', 'remaining': 'NOT_RUN'}
if result['status'] == 'PASS':
    try:
        records = [json.loads(line) for line in (output / 'stdout.jsonl').read_text().splitlines() if line]
        kind = 'production_summary' if args.production else 'summary'
        summaries = [r for r in records if r.get('event') == kind]
        assert [r['chunks'] for r in summaries] == [50, 100, 200], 'incomplete summary matrix'
        for summary in summaries:
            assert summary['warmups'] == args.warmups and summary['repeats'] == args.repeats
            if args.production:
                assert len(summary['samples_ms']) == args.repeats
                assert summary['missing_full_text'] == summary['source_withdrawal'] == 'PASS'
            else:
                assert summary['equal_results'] is True
                assert all(len(v['samples_ms']) == args.repeats for v in summary['variants'])
        result['completed_counts'] = [50, 100, 200]
    except (ValueError, KeyError, AssertionError) as error:
        result = {'status': 'FAIL', 'validation_error': str(error), 'remaining': 'UNVERIFIED'}
result['wall_seconds'] = time.perf_counter() - started
(output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result))
raise SystemExit(0 if result['status'] == 'PASS' else 1)
