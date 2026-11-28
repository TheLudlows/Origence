"""Check source withdrawal, scope and resolve failure states on embedded AnySeek."""

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from urllib.error import HTTPError

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from mcp_smoke import Host, isolated_env
from native_fixture import workspace as seed_workspace


def lookup(host, mode):
    body = {"query": "玻璃鹿 授权", "mode": mode, "allow_partial": False,
            "components": {"summaries": False, "graph": False}}
    search = host.request("/v1/search", body | {"limit": 100})
    resolve = host.request("/v1/resolve", body | {"budget_tokens": 2000})
    return search["hits"], resolve["sources"]


def smoke(binary, model, directory):
    config = json.loads(Path(__file__).with_name("retrieval").joinpath("v2/manifest.json").read_bytes())
    embedding = config["fixed_experiment"]
    command = [
        sys.executable, str(Path(__file__).with_name("local_models.py")),
        "--embedding-path", str(model), "--embedding-model", embedding["embedding"],
        "--embedding-revision", embedding["embedding_revision"], "--port", "0",
        "--cache", str(directory / "cache"), "--metadata", str(directory / "model.json"),
    ]
    with subprocess.Popen(command, stdout=subprocess.PIPE, text=True) as provider:
        try:
            address = json.loads(provider.stdout.readline())["listening"]
            env = isolated_env(str(directory / "origence"))
            env.update(OC_ENABLE_MODELS="true", OC_MODEL_BASE_URL=f"http://{address}/v1",
                       OC_EMBEDDING_MODEL=embedding["embedding"], OC_EMBEDDING_DIMENSION="512")
            owner = seed_workspace(env, "scope-a")["token"]
            other = seed_workspace(env, "scope-b")["token"]
            with Host(binary, env, owner, [owner, other], serve_args=("--local",)) as host:
                modes = ("keyword", "vector", "hybrid")
                asset = host.publish("玻璃鹿 授权 审核规则要求双人确认。")
                for mode in modes:
                    hits, sources = lookup(host, mode)
                    assert any(hit["asset_id"] == asset and hit["version"] == 1 for hit in hits)
                    assert any(source["asset_id"] == asset and source["version"] == 1 for source in sources)
                event = next(hit["source_event_id"] for hit in hits if hit["asset_id"] == asset)
                host.token = other
                for mode in modes:
                    assert lookup(host, mode) == ([], [])
                host.token = owner
                host.request(f"/v1/events/{event}", {}, method="DELETE")
                for mode in modes:
                    assert lookup(host, mode) == ([], [])
                host.publish("玻璃鹿 授权 复核规则要求登记依据。")
                provider.terminate()
                provider.wait(timeout=15)
                for mode in ("vector", "hybrid"):
                    for path in ("/v1/search", "/v1/resolve"):
                        try:
                            host.request(path, {"query": "玻璃鹿 授权", "mode": mode, "allow_partial": False})
                        except HTTPError as error:
                            assert error.code == 503, error.code
                        else:
                            raise AssertionError("model failure returned successful evidence or abstention")
        finally:
            if provider.poll() is None:
                provider.terminate()
                provider.wait(timeout=15)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--embedding-path", required=True, type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="origence-retrieval-scope-") as directory:
        smoke(str(args.binary.resolve()), args.embedding_path.resolve(), Path(directory))
    print("PASS: three modes preserve workspace and source withdrawal; model outage returns HTTP 503")


if __name__ == "__main__":
    main()
