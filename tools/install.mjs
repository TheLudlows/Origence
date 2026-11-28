import { realpathSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { connectClaude, connectCodex, removeClaude, removeCodex } from "./installer/agents.mjs";
import {
  ask, defaultEndpoint, npm, repo, rollback, runInstaller, status, tokenPattern, withoutWorkspaceKey,
} from "./installer/command.mjs";
import {
  check, executable, keyFingerprint, prependPath, restoreUserEnvironment,
  windows, windowsSettings, writeUserEnvironment,
} from "./installer/platform.mjs";
import { connectMcp, endpointUrl, identity } from "./installer/protocol.mjs";

const agents = {
  claude: { label: "Claude Code", binary: "claude", package: "@anthropic-ai/claude-code@2.1.330",
    profile: "CLAUDE_CONFIG_DIR", connect: connectClaude },
  codex: { label: "Codex", binary: "codex", package: "@openai/codex@0.171.0",
    profile: "CODEX_HOME", connect: connectCodex },
};
const usage = `Usage: origence-setup install [--agent claude|codex|both] [--endpoint URL] [--token KEY | --from-env]
       origence-setup uninstall

--agent CLIENT           Install Claude Code (default), Codex, or both
--from-env               Read OC_SERVER_URL and OC_API_KEY
--force                  Update settings for all installed agents
--turn-sync-project DIR  Archive private turns for this project (defaults to the current project)
--no-turn-sync           Pause private turn archiving
--claude PATH            Claude Code executable, if it is outside PATH
--codex PATH             Codex executable, if it is outside PATH

Use origence-deploy to start a local API and console.`;

function turnSyncProject(directory) {
  const path = resolve(directory);
  check(statSync(path, { throwIfNoEntry: false })?.isDirectory(),
    "Choose an existing project directory for private turn archiving.");
  const project = realpathSync(path);
  // Match the plugin's project scope for Git worktrees and regular checkouts.
  for (let current = project; ; current = dirname(current)) {
    const git = join(current, ".git");
    const marker = statSync(git, { throwIfNoEntry: false });
    if (marker?.isFile() || (marker?.isDirectory() && statSync(join(git, "HEAD"), { throwIfNoEntry: false })?.isFile())) {
      return current;
    }
    if (current === dirname(current)) return project;
  }
}

function clientBinary(state, name, selected) {
  const agent = agents[name];
  if (selected) {
    const binary = executable(resolve(selected));
    check(binary, `The requested ${agent.label} executable could not be found. Check --${name}.`);
    return binary;
  }
  const prefix = join(state.dir, name);
  const bin = join(prefix, "node_modules", ".bin", agent.binary);
  const installed = executable(agent.binary) ?? executable(bin);
  if (installed) return installed;
  npm("install", "--prefix", prefix, "--no-audit", "--no-fund", agent.package);
  const binary = executable(bin);
  check(binary, `${agent.label} was not installed. Install it or pass its native executable with --${name}.`);
  return binary;
}

async function uninstall(state) {
  await rollback(state, {
    claude: () => removeClaude(state, process.env),
    codex: () => removeCodex(state, process.env),
    environment: () => restoreUserEnvironment(state),
  }, "Removed Agent configuration owned by setup and rolled back its managed user environment settings.");
}

async function install(state, args) {
  const target = args.agent ?? "claude";
  check(["claude", "codex", "both"].includes(target), "Choose --agent claude, codex, or both.");
  const selected = target === "both" ? Object.keys(agents) : [target];
  const installed = Object.keys(agents).filter(name => state.value[name]);
  const old = state.value.config;
  const projectOption = args["turn-sync-project"];
  check(projectOption === undefined || (projectOption && !args["no-turn-sync"]),
    "Supply a project directory with --turn-sync-project, or use --no-turn-sync.");
  const project = args["no-turn-sync"] ? "" : projectOption ?? old?.turnSyncProject
    ?? process.env.OC_TURN_SYNC_PROJECT ?? process.cwd();
  const interactive = process.stdin.isTTY && !args["from-env"];
  const endpointInput = args.endpoint ?? (args["from-env"] ? process.env.OC_SERVER_URL : old?.endpoint ?? process.env.OC_SERVER_URL)
    ?? (interactive ? (await ask("Origence API endpoint [" + defaultEndpoint + "]: ")).trim() || defaultEndpoint : defaultEndpoint);
  const endpoint = endpointUrl(endpointInput).origin;
  const savedKey = () => windows
    ? windowsSettings({ action: "read", names: ["OC_API_KEY"] }).OC_API_KEY?.value || process.env.OC_API_KEY
    : old?.token ?? process.env.OC_API_KEY;
  const token = args.token ?? (args["from-env"] ? process.env.OC_API_KEY : savedKey())
    ?? (interactive ? (await ask("Workspace API key: ", true)).trim() : undefined);
  check(token, "Supply a workspace API key for the existing Origence API.");
  check(tokenPattern.test(token), "The workspace token is not valid printable credential text.");
  const changeAllowed = args.force && installed.every(name => selected.includes(name));
  const changeMessage = "This endpoint and key are shared by installed Agents. To change them, rerun with --force and select all installed Agents.";
  const previousFingerprint = state.value.registry?.OC_API_KEY?.fingerprint;
  const keyChanged = windows
    ? previousFingerprint !== undefined && previousFingerprint !== keyFingerprint(token)
    : old?.token !== token;
  check(!old || (old.endpoint === endpoint && !keyChanged) || changeAllowed, changeMessage);

  const who = await identity(endpoint, token);
  check(!windows || !old?.key || old.key === who.id || changeAllowed, changeMessage);
  const archiveProject = project ? turnSyncProject(project) : "";
  status("whoami", "success", { workspace_id: who.workspace_id, role: who.role });
  const mcp = await connectMcp(endpoint, token, who.workspace_id);
  status("http-mcp", "success", { tools: mcp.toolCount, workspace_id: who.workspace_id });

  const env = withoutWorkspaceKey(process.env);
  for (const name of selected) {
    const variable = agents[name].profile;
    const profile = env[variable] ? resolve(env[variable]) : null;
    check(!state.value[name] || state.value[name].configDir === profile,
      `Use the ${agents[name].label} profile used for setup, or uninstall before selecting another profile.`);
    if (profile) env[variable] = profile;
  }
  const launchers = join(repo, "bin");
  check(executable(join(launchers, "origence-setup")), "The installer package is missing its origence-setup launcher.");
  const clients = Object.fromEntries(selected.map(name => [
    name, clientBinary(state, name, args[name] ?? state.value[name]?.executable),
  ]));
  const dirs = [...new Set([
    launchers, dirname(process.execPath),
    ...Object.keys(agents).map(name => clients[name] ?? state.value[name]?.executable).filter(Boolean).map(dirname),
  ])];
  env.PATH = prependPath(process.env.PATH, dirs);
  check(executable("node", env.PATH), "Node.js 24 must be on the Agent hook PATH.");
  const vars = { OC_API_KEY: token, OC_SERVER_URL: endpoint, OC_WORKSPACE_ID: who.workspace_id,
    OC_TURN_SYNC_PROJECT: archiveProject };
  state.value.config = { endpoint, ...(windows ? {} : { token }),
    workspace: who.workspace_id, key: who.id, turnSyncProject: turnSync };
  state.save();
  for (const name of selected) agents[name].connect(state, clients[name], { ...env, ...vars }, endpoint);
  writeUserEnvironment(state, vars, dirs);
  status("user-environment", "success", "Saved the Agent environment. Restart your terminal and the selected clients.");
  status("installation", "success", { endpoint, workspace_id: who.workspace_id, role: who.role,
    agents: selected, private_turn_sync: Boolean(archiveProject) });
}

await runInstaller({
  name: "origence-setup", scope: "agent", usage, install, uninstall,
  strings: ["agent", "endpoint", "token", "claude", "codex", "turn-sync-project"], flags: ["no-turn-sync"],
});
