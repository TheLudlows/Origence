"""Run the installer against native Claude Code, the HTTP API and the built console."""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
from urllib.request import Request, urlopen
import uuid

from mcp_smoke import isolated_env, require, sanitized

REPO = Path(__file__).resolve().parents[1]
WINDOWS = os.name == "nt"
PLUGIN = "origence@origence-tools"
SETUP = REPO / "bin" / ("origence-setup.cmd" if WINDOWS else "origence-setup")
VARIABLES = ("OC_API_KEY", "OC_SERVER_URL", "OC_WORKSPACE_ID", "Path")


def port():
    with socket.socket() as reserved:
        reserved.bind(("127.0.0.1", 0))
        return reserved.getsockname()[1]


def request(url, *, payload=None, headers=None, method=None):
    headers = dict(headers or {})
    if payload is not None:
        headers["Content-Type"] = "application/json"
    body = None if payload is None else json.dumps(payload).encode()
    with urlopen(Request(url, data=body, headers=headers, method=method), timeout=15) as response:
        return (json.load(response) if response.status != 204 else None), response.headers


def registry_values():
    import winreg
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, "Environment") as key:
        values = {}
        for name in VARIABLES:
            try:
                values[name] = winreg.QueryValueEx(key, name)
            except FileNotFoundError:
                values[name] = None
    return values


def fresh_windows_environment():
    import ctypes
    from ctypes import wintypes
    kernel = ctypes.windll.kernel32
    advapi = ctypes.windll.advapi32
    userenv = ctypes.windll.userenv
    kernel.GetCurrentProcess.restype = wintypes.HANDLE
    token = wintypes.HANDLE()
    block = ctypes.c_void_p()
    require(advapi.OpenProcessToken(kernel.GetCurrentProcess(), 0x000A, ctypes.byref(token)),
            "Cannot open Windows test account token")
    try:
        require(userenv.CreateEnvironmentBlock(ctypes.byref(block), token, False),
                "Cannot load a new Windows user environment")
        try:
            variables = {}
            offset = block.value
            while entry := ctypes.wstring_at(offset):
                if not entry.startswith("="):
                    name, value = entry.split("=", 1)
                    variables[name.upper()] = value
                offset += len(entry.encode("utf-16-le")) + 2
            return variables
        finally:
            userenv.DestroyEnvironmentBlock(block)
    finally:
        kernel.CloseHandle(token)


def installer(env, commands, forbidden, expected=0, *, entry=None, cwd=REPO):
    entry = entry or ["node", str(REPO / "tools/install.mjs")]
    # On Windows the public launcher is a .cmd file. Its test arguments are fixture values.
    shell = WINDOWS and entry[0].lower().endswith(".cmd")
    child = subprocess.run([*entry, *commands], env=env, cwd=cwd, shell=shell,
                           creationflags=subprocess.CREATE_NO_WINDOW if WINDOWS else 0,
                           capture_output=True, text=True, encoding="utf-8", timeout=150)
    sanitized(child.stdout + child.stderr, forbidden)
    require(child.returncode == expected, "Unexpected installer result: " + child.stdout[-3000:])
    return {item["step"]: item for line in child.stdout.splitlines() if line
            for item in [json.loads(line)]}


