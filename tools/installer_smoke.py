"""Run the installer against native Claude Code, the HTTP API and the built console."""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
from urllib.request import Request, urlopen
import uuid

from mcp_smoke import fixture_directory, isolated_env, require, sanitized
from plugin_smoke import codex_session, tool_value

REPO = Path(__file__).resolve().parents[1]
WINDOWS = os.name == "nt"
PLUGIN = "origence@origence-tools"
SETUP = REPO / "bin" / ("origence-setup.cmd" if WINDOWS else "origence-setup")
DEPLOY = REPO / "bin" / ("origence-deploy.cmd" if WINDOWS else "origence-deploy")
VARIABLES = ("OC_API_KEY", "OC_SERVER_URL", "OC_WORKSPACE_ID", "OC_TURN_SYNC_PROJECT", "Path")


def port(preferred=0):
    with socket.socket() as reserved:
        try:
            reserved.bind(("127.0.0.1", preferred))
        except OSError:
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


def set_registry_workspace_key(entry):
    import winreg
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, "Environment") as key:
        if entry is not None:
            winreg.SetValueEx(key, "OC_API_KEY", 0, entry[1], entry[0])
        else:
            try:
                winreg.DeleteValue(key, "OC_API_KEY")
            except FileNotFoundError:
                pass


def fresh_windows_environment():
    import ctypes
    from ctypes import wintypes
    kernel = ctypes.windll.kernel32
    advapi = ctypes.windll.advapi32
    userenv = ctypes.windll.userenv
    kernel.GetCurrentProcess.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
    advapi.OpenProcessToken.argtypes = (wintypes.HANDLE, wintypes.DWORD, ctypes.POINTER(wintypes.HANDLE))
    userenv.CreateEnvironmentBlock.argtypes = (ctypes.POINTER(ctypes.c_void_p), wintypes.HANDLE, wintypes.BOOL)
    userenv.DestroyEnvironmentBlock.argtypes = (ctypes.c_void_p,)
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


def check_hook(binary, project, env, credentials, archive_project):
    ui = credentials["ui"]
    profile, session = request(ui + "/api/console/session", headers={"Origin": ui}, payload={
        "workspace_key": credentials["token"],
    })
    workspace = credentials["workspace_id"]
    require(profile["session"]["workspaces"][0]["workspace_id"] == workspace,
            "The console login confirmed another workspace")
    headers = {"Cookie": session["Set-Cookie"].split(";")[0], "X-Origence-Workspace": workspace,
               "X-Origence-Session": profile["session"]["binding"]}
    before, _ = request(ui + "/api/v1/runs", headers=headers)
    hook_env = dict(env, OC_API_KEY=credentials["token"], OC_SERVER_URL=credentials["endpoint"],
                    OC_WORKSPACE_ID=workspace, OC_TURN_SYNC_PROJECT=archive_project,
                    CLAUDE_PLUGIN_DATA=str(Path(archive_project).parent / "plugin data"))
    event = {"session_id": str(uuid.uuid4()), "prompt_id": str(uuid.uuid4()), "cwd": str(project)}
    question, answer = "Installer synthetic question", "Installer synthetic answer"
    for payload in (dict(event, hook_event_name="UserPromptSubmit", prompt=question),
                    dict(event, hook_event_name="Stop", last_assistant_message=answer)):
        child = subprocess.run([str(binary), "plugin-hook", "--client", "claude-code"], cwd=project,
                               env=hook_env, input=json.dumps(payload), text=True, capture_output=True,
                               timeout=20, check=True)
        sanitized(child.stdout + child.stderr, [credentials["token"], credentials["deployment_admin_key"]])
    after, _ = request(ui + "/api/v1/runs", headers=headers)
    ids = {entry["run_id"] for entry in before["items"]}
    added = [entry for entry in after["items"] if entry["run_id"] not in ids]
    require(len(added) == 1, "The actual Claude Hook entry point must produce one visible private Run")
    detail, _ = request(ui + "/api/v1/runs/" + added[0]["run_id"], headers=headers)
    require(any(item["report"].get("event", {}).get("component") == "claude_code_hook" for item in detail["events"]),
            "The UI response did not contain the recorded Claude hook event")
    request(ui + "/api/console/session", headers=dict(headers, Origin=ui), method="DELETE")
    api = credentials["endpoint"]
    auth = {"Authorization": "Bearer " + credentials["token"], "X-Origence-Workspace": workspace}
    session, _ = request(api + "/v1/session", headers=auth)
    page, _ = request(api + "/v1/sessions/" + session["session_id"] + "/turns", headers=auth)
    require(len(page["turns"]) == 1, "Setup must archive a completed private turn inside the configured project")
    turn = page["turns"][0]
    require(turn["question"] == question and turn["answer"] == answer, "The archived turn did not round-trip")
    return added[0]["run_id"]


