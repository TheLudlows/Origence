"""No-model release/container smoke test; uses an isolated data directory/volume."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary")
    parser.add_argument("--image")
    args = parser.parse_args()
    if bool(args.binary) == bool(args.image):
        parser.error("select exactly one of --binary or --image")
    name = "oc-smoke-" + uuid.uuid4().hex
    volume = name + "-data"
    process = None
    docker_started = False
    env = os.environ.copy()
    # This test must never use inherited model credentials or production data.
    for key in list(env):
        if key.startswith("OC_"):
            del env[key]
    env["OC_ENABLE_MODELS"] = "false"
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with tempfile.TemporaryDirectory(prefix=name) as directory:
        env["OC_DATA_DIR"] = directory
        with tempfile.TemporaryFile(mode="w+t") as log:
            try:
                if args.image:
                    prefix = ["docker", "run", "--rm", "-v", volume + ":/data",
                              "-e", "OC_ENABLE_MODELS=false", args.image]
                else:
                    prefix = [str(Path(args.binary).resolve())]
                bootstrap = subprocess.run(prefix + ["--offline", "workspace-create", "smoke"],
                                           env=env, capture_output=True, text=True, timeout=120)
                if bootstrap.returncode:
                    raise RuntimeError("workspace bootstrap failed")
                token = json.loads(bootstrap.stdout)["token"]
                upgrade = subprocess.run(prefix + ["--offline", "memory-identity-upgrade", "--dry-run"],
                                         env=env, capture_output=True, text=True, timeout=120)
                if upgrade.returncode or json.loads(upgrade.stdout)["status"] != "already_enabled":
                    raise RuntimeError("fresh database identity upgrade preflight failed")

                if args.image:
                    subprocess.run(["docker", "run", "-d", "--name", name,
                                    "-p", "127.0.0.1::8080", "-v", volume + ":/data",
                                    "-e", "OC_ENABLE_MODELS=false", args.image],
                                   check=True, capture_output=True, timeout=120)
                    docker_started = True
                    port = subprocess.check_output(
                        ["docker", "port", name, "8080/tcp"], text=True).strip().rsplit(":", 1)[1]
                else:
                    process = subprocess.Popen(prefix + ["serve", "--bind", "127.0.0.1:0"],
                                               env=env, stdout=log, stderr=log, text=True)
                    port = None
                deadline = time.monotonic() + 120
                while port is None and time.monotonic() < deadline:
                    if process.poll() is not None:
                        raise RuntimeError("host exited during startup")
                    log.seek(0)
                    for line in log.readlines():
                        try:
                            message = json.loads(line)
                        except json.JSONDecodeError:
                            continue
                        if "listening" in message:
                            port = message["listening"].rsplit(":", 1)[1]
                    time.sleep(0.2)
                if port is None:
                    raise RuntimeError("host startup timed out")
                base = "http://127.0.0.1:" + port

                def request(path, body=None, authenticated=True):
                    headers = {}
                    if authenticated:
                        headers["Authorization"] = "Bearer " + token
                    if body is not None:
                        headers["Content-Type"] = "application/json"
                        headers["Idempotency-Key"] = uuid.uuid4().hex
                    req = urllib.request.Request(base + path,
                                                 data=None if body is None else json.dumps(body).encode(),
                                                 headers=headers)
                    with opener.open(req, timeout=10) as response:
                        return json.load(response)

                while True:
                    try:
                        request("/health/ready", authenticated=False)
                        break
                    except (urllib.error.URLError, TimeoutError):
                        if time.monotonic() >= deadline:
                            raise RuntimeError("readiness timed out") from None
                        time.sleep(0.5)
                try:
                    request("/v1/whoami", authenticated=False)
                except urllib.error.HTTPError as error:
                    if error.code != 401:
                        raise RuntimeError("unexpected unauthenticated status") from None
                else:
                    raise RuntimeError("unauthenticated request was accepted")
                accepted = request("/v1/memories", {"fact_key": "smoke.policy", "content": "SMOKEPOLICY requires approval"})
                def published(accepted):
                    deadline = time.monotonic() + 120
                    while True:
                        job = request("/v1/jobs/" + accepted["job_id"])
                        if job["state"] == "completed":
                            if job["outcome"] != "published":
                                raise RuntimeError("job completed without publication")
                            return
                        if job["state"] in ("failed", "cancelled", "superseded"):
                            raise RuntimeError("publication failed")
                        if time.monotonic() >= deadline:
                            raise RuntimeError("publication timed out")
                        time.sleep(0.2)

                published(accepted)
                identity = {"subject": {"kind": "service", "stable_id": "billing"},
                            "predicate": "release.approval", "context": {"environment": "production"}}
                body = {"identity": identity, "content": "IDENTITYPOLICY requires approval"}
                first = request("/v1/memories/identified", body)
                published(first)
                body["content"] = "IDENTITYPOLICY requires two approvals"
                second = request("/v1/memories/identified", body)
                if first["asset_id"] != second["asset_id"]:
                    raise RuntimeError("same identity created another asset")
                published(second)
                view = request("/v1/assets/" + first["asset_id"])
                if view["version"] != 2 or view["identity"] != identity or view["content"] != body["content"]:
                    raise RuntimeError("identity version was not published correctly")
                identity["context"]["environment"] = "staging"
                third = request("/v1/memories/identified", body)
                if third["asset_id"] == first["asset_id"]:
                    raise RuntimeError("different environments share an asset")
                published(third)
                hits = request("/v1/search", {"query": "SMOKEPOLICY", "mode": "keyword"})["hits"]
                if not any(hit["asset_id"] == accepted["asset_id"] for hit in hits):
                    raise RuntimeError("published memory was not retrieved")
                print("PASS: readiness, authentication, memory publication, identity versioning/isolation and keyword retrieval")
            finally:
                if process is not None:
                    process.terminate()
                    try:
                        process.wait(timeout=30)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                if args.image:
                    if docker_started:
                        subprocess.run(["docker", "rm", "-f", name], capture_output=True, timeout=60)
                    subprocess.run(["docker", "volume", "rm", volume], capture_output=True, timeout=60)


if __name__ == "__main__":
    main()
