# Origence local installer

**English** · [简体中文](INSTALLER.md)

Run from the Origence checkout or a distribution containing `tools/`, `ui/`, and the plugin marketplace. Use PowerShell on Windows, or Bash or Zsh on Linux and macOS. Requires Node.js 24 with npm. The installer uses available Origence and Claude Code executables. It builds missing Origence from the checkout and can install the pinned Claude Code package into its private directory. A source build needs the repository's Rust toolchain and native build tools. Use `--origence` for a prebuilt executable. The installer builds the Node console if needed. First-time dependency fetches require access to their registries.

## Install

For an existing API, start the installer and enter the endpoint and workspace key when prompted. The key prompt hides input.

```sh
node tools/install.mjs
```

You can also use `node tools/install.mjs --endpoint https://origence.example.com --token '<WORKSPACE_KEY>'`. Command-line secrets can appear in history and process listings. For automation, supply `OC_SERVER_URL` and `OC_API_KEY` in the launch environment and run:

```sh
node tools/install.mjs --from-env
```

Remote APIs use HTTPS. For a stopped local instance, use its existing data directory via `--data-dir` or `OC_DATA_DIR`. The installer can also find an already initialized `.data/context.db` in the checkout. Get an existing workspace key from its administrator. The local console account is named `admin` and has the exact permissions of the supplied key. Readers can sign in and retrieve material. Private Run reporting needs writer, reviewer, or admin permission.

For a fresh instance, leave the key prompt blank and supply a workspace name, or run:

```sh
node tools/install.mjs --init demo --endpoint http://127.0.0.1:8080
```

The initial key is displayed once during interactive setup. Interactive success prints the console password. Automation can use `--credentials-file <new path outside Git>` or explicitly request `--show-secrets`. The optional file contains the plaintext password and key. Keep it private and outside Git. Installer state is private to the current user. Unix credential files use mode `0600` and Windows files use a current-user ACL. Programs launched as the current user can read their user environment.

The installer prints structured step results and verifies the real API readiness and identity, MCP initialization and all 11 tools, the browser console root and account session, and the connection reported by Claude Code itself. It compares the workspace and logical key returned by the API, MCP, and console. It records the confirmed workspace so stale scope configuration cannot redirect the plugin.

## Verify, rerun, and uninstall

Restart your terminal and Claude Code. Use `Test-Path Env:OC_API_KEY` in PowerShell or `test -n "$OC_API_KEY"` in Bash or Zsh to check that the key loaded, and run `origence --version`. Use Claude Code's `/mcp` and `/hooks` to check the plugin. Sign in at the printed UI address, which defaults to `http://127.0.0.1:3000`. Complete a Claude Code turn with a writable key to see the resulting Run in that workspace. Add `--verify-write` to create and read a labeled connectivity Run through the console account. Repeated checks use the same Run for the same workspace and logical key.

Reruns reuse the workspace, account, plugin, and listeners. Use `--force` to refresh settings and reset the console password, including when changing the endpoint or key. Only `--force --init <name>` initializes a replacement workspace. It renames an existing data directory to a timestamped backup first. Stop all external Origence processes before reinitializing. The installer checks the listener and existing database lock, and stops its own processes exclusively. Managed processes survive closing the terminal. Rerun the installer to restart them after a reboot.

Run from any directory.

```sh
origence uninstall
```

The shortcut uses the Node executable and installer script recorded during setup. With an older binary or after moving the installer directory, run `node tools/install.mjs --uninstall` from the current checkout.

Installer state and service logs are under `%LOCALAPPDATA%\Origence\installer` on Windows or
`~/.origence/installer` on Unix. Uninstall restores the recorded user environment and PATH changes, removes the installer-created account and Claude plugin, and stops managed processes. It preserves backend data, backups, and exported credential files. Windows uses the current-user registry. Unix startup files source a private environment file. Unrelated PATH entries and shell edits are retained.

| Result | What to check |
| --- | --- |
| Endpoint unreachable or not ready | Check the URL, local data directory, or remote certificate. Logs for managed processes are in the private installer directory. |
| HTTP 401 or 403 | Check the key's workspace, validity and permissions. Existing data requires a key for that workspace. |
| Console port occupied | Select another port with `--ui-port <port>`. The installer leaves external console servers in place. |
| The Run hook produces no record | Check `origence --version`, write permissions, the `/hooks` view, and any `OC_PLUGIN_RUN_HOOK=0` setting. |
| Plugin disabled by policy | Review and enable Origence in Claude Code or contact your organization administrator. |

[Claude Code plugin commands](https://code.claude.com/docs/en/plugins/cli-reference) · [Client integration](CLIENTS.en.md)
