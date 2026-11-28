# Origence Agent setup and local deployment

**English** · [简体中文](INSTALLER.md)

Run from a checkout or distribution containing `bin/`, `tools/`, `ui/`, and the plugin marketplaces. Both commands need Node.js 24 with npm. Building Origence requires the repository's Rust toolchain and native build tools. Use `--origence <path>` to reuse an existing executable. First-time dependency fetching requires access to the relevant registries.

| Task | Command | Inputs |
| --- | --- | --- |
| Connect Claude Code or Codex to Origence | `origence-setup` | Client selection, running API URL, workspace key |
| Run Origence and its console locally | `origence-deploy` | New workspace name, or existing API URL and workspace key |

## Connect an Agent

Run the appropriate launcher from the checkout root.

```powershell
.\bin\origence-setup.cmd install --agent claude
.\bin\origence-setup.cmd install --agent codex
```

```sh
./bin/origence-setup install --agent claude
./bin/origence-setup install --agent codex
```

Use `--agent both` to connect both clients with one endpoint and key. Claude Code is the default. Setup reuses each available client CLI or installs its pinned version through npm. Use `--claude` or `--codex` for an executable outside PATH. [Client integration](CLIENTS.en.md)

Enter the API URL and workspace key when prompted. The key is hidden while typing. The default URL is `http://127.0.0.1:8080`. Use HTTPS for remote APIs and omit the `/mcp` suffix. Automation can supply `OC_SERVER_URL` and `OC_API_KEY` with `--from-env`. To keep keys out of command history, use the prompt or the environment. A reader key supports retrieval. Run hook reporting needs write access and uses the local Origence executable.

Setup verifies the workspace and tools served over HTTP MCP, installs the plugin, and saves the user environment and PATH for the clients and hooks. It checks the registered entry in Codex and uses the vendor CLI for non-default URLs. Conflicting existing MCP entries are reported for the user to resolve. The Agent command uses a running API and needs no console port.

On Windows, Agent setup persists the workspace key only in the `OC_API_KEY` user environment variable. A default rerun reads that variable first, then checks the process environment or prompts for the key. Updating setup removes key strings from old Agent installation records and retains a fingerprint of the configured value. Uninstall removes a key written by setup if it still matches and preserves a matching value the user had already set or a later user change. Replacing a key discards the previous value.

`origence-setup` enables [private turn archiving](CLIENTS.en.md#incremental-private-turn-archiving) by default with a writable key. It uses the Git project root where setup runs, or the current directory outside Git. A saved selection takes precedence, followed by an existing `OC_TURN_SYNC_PROJECT`. Use `--turn-sync-project <directory>` to select, change, or resume archiving for a project, and `--no-turn-sync` to pause. Both clients share the setting. Archiving collects new turns in the selected project after it is enabled.

Restart the terminal and clients, then use `/mcp` to check the connection and `context_session` to confirm the workspace. Follow Codex's trust flow in `/hooks` for the installed hooks. To change the endpoint or key, use `--force` and select all currently connected Agents because they share that configuration.

Run `origence-setup uninstall` from a restarted terminal, or use the platform launcher above with `uninstall`. This rolls back the Agent plugins, MCP overrides, and environment changes it owns. The deployment command manages services and the console.

## Deploy the local API and console

Use `--init` for a fresh local workspace. The console defaults to `127.0.0.1:3000`. Use `--ui-port` if needed. To reuse a workspace, omit `--init` and provide its original endpoint and key. For a stopped service, use `--data-dir` or `OC_DATA_DIR` to specify the existing directory.

```powershell
.\bin\origence-deploy.cmd install --init demo --endpoint http://127.0.0.1:8080 --ui-port 3001
```

```sh
./bin/origence-deploy install --init demo --endpoint http://127.0.0.1:8080 --ui-port 3001
```

Fresh deployment issues a workspace key and a separate deployment administrator key, then verifies both console sign-ins. The workspace key is also the Agent credential. The administrator key can inspect the deployment and moderate global knowledge. Obtain both newly issued keys through interactive initialization, `--credentials-file <new path outside Git>`, or an explicit `--show-secrets`. Operators can rotate the administrator key offline. Existing role-based credentials continue to support account sign-in. Add `--verify-write` to record and read one labeled test Run through the console. Reruns use the same probe Run.

Repeated commands reuse the deployment. Only an explicit `--force --init <name>` replaces the workspace. It renames an existing data directory to a backup first. Stop any external services using that directory before reinitializing.

Run `./bin/origence-deploy uninstall` or `.\bin\origence-deploy.cmd uninstall` from the checkout. It stops deployment-managed processes and removes account configuration that it owns. Databases, backups, exported credentials, and Agent plugins are retained. Existing all-in-one journals are read by ownership. To roll back a former all-in-one install completely, run both uninstall commands. Private state and service logs are under `%LOCALAPPDATA%\Origence\installer` on Windows or `~/.origence/installer` on Unix.

[Operations and recovery](OPERATIONS.en.md) · [Claude Code and Codex integration](CLIENTS.en.md)
