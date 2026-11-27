import { dirname, join, resolve } from "node:path";
import { connectClaude, connectCodex, removeClaude, removeCodex } from "./installer/agents.mjs";
import {
  ask, defaultEndpoint, npm, origenceBinary, repo, rollback, runInstaller, status, tokenPattern, withoutWorkspaceKey,
} from "./installer/command.mjs";
import { check, executable, execute, prependPath, restoreUserEnvironment, writeUserEnvironment } from "./installer/platform.mjs";
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
--origence PATH          Origence executable for Run hooks
--claude PATH            Claude Code executable, if it is outside PATH
--codex PATH             Codex executable, if it is outside PATH

Use origence-deploy to start a local API and console or initialize a workspace.`;

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
  }, "Removed Agent configuration owned by setup and restored its user environment changes.");
}

async function install(state, args) {
  const target = args.agent ?? "claude";
  check(["claude", "codex", "both"].includes(target), "Choose --agent claude, codex, or both.");
  const selected = target === "both" ? Object.keys(agents) : [target];
  const installed = Object.keys(agents).filter(name => state.value[name]);
  const old = state.value.config;
  const interactive = process.stdin.isTTY && !args["from-env"];
  const endpointInput = args.endpoint ?? (args["from-env"] ? process.env.OC_SERVER_URL : old?.endpoint ?? process.env.OC_SERVER_URL)
    ?? (interactive ? (await ask("Origence API endpoint [" + defaultEndpoint + "]: ")).trim() || defaultEndpoint : defaultEndpoint);
  const endpoint = endpointUrl(endpointInput).origin;
  const token = args.token ?? (args["from-env"] ? process.env.OC_API_KEY : old?.token ?? process.env.OC_API_KEY)
    ?? (interactive ? (await ask("Workspace API key: ", true)).trim() : undefined);
  check(token, "Supply a workspace API key for the existing Origence API.");
  check(tokenPattern.test(token), "The workspace token is not valid printable credential text.");
  const changed = old && (old.endpoint !== endpoint || old.token !== token);
  check(!changed || args.force && installed.every(name => selected.includes(name)),
    "This endpoint and key are shared by installed Agents. To change them, rerun with --force and select all installed Agents.");

  const who = await identity(endpoint, token);
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
  const binary = origenceBinary(args.origence ?? old?.binary);
  const launchers = join(repo, "bin");
  check(executable(join(launchers, "origence-setup")), "The installer package is missing its origence-setup launcher.");
  const clients = Object.fromEntries(selected.map(name => [
    name, clientBinary(state, name, args[name] ?? state.value[name]?.executable),
  ]));
  const dirs = [...new Set([
    launchers, dirname(binary),
    ...Object.keys(agents).map(name => clients[name] ?? state.value[name]?.executable).filter(Boolean).map(dirname),
  ])];
  env.PATH = prependPath(process.env.PATH, dirs);
  check(executable("origence", env.PATH) === binary,
    "Select a binary named origence (origence.exe on Windows) for the plugin hook PATH.");
  execute(binary, ["plugin-hook", "--help"], { env, diagnosis: "The Origence binary does not support Agent hooks" });
  const vars = { OC_API_KEY: token, OC_SERVER_URL: endpoint, OC_WORKSPACE_ID: who.workspace_id };
  state.value.config = { endpoint, token, binary, workspace: who.workspace_id, key: who.id };
  state.save();
  for (const name of selected) agents[name].connect(state, clients[name], { ...env, ...vars }, endpoint);
  writeUserEnvironment(state, vars, dirs);
  status("user-environment", "success", "Saved the Agent environment. Restart your terminal and the selected clients.");
  status("installation", "success", { endpoint, workspace_id: who.workspace_id, role: who.role, agents: selected });
}

await runInstaller({
  name: "origence-setup", scope: "agent", usage, install, uninstall,
  strings: ["agent", "endpoint", "token", "origence", "claude", "codex"], flags: [],
});
