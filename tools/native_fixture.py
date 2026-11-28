"""Run the Rust SDK fixture for synthetic repository acceptance only."""

from functools import lru_cache
import json
import os
import secrets
import sys
from pathlib import Path
import subprocess


@lru_cache(maxsize=1)
def executable():
    repo = Path(__file__).resolve().parents[1]
    build = subprocess.run(
        ["cargo", "build", "--locked", "--example", "native_fixture", "--message-format=json"],
        cwd=repo, stdout=subprocess.PIPE, check=True, timeout=900,
        env={key: value for key, value in os.environ.items() if not key.upper().startswith("OC_")},
    )
    for line in build.stdout.splitlines():
        value = json.loads(line)
        if value.get("reason") == "compiler-artifact" and value.get("target", {}).get("name") == "native_fixture":
            if value.get("executable"):
                return value["executable"]
    raise RuntimeError("Native fixture executable is unavailable")


def run(env, fixture, **fields):
    return subprocess.run(
        [executable()], env=env, input=json.dumps({"fixture": fixture, **fields}).encode(),
        capture_output=True, timeout=120,
    )


def workspace(env, name, same_tenant_as=None):
    result = run(env, "workspace", name=name, same_tenant_as=same_tenant_as)
    if result.returncode:
        raise RuntimeError("Could not seed synthetic workspaces through the Rust storage API")
    return json.loads(result.stdout)


def admin_env(env):
    token = "oc_admin_" + secrets.token_hex(32)
    env["OC_DEPLOYMENT_ADMIN_KEY"] = token
    return token

if __name__ == "__main__":
    fields = json.load(sys.stdin)
    result = run(os.environ, fields.pop("fixture"), **fields)
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
    raise SystemExit(result.returncode)
