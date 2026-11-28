"""Exercise Run → Skill via the real console and local Origence using synthetic data."""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import uuid

from native_fixture import workspace as seed_workspace
from mcp_smoke import Host, isolated_env, require, sanitized
from ui_trace_smoke import QUERY, publish_policy

BROWSER_CHECK = Path(__file__).resolve().parents[1] / "ui/scripts/check-run-skill-e2e.cjs"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    binary = parser.parse_args().binary.resolve()
    with tempfile.TemporaryDirectory(prefix="origence-run-skill-") as temporary:
        root = Path(temporary)
        env = isolated_env(temporary)
        workspace = seed_workspace(env, "Synthetic Skill workspace")
        unrelated = seed_workspace(env, "Unrelated Skill workspace")
        admin = workspace["token"]
        forbidden = [admin, unrelated["token"]]
        with Host(str(binary), env, admin, forbidden, serve_args=("--local",)) as host:
            keys = {}
            for name, role in (("writer", "writer"), ("reviewer", "writer"), ("reader", "reader")):
                keys[name] = host.request("/admin/keys", {
                    "workspace_id": workspace["workspace_id"], "role": role,
                })
                forbidden.append(keys[name]["token"])
            policy = publish_policy(host, 1)
            data = {"api": host.base, "workspace": workspace["workspace_id"], "keys": keys,
                    "admin": admin, "outsider": unrelated["token"], "query": QUERY,
                    "source": policy["source_event_id"], "run": str(uuid.uuid4())}
            result = subprocess.run(["node", str(BROWSER_CHECK)], input=json.dumps(data),
                                    capture_output=True, text=True, encoding="utf-8", timeout=90)
            sanitized(result.stdout + result.stderr, forbidden)
            require(result.returncode == 0, (result.stderr or result.stdout)[-4000:])
            print(result.stdout.strip(), flush=True)


if __name__ == "__main__":
    main()