def check_hook(binary, project, env, credentials):
    ui = credentials["ui"]
    profile, session = request(ui + "/api/console/session", headers={"Origin": ui}, payload={
        "username": credentials["username"], "password": credentials["password"],
    })
    workspace = credentials["workspace_id"]
    require(profile["session"]["workspaces"][0]["workspace_id"] == workspace,
            "The console login confirmed another workspace")
    headers = {"Cookie": session["Set-Cookie"].split(";")[0], "X-Origence-Workspace": workspace,
               "X-Origence-Session": profile["session"]["binding"]}
    before, _ = request(ui + "/api/v1/runs", headers=headers)
    project.mkdir()
    hook_env = dict(env, OC_API_KEY=credentials["token"], OC_SERVER_URL=credentials["endpoint"],
                    OC_WORKSPACE_ID=workspace, CLAUDE_PLUGIN_DATA=str(project.parent / "plugin data"))
    child = subprocess.run([str(binary), "plugin-hook", "--client", "claude-code"], cwd=project,
                           env=hook_env, input=json.dumps({"session_id": str(uuid.uuid4()),
                           "prompt_id": str(uuid.uuid4()), "cwd": str(project),
                           "hook_event_name": "UserPromptSubmit"}),
                           text=True, capture_output=True, timeout=20, check=True)
    sanitized(child.stdout + child.stderr, [credentials["token"], credentials["password"]])
    after, _ = request(ui + "/api/v1/runs", headers=headers)
    ids = {entry["run_id"] for entry in before["items"]}
    added = [entry for entry in after["items"] if entry["run_id"] not in ids]
    require(len(added) == 1, "The actual Claude Hook entry point must produce one visible private Run")
    detail, _ = request(ui + "/api/v1/runs/" + added[0]["run_id"], headers=headers)
    require(any(item["report"].get("event", {}).get("component") == "claude_code_hook" for item in detail["events"]),
            "The UI response did not contain the recorded Claude hook event")
    request(ui + "/api/console/session", headers=dict(headers, Origin=ui), method="DELETE")
    return added[0]["run_id"]


def check_browser(env, credentials, run, forbidden):
    info = {name: credentials[name] for name in ("ui", "username", "password")}
    child = subprocess.run(["node", str(REPO / "ui/scripts/check-installer-browser.cjs")],
                           env=env, input=json.dumps(dict(info, run=run)), capture_output=True,
                           encoding="utf-8", timeout=45)
    sanitized(child.stdout + child.stderr, forbidden)
    require(child.returncode == 0, child.stderr[-2000:])
    print(child.stdout.strip())


def check_existing_api(binary, claude, root, env, credentials, forbidden):
    profile = root / "existing-api profile"
    profile.mkdir()
    profile_env = dict(env, HOME=str(profile), USERPROFILE=str(profile), APPDATA=str(profile),
                       LOCALAPPDATA=str(profile), CLAUDE_CONFIG_DIR=str(profile / "claude config"))
    endpoint = credentials["endpoint"]
    ui_port = port()
    try:
        connected = installer(profile_env, ["--endpoint", endpoint, "--token", credentials["token"],
                              "--origence", str(binary), "--claude", str(claude), "--ui-port", str(ui_port)],
                              forbidden)
        require(connected["api"]["status"] == "skipped" and
                connected["whoami"]["detail"]["workspace_id"] == credentials["workspace_id"] and
                "bootstrap" not in connected,
                "Endpoint and token alone must bind a fresh install to existing data")
    finally:
        installer(profile_env, ["--uninstall"], forbidden)
    who, _ = request(endpoint + "/v1/whoami", headers={"Authorization": "Bearer " + credentials["token"]})
    require(who["workspace_id"] == credentials["workspace_id"], "Uninstall stopped or changed the external API")


