#!/usr/bin/env python3
"""Create an external harness workspace with a read-only path dependency.

The selected Origence checkout is never modified. Generate the real lock using
Cargo in the output workspace before --locked measurements.
"""
import argparse
import hashlib
import json
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--checkout', required=True)
p.add_argument('--label', required=True)
p.add_argument('--output', required=True)
args = p.parse_args()
checkout = Path(args.checkout).resolve()
assert (checkout / 'Cargo.toml').is_file()
output = Path(args.output)
output.mkdir()
(output / 'src').mkdir()
(output / 'src/main.rs').write_bytes(Path(__file__).with_name('main.rs').read_bytes())
manifest = '''[package]
name = "origence-publication-production-probe"
version = "0.1.0"
edition = "2024"
rust-version = "1.98.0"
publish = false
[workspace]
[dependencies]
anyhow = "=1.0.104"
serde_json = "=1.0.151"
tokio = { version = "=1.53.1", features = ["rt-multi-thread", "macros"] }
uuid = { version = "1", features = ["v4", "serde"] }
origence = { path = CHECKOUT, default-features = false }
[profile.release]
debug = 0
'''.replace('CHECKOUT', json.dumps(str(checkout)))
(output / 'Cargo.toml').write_text(manifest)
evidence = {'label': args.label, 'checkout': str(checkout), 'files': {}}
files = [checkout / 'Cargo.toml', checkout / 'Cargo.lock']
files.extend(sorted(p for p in (checkout / 'src').rglob('*') if p.is_file() and p.suffix in {'.rs', '.sql'}))
for path in files:
    evidence['files'][str(path.relative_to(checkout))] = hashlib.sha256(path.read_bytes()).hexdigest()
(output / 'source-evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
print(json.dumps({'workspace': str(output), 'source_evidence': evidence}))