def check_browser(env, credentials, run, forbidden):
    info = {name: credentials[name] for name in ("ui", "token")}
    child = subprocess.run(["node", str(REPO / "ui/scripts/check-installer-browser.cjs")],
                           env=env, input=json.dumps(dict(info, run=run)), capture_output=True,
                           encoding="utf-8", timeout=45)
    sanitized(child.stdout + child.stderr, forbidden)
    require(child.returncode == 0, child.stderr[-2000:])
    print(child.stdout.strip())


def check_codex(codex, project, env, credentials, forbidden):
    project.mkdir()
    with codex_session(str(codex), project, env, forbidden) as (rpc, thread):
        entries = rpc("mcpServerStatus/list", {"threadId": thread})["data"]
        require(sum(item["name"] == "origence" for item in entries) == 1,
                "Codex must load exactly one Origence MCP endpoint")
        result = tool_value(rpc("mcpServer/tool/call", {
            "server": "origence", "tool": "context_session", "threadId": thread, "arguments": {},
        }))
        require(result["workspace_id"] == credentials["workspace_id"], "Codex resolved the wrong workspace")


def smoke(binary, claude, codex, root, browser):
    env = isolated_env(str(root))
    env.pop("OC_DATA_DIR", None)
    env.update(HOME=str(root), USERPROFILE=str(root), LOCALAPPDATA=str(root), APPDATA=str(root),
               CLAUDE_CONFIG_DIR=str(root / "claude config"), CODEX_HOME=str(root / "codex config"),
               OC_WORKSPACE_ID=str(uuid.uuid4()), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1",
               DISABLE_TELEMETRY="1")
    Path(env["CODEX_HOME"]).mkdir()
    env.pop("ZDOTDIR", None)
    state = root / ("Origence" if WINDOWS else ".origence") / "installer" / "state.json"
    env["ORIGENCE_UI_ACCOUNTS_DB"] = str(root / "unused-console-accounts.db")
    rc = root / ".bashrc"
    previous_shell = "export OC_API_KEY=prior-shell-value\n# unrelated edit"
    registry_before = registry_values() if WINDOWS else None
    if not WINDOWS:
        rc.write_text(previous_shell, encoding="utf-8")
    data = root / "instance data"
    output = root / "private credentials 验收.json"
    ui_port = port(3000)
    endpoint = f"http://127.0.0.1:{port()}"
    while endpoint.endswith(f":{ui_port}"):
        endpoint = f"http://127.0.0.1:{port()}"
    deploy_args = ["--endpoint", endpoint, "--ui-port", str(ui_port), "--data-dir", str(data),
                   "--origence", str(binary)]
    setup_args = ["--agent", "both", "--origence", str(binary), "--claude", str(claude), "--codex", str(codex)]
    deploy_cmd = [str(DEPLOY)]
    setup_cmd = [str(SETUP)]
    project = root / "Claude synthetic project"
    (project / ".git").mkdir(parents=True)
    (project / ".git" / "HEAD").write_text("ref: refs/heads/main\n", encoding="utf-8")
    subdir = project / "subdir"
    subdir.mkdir()
    forbidden = []
    try:
        alone = installer(env, ["uninstall"], [], entry=setup_cmd, cwd=root)
        require(alone["uninstall"]["status"] == "skipped", "Empty setup must be a no-op")
        absent = installer(env, ["install", *deploy_args, "--origence", str(root / "missing" / "origence"),
                                 "--init", "Missing"], [], expected=1, entry=deploy_cmd, cwd=root)
        require("requested Origence executable could not be found" in absent["installation"]["detail"]
                and not data.exists(), "Missing Origence executable must be diagnosed before creating a workspace")
        invalid = "invalid-installer-fixture-key"
        unreachable = installer(dict(env, OC_SERVER_URL=endpoint, OC_API_KEY=invalid),
                                ["install", "--from-env", *setup_args], [invalid],
                                expected=1, entry=setup_cmd, cwd=root)
        require("could not reach" in unreachable["installation"]["detail"] and not data.exists() and not state.exists(),
                "Agent setup against an unreachable API must not deploy a service or save state")
        first = installer(env, ["install", *deploy_args, "--init", "Installer synthetic", "--verify-write",
                                "--credentials-file", str(output)], forbidden, entry=deploy_cmd, cwd=root)
        credentials = json.loads(output.read_text(encoding="utf-8"))
        forbidden = [credentials["token"], credentials["deployment_admin_key"]]
        sanitized(first, forbidden)
        require(credentials["login_mode"] == "workspace" and credentials["role"] == "workspace"
                and "password" not in credentials and first["console-session"]["status"] == "success"
                and first["deployment-admin-session"]["status"] == "success",
                "Fresh deployment must provision the workspace and deployment administrator Keys")
        require(not (state.parent / "console-accounts.db").exists()
                and not Path(env["ORIGENCE_UI_ACCOUNTS_DB"]).exists(),
                "A fresh deployment must not create or require a console account database")
        require("claude-code" not in first and "codex" not in first and "agent" not in json.loads(state.read_text(encoding="utf-8")),
                "Deployment must not configure either agent")
        if WINDOWS:
            require(registry_values() == registry_before, "Deployment must leave the registry environment alone")
        else:
            require(output.stat().st_mode & 0o077 == 0 and rc.read_text(encoding="utf-8") == previous_shell,
                    "Deployment must protect exported credentials and leave shell config alone")
        env_key = dict(env, OC_SERVER_URL=endpoint, OC_API_KEY=credentials["token"])
        deployed = json.loads(state.read_text(encoding="utf-8"))["deploy"]
        second = installer(env_key, ["install", "--from-env", "--verify-write"], forbidden, entry=deploy_cmd, cwd=root)
        require(second["api"]["status"] == second["ui"]["status"] == "skipped" and
                second["write-check"]["detail"] == first["write-check"]["detail"],
                "Deployment rerun must reuse its API, console and probe")
        failure = installer(dict(env_key, OC_API_KEY=invalid), ["install", "--from-env", *setup_args],
                            [*forbidden, invalid], expected=1, entry=setup_cmd, cwd=root)
        require("401" in failure["installation"]["detail"], "Rejected key must yield an actionable 401")

        # The console is already listening. Agent setup only needs the API.
        claude_only = installer(env_key, ["install", "--from-env", "--agent", "claude", "--origence", str(binary),
                                         "--claude", str(claude)], forbidden, entry=setup_cmd, cwd=subdir)
        require("claude-code" in claude_only and "codex" not in claude_only,
                "Selecting Claude must leave Codex untouched")
        require(claude_only["installation"]["detail"]["private_turn_sync"] is True,
                "Setup with a writable Key must enable project archiving")
        split = json.loads(state.read_text(encoding="utf-8"))
        if WINDOWS:
            sanitized(split["agent"], forbidden)
        legacy = {"version": 1, **split["deploy"],
                  **{name: split["agent"][name] for name in ("claude", "shell", "registry") if name in split["agent"]}}
        rerun_env, source_flags = env_key, ["--from-env"]
        if WINDOWS:
            previous = "prior-synthetic-workspace-key"
            legacy["registry"]["OC_API_KEY"] = {
                "before": {"value": previous, "kind": "String"},
                "installed": {"value": credentials["token"], "kind": "String"},
            }
            rerun_env, source_flags = dict(env, OC_API_KEY=invalid), []
            forbidden = [*forbidden, previous, invalid]
        state.write_text(json.dumps(legacy), encoding="utf-8")
        connected = installer(rerun_env, ["install", *source_flags, *setup_args, "--turn-sync-project", str(subdir)],
                              forbidden, entry=setup_cmd, cwd=root)
        require(connected["http-mcp"]["status"] == "success" and connected["claude-code"]["status"] == "success"
                and connected["codex"]["status"] == "success" and "ui" not in connected and "api" not in connected,
                "Agent setup must connect both clients without starting a UI or an API")
        replay = json.loads(state.read_text(encoding="utf-8"))
        require(replay["version"] == 2 and replay["deploy"] == deployed,
                "Agent setup must migrate a legacy journal without changing deployment settings")
        if WINDOWS:
            sanitized(replay["agent"], forbidden)
        installer(rerun_env, ["install", *source_flags, "--agent", "both"], forbidden, entry=setup_cmd, cwd=root)
        require(json.loads(state.read_text(encoding="utf-8")) == replay,
                "Agent setup rerun must reuse the same settings and ownership")
        actual = None
        if not WINDOWS:
            check = ('test "$OC_API_KEY" = "$EXPECTED" && test "$OC_WORKSPACE_ID" = "$WORKSPACE"'
                     ' && test "$OC_TURN_SYNC_PROJECT" = "$PROJECT"'
                     ' && { origence --version && origence-setup --help && origence-deploy --help; } >/dev/null'
                     ' && printf "%s" "$OC_TURN_SYNC_PROJECT"')
            shell_env = dict(env, EXPECTED=credentials["token"], WORKSPACE=credentials["workspace_id"],
                             PROJECT=str(project.resolve()))
            inherited = subprocess.run(["bash", "--noprofile", "--rcfile", str(rc), "-ic", check], cwd=root,
                                       env=shell_env, capture_output=True, timeout=15, check=True, encoding="utf-8").stdout
            if shutil.which("zsh"):
                subprocess.run(["zsh", "-ic", check], cwd=root, env=shell_env,
                               capture_output=True, timeout=15, check=True)
        else:
            actual = fresh_windows_environment()
            inherited = actual["OC_TURN_SYNC_PROJECT"]
            require(actual["OC_API_KEY"] == credentials["token"] and actual["OC_SERVER_URL"] == endpoint and
                    actual["OC_WORKSPACE_ID"] == credentials["workspace_id"], "New Windows sessions must receive settings")
            origence = shutil.which("origence.exe", path=actual["PATH"])
            require(origence is not None and Path(origence).resolve() == binary.resolve(), "Windows PATH is missing Origence")
            for launch in (SETUP, DEPLOY):
                found = shutil.which(launch.name, path=actual["PATH"])
                require(found is not None and Path(found).resolve() == launch, "Windows PATH is missing " + launch.name)
        scoped = dict(env_key, OC_WORKSPACE_ID=credentials["workspace_id"])
        require(Path(inherited).resolve() == project.resolve(), "New sessions must inherit the setup project's Git root")
        check_codex(codex, root / "Codex synthetic project", scoped, credentials, forbidden)
        hook_run = check_hook(binary, subdir, env, credentials, inherited)
        if browser:
            check_browser(env, credentials, hook_run, forbidden)

        if WINDOWS:
            # The Windows runner obtains its Node version on the job PATH.
            node = Path(shutil.which("node.exe", path=env["PATH"])).parent
            launch_path = os.pathsep.join((str(node), actual["PATH"]))
            installer(dict(env, PATH=launch_path), ["uninstall"], forbidden, entry=setup_cmd, cwd=root)
            require(registry_values() == dict(registry_before, OC_API_KEY=None),
                    "Setup uninstall must clear its workspace Key and restore other user settings")
        else:
            with rc.open("a", encoding="utf-8") as handle:
                handle.write("\n# added after agent setup")
            installer(env, [], forbidden, cwd=root,
                      entry=["bash", "--noprofile", "--rcfile", str(rc), "-ic", "origence-setup uninstall"])
            require(rc.read_text(encoding="utf-8") == previous_shell + "\n# added after agent setup",
                    "Setup uninstall must restore shell settings and preserve later edits")
        require(json.loads(state.read_text(encoding="utf-8"))["deploy"] == deployed,
                "Setup uninstall altered deployment ownership or settings")
        identity, _ = request(endpoint + "/v1/whoami", headers={"Authorization": "Bearer " + credentials["token"]})
        require(identity["workspace_id"] == credentials["workspace_id"], "Setup uninstall stopped the deployed API")
        with urlopen(credentials["ui"] + "/", timeout=10) as response:
            require(response.status == 200, "Setup uninstall stopped the deployed console")
        for cli in (claude, codex):
            inventory = subprocess.run([str(cli), "plugin", "list", "--json"], cwd=root, env=env,
                                       capture_output=True, encoding="utf-8", timeout=30, check=True)
            sanitized(inventory.stdout + inventory.stderr, forbidden)
            require(PLUGIN not in inventory.stdout, "Agent uninstall left an owned plugin installed")
        inventory = subprocess.run([str(codex), "mcp", "list", "--disable", "plugins", "--json"], cwd=root, env=env,
                                   capture_output=True, encoding="utf-8", timeout=30, check=True)
        require(not any(item["name"] == "origence" for item in json.loads(inventory.stdout)),
                "Agent uninstall left its owned Codex override")
        repeat = installer(env, ["uninstall"], forbidden, entry=setup_cmd, cwd=root)
        require(repeat["uninstall"]["status"] == "skipped", "Rerunning Agent uninstall must skip")

        (data / "preserve-me").write_text("backup fixture", encoding="utf-8")
        replacement = installer(env, ["install", *deploy_args, "--force", "--init", "Other synthetic"],
                                forbidden, entry=deploy_cmd, cwd=root)
        backup = Path(replacement["backup"]["detail"])
        require((backup / "preserve-me").read_text(encoding="utf-8") == "backup fixture" and
                replacement["whoami"]["detail"]["workspace_id"] != credentials["workspace_id"],
                "Force plus init must preserve the original data and bind the new workspace")
        config = json.loads(state.read_text(encoding="utf-8"))["deploy"]["config"]
        forbidden.append(config["token"])
        sanitized(replacement, forbidden)
        new_env = dict(env, OC_SERVER_URL=endpoint, OC_API_KEY=config["token"])
        if WINDOWS:
            import winreg
            adopted_key = (config["token"], winreg.REG_SZ)
            set_registry_workspace_key(adopted_key)
        paused = installer(new_env, ["install", "--from-env", *setup_args, "--no-turn-sync"],
                           forbidden, entry=setup_cmd, cwd=root)
        require(paused["installation"]["detail"]["private_turn_sync"] is False,
                "Setup must allow private turn archiving to be paused")
        session, _ = request(endpoint + "/v1/session", headers={"Authorization": "Bearer " + config["token"]})
        agent = json.loads(state.read_text(encoding="utf-8"))["agent"]
        installer(env, ["uninstall"], forbidden, entry=deploy_cmd, cwd=root)
        remaining = json.loads(state.read_text(encoding="utf-8"))
        require("deploy" not in remaining and remaining["agent"] == agent,
                "Deployment uninstall must leave Agent settings and ownership alone")
        require((data / "context.db").is_file() and (backup / "context.db").is_file(),
                "Deployment uninstall must preserve both live and backed-up data")
        rotated = subprocess.run([str(binary), "--data-dir", str(data), "--offline",
                                  "workspace-key-rotate", config["workspace"]],
                                 cwd=root, env=env, capture_output=True, timeout=60, check=True)
        replacement_key = json.loads(rotated.stdout)
        forbidden.append(replacement_key["token"])
        require(replacement_key["key_id"] != config["key"], "Key rotation must issue a new credential ID")
        rotated_env = dict(env, OC_SERVER_URL=endpoint, OC_API_KEY=replacement_key["token"])
        resumed = installer(rotated_env, ["install", "--from-env", "--ui-port", str(ui_port),
                                          "--data-dir", str(data), "--origence", str(binary)],
                            forbidden, entry=deploy_cmd, cwd=root)
        require(resumed["http-mcp"]["status"] == "success" and resumed["console-session"]["status"] == "success",
                "Deployment must accept the rotated Key")
        current_session, _ = request(endpoint + "/v1/session",
                                     headers={"Authorization": "Bearer " + replacement_key["token"]})
        require(current_session == session, "The rotated Key must keep its logical session")
        installer(env, ["uninstall"], forbidden, entry=deploy_cmd, cwd=root)
        user_url = "http://127.0.0.1:20123/mcp"
        subprocess.run([str(codex), "mcp", "add", "origence", "--url", user_url, "--bearer-token-env-var",
                        "OC_API_KEY"], cwd=root, env=env, capture_output=True, timeout=30, check=True)
        result = installer(env, ["uninstall"], forbidden, entry=setup_cmd, cwd=root)
        require(result["codex-mcp"]["status"] == "skipped", "Uninstall must preserve a user-edited Codex entry")
        inventory = subprocess.run([str(codex), "mcp", "get", "origence", "--json"], cwd=root, env=env,
                                   capture_output=True, encoding="utf-8", timeout=30, check=True)
        require(json.loads(inventory.stdout)["transport"]["url"] == user_url,
                "Agent uninstall removed a user-edited Codex entry")
        if WINDOWS:
            require(registry_values()["OC_API_KEY"] == adopted_key,
                    "Agent uninstall must preserve a preconfigured user Key")
        require(not state.exists(), "Both uninstalls left an active journal")
        print("PASS split deployment and Agent setup, native Claude/Codex MCP, Hook Run, private turn archive, backups, and isolated rollback")
    finally:
        try:
            if state.exists():
                installer(env, ["uninstall"], forbidden, entry=setup_cmd, cwd=root)
                installer(env, ["uninstall"], forbidden, entry=deploy_cmd, cwd=root)
        finally:
            if WINDOWS:
                set_registry_workspace_key(registry_before["OC_API_KEY"])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--claude", type=Path, required=True)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--browser", action="store_true", help="Also use Playwright Chromium to inspect the Hook Run.")
    parser.add_argument("--allow-windows-user-env", action="store_true",
                        help="Permit registry writes and restoration in a disposable Windows runner account.")
    args = parser.parse_args()
    if WINDOWS and not args.allow_windows_user_env:
        sys.exit("Run Windows acceptance in a disposable account with --allow-windows-user-env.")
    with fixture_directory("origence installer ") as root:
        binary_dir = root / "Origence binaries"
        binary_dir.mkdir()
        binary = binary_dir / ("origence.exe" if WINDOWS else "origence")
        if WINDOWS:
            shutil.copy2(args.binary, binary)
        else:
            binary.symlink_to(args.binary.absolute())
        smoke(binary, args.claude.absolute(), args.codex.absolute(), root, args.browser)