def smoke(binary, claude, root, browser):
    env = isolated_env(str(root))
    env.pop("OC_DATA_DIR", None)
    env.update(HOME=str(root), USERPROFILE=str(root), LOCALAPPDATA=str(root), APPDATA=str(root),
               CLAUDE_CONFIG_DIR=str(root / "claude config"), OC_WORKSPACE_ID=str(uuid.uuid4()),
               CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1", DISABLE_TELEMETRY="1")
    env.pop("ZDOTDIR", None)
    state_dir = root / ("Origence" if WINDOWS else ".origence") / "installer"
    rc = root / ".bashrc"
    previous_shell = "export OC_API_KEY=prior-shell-value\n# unrelated edit"
    registry_before = registry_values() if WINDOWS else None
    if not WINDOWS:
        rc.write_text(previous_shell, encoding="utf-8")
    data = root / "instance data"
    output = root / "private credentials 验收.json"
    endpoint = f"http://127.0.0.1:{port()}"
    ui_port = port()
    while endpoint.endswith(f":{ui_port}"):
        ui_port = port()
    paths = ["--endpoint", endpoint, "--ui-port", str(ui_port), "--data-dir", str(data),
             "--origence", str(binary), "--claude", str(claude)]
    forbidden = []
    try:
        absent = installer(env, [*paths, "--origence", str(root / "missing" / "origence"), "--init", "Missing"],
                           [], expected=1)
        diagnosis = absent["installation"]["detail"]
        require(absent["installation"]["status"] == "failure" and
                diagnosis == "The requested Origence executable could not be found. Check --origence." and
                not data.exists(),
                "Missing Origence executable must be diagnosed. Installer returned: " + diagnosis)
        print("PASS missing Origence executable diagnosed before creating workspace data")
        invalid = "invalid-installer-fixture-key"
        unreachable = installer(env, [*paths, "--token", invalid], [invalid], expected=1)
        require("unreachable" in unreachable["installation"]["detail"] and not data.exists(),
                "An unreachable uninitialized API must not create a blank workspace")
        first = installer(env, ["install", *paths, "--init", "Installer synthetic", "--verify-write",
                                "--credentials-file", str(output)], forbidden, entry=[str(SETUP)], cwd=root)
        credentials = json.loads(output.read_text(encoding="utf-8"))
        forbidden = [credentials["token"], credentials["password"]]
        sanitized(first, forbidden)
        require(len(credentials["password"]) >= 15, "Console password is too short")
        require(first["http-mcp"]["detail"]["tools"] == 11 and first["claude-code"]["status"] == "success",
                "MCP tool catalog and the native client must connect")
        if not WINDOWS:
            require(output.stat().st_mode & 0o077 == 0, "Credentials file must be private")
            check = ('test "$OC_API_KEY" = "$EXPECTED" && test "$OC_WORKSPACE_ID" = "$WORKSPACE"'
                     ' && origence --version && origence-setup --help')
            subprocess.run(["bash", "--noprofile", "--rcfile", str(rc), "-ic", check], cwd=root,
                           env=dict(env, EXPECTED=credentials["token"], WORKSPACE=credentials["workspace_id"]),
                           capture_output=True, timeout=15, check=True)
            if shutil.which("zsh"):
                subprocess.run(["zsh", "-ic", check], cwd=root,
                               env=dict(env, EXPECTED=credentials["token"], WORKSPACE=credentials["workspace_id"]),
                               capture_output=True, timeout=15, check=True)
        else:
            actual = fresh_windows_environment()
            require(actual["OC_API_KEY"] == credentials["token"] and actual["OC_SERVER_URL"] == endpoint and
                    actual["OC_WORKSPACE_ID"] == credentials["workspace_id"],
                    "A fresh Windows user environment did not receive the credentials and workspace")
            origence = shutil.which("origence.exe", path=actual["PATH"])
            require(origence is not None and Path(origence).resolve() == binary.resolve(),
                    "A fresh Windows environment did not find the configured Origence binary")
            subprocess.run([origence, "--version"], cwd=root, env=actual, capture_output=True, timeout=15, check=True)
            setup = shutil.which("origence-setup.cmd", path=actual["PATH"])
            require(setup is not None and Path(setup).resolve() == SETUP, "Windows PATH is missing origence-setup")
        env_key = dict(env, OC_SERVER_URL=endpoint, OC_API_KEY=credentials["token"])
        second = installer(env_key, ["--from-env", "--verify-write"], forbidden)
        require(second["api"]["status"] == second["ui"]["status"] == "skipped" and
                second["write-check"]["detail"] == first["write-check"]["detail"],
                "Rerun did not reuse the listener, account and probe Run")
        failure = installer(dict(env_key, OC_API_KEY=invalid), ["--from-env", "--force"],
                            [*forbidden, invalid], expected=1)
        require("401" in failure["installation"]["detail"], "Rejected key must yield an actionable 401")
        hook_run = check_hook(binary, root / "synthetic project", env, credentials)
        if browser:
            check_browser(env, credentials, hook_run, forbidden)
        check_existing_api(binary, claude, root, env, credentials, forbidden)
        reader, _ = request(endpoint + "/admin/keys", headers={"Authorization": "Bearer " + credentials["token"]},
                            payload={"workspace_id": credentials["workspace_id"], "role": "reader"})
        forbidden.append(reader["token"])
        read_only = installer(dict(env_key, OC_API_KEY=reader["token"]),
                              ["--from-env", "--force", "--verify-write"], forbidden)
        require(read_only["whoami"]["detail"]["role"] == "reader" and
                read_only["write-check"]["status"] == "skipped" and read_only["api"]["status"] == "skipped",
                "An existing listener and a reader key must support login without a write")
        # Explicit replacement stores the old directory before creating any workspace.
        (data / "preserve-me").write_text("backup fixture", encoding="utf-8")
        replacement = installer(env, [*paths, "--force", "--init", "Other synthetic"], forbidden)
        backup = Path(replacement["backup"]["detail"])
        require((backup / "preserve-me").read_text(encoding="utf-8") == "backup fixture" and
                replacement["whoami"]["detail"]["workspace_id"] != credentials["workspace_id"],
                "Force plus init must preserve the original data and bind the new workspace")
        config = json.loads((state_dir / "state.json").read_text(encoding="utf-8"))["config"]
        forbidden.extend([config["token"], config["password"]])
        sanitized(replacement, forbidden)
        if not WINDOWS:
            with rc.open("a", encoding="utf-8") as handle:
                handle.write("\n# added after installation")
        if WINDOWS:
            removed = installer(dict(env, PATH=actual["PATH"]), ["uninstall"], forbidden, entry=[setup], cwd=root)
        else:
            removed = installer(env, [], forbidden, cwd=root,
                                entry=["bash", "--noprofile", "--rcfile", str(rc), "-ic", "origence-setup uninstall"])
        require(removed["uninstall"]["status"] == "success",
                "origence-setup must roll back from outside the repository")
        repeated = installer(env, ["uninstall"], forbidden, entry=[str(SETUP)], cwd=root)
        require(repeated["uninstall"]["status"] == "skipped", "Rerunning uninstall must be a no-op")
        require((data / "context.db").is_file() and (backup / "context.db").is_file(),
                "Uninstall must preserve both live and backed-up data")
        if WINDOWS:
            require(registry_values() == registry_before, "Uninstall must restore Windows user registry values and kinds")
        else:
            require(rc.read_text(encoding="utf-8") == previous_shell + "\n# added after installation",
                    "Uninstall must restore the old shell settings and preserve later edits")
        require(not (state_dir / "state.json").exists(), "Uninstall retained active installer settings")
        install_list = subprocess.run([str(claude), "plugin", "list", "--json"], cwd=root, env=env,
                                      capture_output=True, encoding="utf-8", timeout=30, check=True)
        require(not any(item.get("id") == PLUGIN for item in json.loads(install_list.stdout)),
                "Uninstall left the installer-owned plugin enabled")
        print("PASS standalone setup, native Claude MCP, UI login, Hook Run, idempotence, 401, backup, and uninstall rollback")
    finally:
        if (state_dir / "state.json").exists():
            installer(env, ["--uninstall"], forbidden)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--claude", type=Path, required=True)
    parser.add_argument("--browser", action="store_true", help="Also use Playwright Chromium to inspect the Hook Run.")
    parser.add_argument("--allow-windows-user-env", action="store_true",
                        help="Permit registry writes and restoration in a disposable Windows runner account.")
    args = parser.parse_args()
    if WINDOWS and not args.allow_windows_user_env:
        sys.exit("Run Windows acceptance in a disposable account with --allow-windows-user-env.")
    with tempfile.TemporaryDirectory(prefix="origence installer ") as directory:
        root = Path(directory)
        binary_dir = root / "Origence binaries"
        binary_dir.mkdir()
        binary = binary_dir / ("origence.exe" if WINDOWS else "origence")
        if WINDOWS:
            shutil.copy2(args.binary, binary)
        else:
            binary.symlink_to(args.binary.absolute())
        smoke(binary, args.claude.absolute(), root, args.browser)
