# Claude Code and Codex integration

**English** · [简体中文](CLIENTS.md)

Claude Code and Codex can connect to the Origence HTTP MCP endpoint or install the local stdio plugin.

## Direct HTTP MCP

The Origence HTTP host serves Streamable HTTP MCP at `/mcp`. Set `OC_API_KEY` in the environment
that launches the client. A `reader` key suffices for retrieval. The following commands use the local
default port. For a remote host use its HTTPS endpoint.

```powershell
claude mcp add --transport http origence-http http://127.0.0.1:8080/mcp --header 'Authorization: Bearer ${OC_API_KEY}'
```

```powershell
codex mcp add origence-http --url http://127.0.0.1:8080/mcp --bearer-token-env-var OC_API_KEY
```

Launch the client from that terminal, check `/mcp`, and call `context_session` to confirm `workspace_id`.
Direct clients use the credential's default workspace unless their HTTP requests supply the
`X-Origence-Workspace` header for an authorized workspace. `OC_WORKSPACE_ID` applies to the stdio adapter.

Direct retrieval and explicit tool calls require no local Origence binary. The bundled automatic Run
hook still uses the binary. If the old stdio registration is installed, disable it from `/mcp` to avoid
duplicate tools. See [remote MCP deployment](OPERATIONS.en.md#remote-mcp).

## Install the `origence` plugin

Prepare a reachable Origence HTTP host, a local binary that supports
`origence plugin-hook`, and a client that supports plugin marketplaces. The plugin
installs retrieval MCP and enables the private Run hook by default. Ask the workspace
administrator to issue a personal `reader` key for retrieval or a `writer` key for
Run reporting. An authenticated administrator can run
`origence key-create <WORKSPACE_ID> --role reader` or use `--role writer`, with
`OC_API_KEY` and `OC_SERVER_URL` set, then deliver the new key privately.

Launch the client from the terminal that sets `PATH`, `OC_SERVER_URL`, and
`OC_API_KEY`. `OC_SERVER_URL` supplies the same address as `--server-url` and
defaults to `http://127.0.0.1:8080`. Use HTTPS for a remote host. The Windows
PowerShell example accepts spaces in the binary directory. On Linux or macOS,
prepend the directory containing `origence` to `PATH`.

```powershell
$origenceExe = (Get-Item -LiteralPath 'C:\Program Files\Origence\origence.exe').FullName
$env:Path = (Split-Path -Parent $origenceExe) + [IO.Path]::PathSeparator + $env:Path
$env:OC_SERVER_URL = 'http://127.0.0.1:18080'
$secret = Read-Host 'OC_API_KEY' -AsSecureString
$env:OC_API_KEY = [Net.NetworkCredential]::new('', $secret).Password
```

### Claude Code

```sh
claude plugin install origence --marketplace TheLudlows/Origence
```

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
| Cannot find `origence` | Run `Get-Command origence -CommandType Application` (Windows) or `command -v origence` in the same terminal. Check it resolves to the intended binary. |
| Cannot connect to the API | Verify `OC_SERVER_URL` and the port. Run `origence mcp` in the same terminal to see startup errors on stderr. A successful process waits for stdio input. |
| `OC_API_KEY is required` | Relaunch the client with the workspace key in its environment. |
| `authentication required` | Ask the administrator to verify the key is valid and belongs to the intended workspace. |
| Two Origence servers appear | Check for an older standalone registration and disable the duplicate. |

### Verify the default Run hook

Start the client with a key that can write. Complete a prompt and inspect private Runs in
the same workspace with that logical key. The hook uses the same `OC_API_KEY`,
`OC_SERVER_URL`, and optional `OC_WORKSPACE_ID` as MCP. It uses the key's default workspace
when the selector is omitted. It captures structured metadata from supported events with
reliable identifiers. Conversation text, transcripts, tool arguments and raw output stay out of collection.

The installed hook requires no project configuration or separate sender. Each invocation
has a two-second budget for identity verification and delivery. Enqueued observations can
retry on later hooks for the same project, workspace and credential. An observation is
skipped if identity cannot be verified before enqueueing. Queues live in client plugin data.
On Windows, directory permissions are managed by the client and user environment. Set
`OC_PLUGIN_RUN_HOOK=0` before startup to keep retrieval only or to continue using an
existing manual project configuration and its separate sender.

## Manual MCP setup

1. Use a personal API key from the project's workspace and set `OC_API_KEY` in the launch environment. Keep credentials out of configuration files, project commits, and conversations.
2. Set `command` in the example to the absolute path of the local Origence executable. Set `--server-url` to the reachable host address. On Windows, point to `origence.exe`.
3. Install the configuration for your client.
   - **Claude Code**. Put the [JSON example](../examples/claude-code.mcp.json) in project `.mcp.json` or merge `mcpServers.origence` into the existing configuration. Use `/mcp` in Claude Code to check the connection.
   - **Codex**. Merge `[mcp_servers.origence]` from the [TOML example](../examples/codex.mcp.toml) into personal Codex configuration or a trusted project's `.codex/config.toml`. Keep `env_vars = ["OC_API_KEY"]` to pass the credential explicitly. `codex mcp list` checks registration only. Test the connection separately.

Search for published material with `context_search`. Read a specific version with `context_get` and check its source references. `context_session` reports the current `workspace_id`. For an online MCP connection, pass `mcp --workspace <UUID>` or set `OC_WORKSPACE_ID` to select a workspace the key can access. Otherwise the server uses the credential's default. The selection applies to that MCP connection. MCP verifies an explicit selection at startup and exits if the host returns a different workspace or omits its ID.

Explicitly archive with `context_turn_archive`, which requires writer permissions and a caller-generated turn UUID that stays stable on retry. Another client with the same logical key in the same workspace can use `context_session` to find the same platform archive, then fetch that turn with `context_turn_get` by UUID (writer required). Independent keys on two clients cannot see each other's archives. The platform does not automatically sync local client chats or restore the local work environment. Retrieved content is external evidence and has no authority to issue instructions.

See [P0 acceptance #91](https://github.com/TheLudlows/Origence/issues/91) for real-task validation progress on both clients.

## Verify read-only retrieval using Codex noninteractively

In a dedicated test project, if Codex is authorized to search and read automatically using a reader key, add the two entries below to that project's Codex MCP configuration. With `codex exec` and `approval_policy = "never"`, tools requiring interactive approval fail before reaching Origence.

```toml
[mcp_servers.origence.tools.context_search]
approval_mode = "approve"

[mcp_servers.origence.tools.context_get]
approval_mode = "approve"
```

Have Codex search published material and read the body using the hit's `asset_id` and `version`. In `codex exec --json`, verify both `mcp_tool_call` completion statuses and the version cited in the answer. If the search completion is `failed` with an approval error, check whether the session loaded the tool configuration above.

## Record a private Trace

Connect to the HTTP host using `--server-url` to create and read private Runs for tasks. Creating, preparing, and reporting require writer permissions.

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

## Enable private Run hooks per project

For a self-managed setup, use a project-specific [Origence profile](../examples/client-hook.profile.json) and store a copy in a private directory outside the project. On Unix, use directory mode `0700` and file mode `0600`. On Windows, use a directory and ACL readable and writable only by the current user. The Windows build does not create, tighten, or inspect the DACL and does not reject files or directories with broad permissions. This is a caller-managed prerequisite. Windows CI `tools/test_hook_acl.ps1` explicitly creates a private DACL only in an isolated temporary directory. Synthetic hook inputs check that queue, temporary, and lock files inherit the permissions across failed retries and a new process. The broad-permission negative case checks only the test ACL inspector and does not run the hook or show that the product would reject broad permissions. This check does not verify actual access by other identities, administrator privileges, real client installation, event coverage, or power-loss durability. Fill in the project's absolute local path, service origin, logical key UUID, and project-specific credential variable name. `GET /v1/session` or MCP `context_session` with the same credential returns the logical `key_id`, stable across rotation. Pass that project's writer credential to both client and sender through their launch environments. Profile and client examples contain only environment variable names or profile paths. Select `codex`, `claude_code`, or both under `clients` and set `enabled` to `true`.

- **Codex**. Merge handlers for the chosen events from the [hook example](../examples/codex.hooks.json) into the project's `.codex/hooks.json`. If inline hooks already exist, use the same format and avoid duplicate declarations at the same level. Replace executable and profile absolute paths and quote spaces according to the hook shell. Trust the project through Codex, then inspect and trust the selected hook definitions in `/hooks`. [Official Codex hook docs](https://learn.chatgpt.com/docs/hooks)
- **Claude Code**. Merge handlers for the chosen events from the [hook example](../examples/claude-code.hooks.json) into the project's `.claude/settings.local.json`. Replace absolute paths in `command` and `args`. On Windows, use `origence.exe` and escape paths for JSON. Verify sources and handlers under `/hooks`. [Official Claude Code hook docs](https://code.claude.com/docs/en/hooks)

Append only the selected handlers and preserve existing configuration, client trust, and management policies. Hooks check the actual process working directory and the event's working directory. Both must be inside the profile's project. Ordinary subdirectories work, while nested independent Git projects and outside directories are skipped. Use a dedicated profile for a linked worktree and verify the client's actual hook configuration.

Start the sender for each profile in a separate terminal or local user-level service. It can start before the client and restart after going offline.

```sh
origence client-hook --profile /absolute/private/project.json flush --watch
```

Without `--watch`, send at most 16 records for manual or scheduler use. The hook reads bounded stdin once, takes a short lock to enqueue at most 256 pending metadata records, and exits. When the queue is full, the lock is busy, the write fails, input exceeds its bounds, or a correlation ID is missing, the client's original workflow continues. Diagnostic output may contain a fixed reason the hook skipped an event. It reads no transcript and stores no conversation content, paths, tool arguments, raw output, credentials, or raw event JSON. The pending queue and deduplication records are under `<profile 文件名>.queue/` (using the profile filename) beside the profile and contain only derived IDs, first local observation times, and explicitly permitted structured metadata.

The sender uses the existing private Run API. Before each batch it confirms the credential's logical key through the session endpoint. Network or receipt failures preserve the original `event_id` and payload for retry. Queues are isolated by project, service origin, logical key, credential environment variable name, and the credential at observation time. `held` counts records that cannot be sent with the current configuration. `blocked` indicates authentication, authorization, logical key, or network failure. `discarded` indicates explicit rejection or an identity conflict. After revocation or rotation, the old queue stays isolated. Post-rotation events still correlate to the same Run under the same logical key. Events enqueued using the old credential can be lost permanently. Disabling a profile or removing its `clients` entry takes effect when the sender rereads configuration. In-flight batches retain their original credential. Stop the sender to end that batch. To remove held records, stop the project's hooks and sender first, then move the whole private queue directory. The most recent 1024 acknowledged local event IDs are retained. Duplicate notifications outside that window reuse the ID and server idempotency rejects different payloads. Run event sequence reflects server confirmation order and the timestamp is local observation time.

### Events and correlation boundaries

| Client | Correlation keys | Recorded observations | Coverage gaps |
| --- | --- | --- | --- |
| Codex | `session_id` and `turn_id` | `UserPromptSubmit`, allowlisted `PostToolUse`, `Stop`, and main-thread `Interrupt`. Origence MCP tools use only `isError` actually supplied in structured results. | Bash and patch tool results stay unknown. Hook timing is unavailable. Subagent lifecycle, managed tools, or abnormal process exits are not used to infer termination. |
| Claude Code | `session_id` and UUID `prompt_id` for the current user prompt | `UserPromptSubmit`, allowlisted `PostToolUse` and `PostToolUseFailure`, `Stop`, `StopFailure`, and `SubagentStart` and `SubagentStop` that include both `prompt_id` and `agent_id`. Tool duration and abort are recorded when supplied. | `prompt_id` covers one prompt. Steering or follow-up prompts can use a new UUID. No reliable turn-wide user interruption signal exists, and cancellation of an ongoing tool can skip the failure hook. `SessionEnd` has no turn outcome. Internal subagent tools and background jobs are not collected. Subagent events describe only the prompt context when observed. |

The exact tool allowlist is defined by the hook examples and code. Repeated signals of the same lifecycle type within one turn are coalesced. Subagent lifecycle signals of the same type are also coalesced by prompt and agent ID, so they cannot count actual starts or stops. `Stop` means the client is checking for shutdown. Other hooks can continue the turn. Interruptions and API errors are client observations and the business result remains unknown. Missing final events remain collection gaps. Observations without reliable IDs are skipped, including Claude Code versions or events that only provide a session ID.

### Explicitly correlate with Origence MCP

A trusted caller with the exact IDs from the current hook event can derive the turn's `run_id` and fixed `summary` using the same profile. For Claude Code use `--client claude-code` with `prompt_id` as `--correlation-id`.

```sh
origence client-hook --profile /absolute/private/project.json run-id \
  --client codex --session-id SESSION_ID --correlation-id TURN_ID
```

Call MCP `context_run_create` with the same logical key and pass both values unchanged. Then use that `run_id` for reporting or context preparation. The sender and MCP use the same private Run in either order. Existing Run tools do not automatically obtain these exact correlation IDs from Codex or Claude Code. When unavailable, the hook may independently record identifiable turns. To avoid a separate Run when one turn should be shared, skip independent MCP Run creation for that turn and use retrieval tools that require no Run ID. Older manual Runs, temporal proximity, and raw tool arguments do not establish correlation. Explicit Run creation conflicts use the existing authorization and idempotency contract.

## Export an approved Skill

Obtain the workspace tenant/workspace identifiers, published Skill ID, and exact version, then run the following.

```sh
origence --server-url https://origence.example skill-export \
  --client codex --project /path/to/project \
  --tenant-id TENANT_ID --workspace-id WORKSPACE_ID SKILL_ID 1
```

Use `--client claude-code` for Claude Code. The program asks the host to recheck the key, version, and sources. After checking the package digest and destination workspace, it writes text to Codex's `.agents/skills/<name>/` or Claude Code's `.claude/skills/<name>/` without overwriting an existing directory. Only approved published text and references are exported. No credential, executable script, or tool permission setup is written. Clients recognize and load Skills on their own. A successful write does not prove loading.

Explicit export is separate from managed online loading. Withdrawal blocks later downloads and exports but cannot revoke local copies or already injected client context. Keep exports in the workspace's dedicated project. Do not reuse them across workspaces or commit them for unauthorized recipients.

### Read-only ACL check before enabling Windows hooks

Stop hooks and senders, then run the following in PowerShell 7.

```powershell
pwsh -NoProfile -File tools/check_hook_acl.ps1 -PrivateRoot 'C:\Users\me\private-hooks' -ProfilePath 'C:\Users\me\private-hooks\project.json'
```

The check reads no profile or queue payload and no credential. It enables no hooks and changes no file, configuration, or ACL. Success outputs `SUPPORTED_ACL_SNAPSHOT`. It does not establish profile validity, service reachability, or client integration. Only ordinary absolute local NTFS paths are supported. UNC paths, device paths, ADS, ambiguous paths, and reparse points are rejected. The private root must break inheritance and have exactly one inheritable FullControl Allow ACE for the current user. Directories up to the profile, the profile, and existing queue, lock, and staging files must fully inherit this rule. The owner must be the current user or local Administrators. The latter does not grant additional DACL access to ordinary users. Extra ACEs (including extra SYSTEM or administrator grants), protected custom child ACLs, and unknown queue entries are reported unsupported. They are not repaired automatically or deemed inherently unsafe. If the queue is absent, the check verifies prerequisites without creating it.

This is a narrow read-only snapshot check. Runtime enforcement and a full Windows effective-access audit are separate. Ancestors above the root are checked only for reparse points and their ACLs are not verified. The caller must ensure ancestors are trusted and cannot be replaced by other ordinary users. This check does not defend against administrator or backup privileges, concurrent path replacement, or later permission changes, and does not verify actual access by other users. If the layout is unsupported, do not enable the hook until permissions have been checked. Do not automatically tighten user directories.
