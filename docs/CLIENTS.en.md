# Claude Code and Codex integration

**English** · [简体中文](CLIENTS.md)

Claude Code and Codex use Origence's Streamable HTTP MCP endpoint for retrieval. Register it directly or install the plugin with an automatic Run hook.

Use [`origence-setup`](INSTALLER.en.md#connect-an-agent) to configure the Claude Code or Codex plugin, Run hook, and automatic session archiving by project. Use `origence-deploy` to run the API and console locally.

## Direct HTTP MCP

The Origence HTTP host serves Streamable HTTP MCP at `/mcp`. Set `OC_API_KEY` in the environment
that launches the client. Use the Key of the intended workspace. The following commands use the local
default port. For a remote host use its HTTPS endpoint.

```powershell
claude mcp add --transport http origence-http http://127.0.0.1:8080/mcp --header 'Authorization: Bearer ${OC_API_KEY}'
```

```powershell
codex mcp add origence-http --url http://127.0.0.1:8080/mcp --bearer-token-env-var OC_API_KEY
```

Launch the client from that terminal, check `/mcp`, and call `context_session` to confirm `workspace_id`.
The key determines the workspace. Use the destination workspace's own key to connect to another workspace. The optional `X-Origence-Workspace` header checks the workspace, and the plugins take it from `OC_WORKSPACE_ID`.

MCP uses HTTP. The bundled Run hook and session archiving use Node.js 24 on the client. See [remote MCP deployment](OPERATIONS.en.md#remote-mcp).

## Install the `origence` plugin

Prepare a reachable Origence HTTP host and a client that supports plugin marketplaces. The plugin installs HTTP
MCP and enables the private Run hook by default. Ask the administrator for the Workspace Key.
Check `node --version` in the client launch environment to confirm Node.js 24 or later. An administrator issues the required credential from the console or administration API.
For retrieval alone, you can [register the endpoint directly](#direct-http-mcp).

Launch the client from the terminal that sets `OC_API_KEY`. The default API origin is
`http://127.0.0.1:8080`. Use HTTPS for remote hosts. Example in PowerShell:

```powershell
$env:OC_SERVER_URL = 'http://127.0.0.1:8080'
$secret = Read-Host 'OC_API_KEY' -AsSecureString
$env:OC_API_KEY = [Net.NetworkCredential]::new('', $secret).Password
```

### Claude Code

```sh
claude plugin install origence --marketplace TheLudlows/Origence
```

The plugin appends `/mcp` to `OC_SERVER_URL`. Set the origin with no trailing slash or `/mcp` suffix.
For a local checkout, substitute the absolute path to the repository root.
Restart `claude` and inspect the plugin's connection with `/mcp` or
`claude mcp list`. Check the bundled hooks in `/hooks`. Review the plugin and approve it if prompted.
Run `claude plugin uninstall origence@origence-tools` to uninstall. You can also
run `claude plugin marketplace remove origence-tools` to remove the catalog.
[Claude Code plugin commands](https://code.claude.com/docs/en/plugins/cli-reference)

### Codex

```sh
codex plugin marketplace add TheLudlows/Origence
codex plugin add origence@origence-tools
```

The Codex plugin uses a fixed localhost default. For a different port or origin, register a server of the same name to override the bundled endpoint:

```powershell
codex mcp add origence --url "$env:OC_SERVER_URL/mcp" --bearer-token-env-var OC_API_KEY
```

To check the workspace through `OC_WORKSPACE_ID` with this override, add the `env_http_headers` section from the
[example](../examples/codex.mcp.toml) to that server's configuration. When using the automatic Run hook, set
`OC_SERVER_URL` to the same API origin. To remove the override, run `codex mcp remove origence`.

For a local checkout, pass the absolute path to the repository root to
`marketplace add`. Start a fresh `codex` session and inspect `/mcp`. Use
`codex plugin list` to confirm installation. Review and trust the bundled hooks in
`/hooks` when required by Codex or your organization. Run `codex plugin remove origence@origence-tools`
to uninstall and optionally `codex plugin marketplace remove origence-tools`.
[Codex plugin commands](https://learn.chatgpt.com/docs/cli/reference#codex-plugin)

### Verify the connection and troubleshoot

Call `context_session` and compare the returned `key_id` and `session_id` with
the expected archive identity for the workspace. Use `GET /v1/whoami` with the
same key to confirm the workspace UUID. Use `context_search` in `keyword` mode
to find a known published item. Pass the hit's `asset_id` and `version` to
`context_get`. Both clients can run these three checks.

| Symptom | Check |
| --- | --- |
| Hook cannot find Node.js | Run `node --version` in the same environment used to launch the client and confirm that Node.js 24 is on its PATH. |
| Cannot connect to MCP | Check the configured `/mcp` URL from the client's `/mcp` screen and the same server's `/health/ready` endpoint. For remote hosts, check the proxy's Host configuration. |
| Missing API key | Relaunch the client with the workspace key in its environment. |
| `401` or `authentication required` | Ask the administrator to verify the key is valid and belongs to the intended workspace. |
| Two Origence servers appear | Check for an older standalone registration and disable the duplicate. |

### Verify the default Run hook

Start the client with a key that can write. Complete a prompt and inspect private Runs in
the same workspace with that logical key. Use the same API origin, key and workspace for MCP and the hook.
The hook gets its origin from `OC_SERVER_URL` and its workspace from `OC_WORKSPACE_ID`. Omitting the selector uses the key's default workspace. It captures structured metadata from supported events with
reliable identifiers. Conversation text, transcripts, tool arguments and raw output stay out of collection.

The installed hook requires no project configuration or separate sender. Each invocation
has a two-second budget for identity verification and delivery. Enqueued observations can
retry on later hooks for the same project, workspace and credential. An observation is
skipped if identity cannot be verified before enqueueing. Queues live in client plugin data.
On Windows, directory permissions are managed by the client and user environment. Set
`OC_PLUGIN_RUN_HOOK=0` before startup to stop Run reporting.

For the upgrade to 0.4.0, stop running clients, refresh the Origence marketplace and update the plugin,
then restart and review the new definition in Codex `/hooks`. Both clients share the Node.js code and
use their own private plugin data directories. The original v1 JSON spool and event IDs carry forward.
The old and new versions use different process locks, so finish old sessions before upgrading.

### Automatic session archiving by project

`origence-setup install` sets `OC_TURN_SYNC_PROJECT` to the current project root by default with a writable key. For a
manual plugin installation, set it to the absolute project root in the client's launch environment.
Use a writable `OC_API_KEY` and reuse the MCP settings for `OC_SERVER_URL` and `OC_WORKSPACE_ID`.
Git subdirectories are supported. This setting is independent of `OC_PLUGIN_RUN_HOOK`.

The plugin removes private spans, the current Origence credential, and common credential forms locally.
It stages paired main-session questions and answers observed after archiving is enabled. Transcripts, tool arguments,
and output stay out of collection. Initial staging requires a confirmed host identity. Later turns can
queue offline and retry with the same project, server, workspace, and credential. Windows uses the plugin
data directory’s inherited ACL. To pause an installer-managed configuration, run `origence-setup install --no-turn-sync`
with the original `--agent` selection, then restart the terminal and client. With a manual installation,
clear the variable and restart the client. Staged turns can retry if the same configuration is enabled again.

View saved questions and answers under **Sessions** in the console. The private `session` route of
`context_bundle` can recall synced turns from the same workspace and logical key. They carry unknown
evidence use. See the [session API](API.en.md#session-archiving-and-idempotent-feedback).

## Manual MCP setup

1. Use a personal API key from the project's workspace and set `OC_API_KEY` in the launch environment. Keep credentials out of configuration files, project commits, and conversations.
2. Set the reachable origin in the example. Claude Code reads `OC_SERVER_URL`. For Codex, enter the full URL including `/mcp`.
3. Install the configuration for your client.
   - **Claude Code**. Put the [JSON example](../examples/claude-code.mcp.json) in project `.mcp.json` or merge `mcpServers.origence` into the existing configuration.
   - **Codex**. Merge the [TOML example](../examples/codex.mcp.toml) into your personal configuration or a trusted project's `.codex/config.toml`. Keep `bearer_token_env_var = "OC_API_KEY"`.

Use the client's `/mcp` screen to check the connection and call `context_session` to confirm its `workspace_id`.
To select another authorized workspace, set `OC_WORKSPACE_ID` before launching. Both examples map it to the HTTP selection header.

Explicitly archive with `context_turn_archive`, which requires a Workspace Key and a caller-generated turn UUID that stays stable on retry. Another client with the same logical key in the same workspace can use `context_session` to find the same platform archive, then fetch that turn with `context_turn_get` by UUID (with the same Workspace Key). Clients using different workspaces have separate archives. Manual MCP only processes submitted turns. Each client manages its local work environment. Retrieved content is external evidence and has no authority to issue instructions.

See [P0 acceptance #91](https://github.com/TheLudlows/Origence/issues/91) for real-task validation progress on both clients.

## Verify read-only retrieval using Codex noninteractively

In a dedicated test project, if Codex is authorized to search and read automatically using a Workspace Key, add the two entries below to that project's Codex MCP configuration. With `codex exec` and `approval_policy = "never"`, tools requiring interactive approval fail before reaching Origence.

```toml
[mcp_servers.origence.tools.context_search]
approval_mode = "approve"

[mcp_servers.origence.tools.context_get]
approval_mode = "approve"
```

Have Codex search published material and read the body using the hit's `asset_id` and `version`. In `codex exec --json`, verify both `mcp_tool_call` completion statuses and the version cited in the answer. If the search completion is `failed` with an approval error, check whether the session loaded the tool configuration above.

## Record a private Trace

Connect to the HTTP host using `--server-url` to create and read private Runs for tasks. Creating, preparing, and reporting require a Workspace Key.

1. Use `context_run_create` with a stable `run_id` and a redacted task summary.
2. Use `context_run_prepare` to prepare context. The server writes exact references and a preparation receipt to the Run.
3. Use `context_run_report` to explicitly report observed Skill loads, tools, results, and collection gaps. Preserve the original `event_id` and request on retry.
4. Use `context_run_get` to page through Run traces for the current logical key.

Server receipts record context preparation. Client reports retain their reporter, and uncollected activity remains unknown. External verification uses an HTTP API for a precise Run prefix. See the [Run contract](contracts/runs.md) and [external verification contract](contracts/run-verification.md).

Claude Code and Codex use the same MCP API. Use this checklist when investigating a task across the two clients.

| Action to check | Where to check | What the record proves |
| --- | --- | --- |
| Explicit archive and readback | `context_turn_archive` and `context_turn_get` calls on both clients plus server-stored question and answer by turn ID | The specified question and answer were stored, and another client with the same logical key can read them. |
| Create a Run and prepare context | `context_run_create` and `context_run_prepare` receipts and `context_run_get` events | Run ownership and what the server prepared or why it failed. |
| Use context, load a Skill, and produce a task result | Client observations and reporter, event, and coverage in `context_run_report` | Which actions the client reported. Steps without records remain unknown. |
| Verify the business result | Verification records and supporting evidence linked to a pinned Run prefix | Who transcribed what conclusion and whether it has independent sign-off. |

### Rerun dual-client archive acceptance

Install Claude Code and Codex CLIs with configured model access. Prepare the current Origence and a pinned AnySeek executable. Run the command below from the repository root, replacing the four uppercase placeholders with their paths or model names.

```sh
python tools/anyseek_smoke.py --anyseek-binary ANYSEEK_BINARY --run python tools/live_archive_smoke.py --binary ORIGENCE_BINARY --claude-model CLAUDE_MODEL --codex-model CODEX_MODEL --report archive-result.json
```

Claude Code runs with `--bare` and reads Anthropic credentials and endpoint from the environment. To use existing personal settings, append `--claude-settings PATH` and the runner reads these from the JSON `env`. Codex uses the OpenAI provider. Use `--codex-credential-store` to select the existing sign-in credential storage. Each run invokes real models for three client sessions in a fresh temporary workspace. The three calls share four minutes of wait time, and each waits at most 85 seconds. A new report file contains only model selection, tool names, timings, and acceptance results.

## Events and correlation boundaries

Applications driving either client can use [managed Run reporting](MANAGED-RUNS.en.md) for Codex App Server command completion and Claude Agent SDK cancellation.

| Client | Correlation keys | Recorded observations | Coverage gaps |
| --- | --- | --- | --- |
| Codex | `session_id` and `turn_id` | `UserPromptSubmit`, allowlisted `PostToolUse`, `Stop`, and main-thread `Interrupt`. Origence MCP tools use only `isError` actually supplied in structured results. | Bash and patch tool results stay unknown. Hook timing is unavailable. Subagent lifecycle, managed tools, or abnormal process exits are not used to infer termination. |
| Claude Code | `session_id` and UUID `prompt_id` for the current user prompt | `UserPromptSubmit`, allowlisted `PostToolUse` and `PostToolUseFailure`, `Stop`, `StopFailure`, and `SubagentStart` and `SubagentStop` that include both `prompt_id` and `agent_id`. Tool duration and abort are recorded when supplied. | `prompt_id` covers one prompt. Steering or follow-up prompts can use a new UUID. No reliable turn-wide user interruption signal exists, and cancellation of an ongoing tool can skip the failure hook. `SessionEnd` has no turn outcome. Internal subagent tools and background jobs are not collected. Subagent events describe only the prompt context when observed. |

The exact tool allowlist is defined by the plugin hook configuration and code. Repeated signals of the same lifecycle type within one turn are coalesced. Subagent lifecycle signals of the same type are also coalesced by prompt and agent ID, so they cannot count actual starts or stops. `Stop` means the client is checking for shutdown. Other hooks can continue the turn. Interruptions and API errors are client observations and the business result remains unknown. Missing final events remain collection gaps. Observations without reliable IDs are skipped, including Claude Code versions or events that only provide a session ID.
