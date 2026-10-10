"""Run the frozen S1 matrix with real local CPU models in one process tree.

Optional dependencies live outside the Rust application. Download only the pinned
snapshots named by the dataset manifest; retain provider-free compute usage and
unaltered generated projections separately from query quality results.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import re
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--dataset', default=str(Path(__file__).parent / 's1' / 'v2'))
    parser.add_argument('--model-dir', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--download-models', action='store_true')
    args = parser.parse_args()
    root = Path(args.dataset)
    manifest = json.loads((root / 'manifest.json').read_text(encoding='utf-8'))
    for name, digest in manifest['sha256'].items():
        if hashlib.sha256((root / name).read_bytes()).hexdigest() != digest:
            parser.error('frozen dataset hash mismatch')
    config = manifest['fixed_experiment']
    model_dir = Path(args.model_dir)
    paths = {}
    for kind in ['embedding', 'extraction']:
        paths[kind] = model_dir / config[kind].split('/')[-1]
        if args.download_models:
            from huggingface_hub import snapshot_download
            snapshot_download(config[kind], revision=config[kind + '_revision'], local_dir=paths[kind],
                              allow_patterns=['*.json', '*.txt', '*.safetensors', '*.model'])
    output = Path(args.output)
    if output.exists():
        parser.error('output already exists')
    server = None
    with tempfile.TemporaryDirectory(prefix='origence-model-') as directory:
        work = Path(directory)
        command = [sys.executable, str(Path(__file__).with_name('local_models.py')),
                   '--embedding-path', str(paths['embedding']), '--embedding-model', config['embedding'],
                   '--embedding-revision', config['embedding_revision'],
                   '--extraction-path', str(paths['extraction']), '--extraction-model', config['extraction'],
                   '--extraction-revision', config['extraction_revision'], '--port', '0',
                   '--cache', str(work / 'projections'), '--metadata', str(work / 'models.json')]
        try:
            server = subprocess.Popen(command, stdout=subprocess.PIPE, text=True)
            address = json.loads(server.stdout.readline())['listening']
            # Prepare real model projections offline before the application host
            # starts, keeping its normal 45-second request timeout unchanged.
            # All S1 sources fit one text chunk. Cache keys use the same exact
            # prompts as Models; cached responses remain unaltered model outputs.
            model_source = (Path(__file__).parent.parent / 'src' / 'models.rs').read_text(encoding='utf-8')
            prompts = {name: json.loads(re.search(r'pub const ' + name + r': &str = ("(?:[^"\\]|\\.)*");', model_source).group(1))
                       for name in ['SUMMARY_PROMPT','GRAPH_EXTRACTION_PROMPT']}
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            documents = [json.loads(line) for line in (root / 'corpus.jsonl').read_text(encoding='utf-8').splitlines()]
            for index, doc in enumerate(documents):
                for kind, prompt in prompts.items():
                    body = {'model':config['extraction'],'temperature':0,
                            'messages':[{'role':'system','content':prompt},{'role':'user','content':doc['content']}]}
                    if kind == 'GRAPH_EXTRACTION_PROMPT':
                        body['response_format'] = {'type':'json_object'}
                    request = urllib.request.Request('http://' + address + '/v1/chat/completions',
                                                     data=json.dumps(body).encode(),headers={'Content-Type':'application/json'})
                    with opener.open(request, timeout=300) as response:
                        value = json.load(response)['choices'][0]['message']['content']
                        if kind == 'GRAPH_EXTRACTION_PROMPT':
                            graph = json.loads(value)
                            names = {e['name'].strip().lower() for e in graph['entities']}
                            if any(r['source'].strip().lower() not in names or r['target'].strip().lower() not in names
                                   for r in graph['relations']):
                                raise ValueError('invalid generated graph endpoints')
                print(json.dumps({'prepared_source':doc['id'],'completed':index+1,'total':len(documents)}), flush=True)
            import os
            env = os.environ.copy()
            # An isolated local run must not inherit any remote model configuration.
            for name in list(env):
                if name.startswith('OC_'):
                    del env[name]
            env.update(OC_ENABLE_MODELS='true', OC_MODEL_BASE_URL='http://' + address + '/v1',
                       OC_EMBEDDING_MODEL=config['embedding'], OC_EMBEDDING_DIMENSION='512',
                       OC_EXTRACTION_MODEL=config['extraction'])
            evaluate = [sys.executable, str(Path(__file__).with_name('run.py')), '--binary', args.binary,
                        '--corpus', str(root / 'corpus.jsonl'), '--cases', str(root / 'cases.jsonl'),
                        '--matrix', '--allow-model-calls', '--require-evidence', '--k', str(config['k']),
                        '--budget', str(config['budget_utf8_bytes']), '--commit', args.commit,
                        '--model-metadata', str(work / 'models.json'), '--output', str(output)]
            evaluate.extend(['--usage-url', 'http://' + address + '/metadata'])
            result = subprocess.run(evaluate, env=env)
            if output.exists():
                shutil.copyfile(work / 'models.json', output / 'models.json')
                shutil.copytree(work / 'projections', output / 'generated-projections')
                shutil.copyfile(root / 'manifest.json', output / 'dataset-manifest.json')
                (output / 'preparation.json').write_text(json.dumps({'policy':'real model projections prepared offline and cached before import; generation time excluded from import latency',
                    'prompt_sha256':{k:hashlib.sha256(v.encode()).hexdigest() for k,v in prompts.items()}},indent=2),encoding='utf-8')
            raise SystemExit(result.returncode)
        finally:
            if server is not None:
                server.terminate()
                server.wait(timeout=30)


if __name__ == '__main__':
    main()
