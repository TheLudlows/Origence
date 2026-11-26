import { createHash } from "node:crypto";
import { check, executable, execute } from "./platform.mjs";
import { defaultEndpoint, repo, status, withoutWorkspaceKey } from "./command.mjs";

const plugin = "origence@origence-tools";
const marketplace = "origence-tools";
const fingerprint = value => createHash("sha256").update(JSON.stringify(value)).digest("hex");

function pluginCommand(state, claude, env, args) {
  return execute(claude, args, {
    cwd: state.dir, env, diagnosis: "Claude Code could not configure or verify Origence. Check CLI version and plugin policy",
  });
}

function inventory(state, claude, env) {
  const plugins = JSON.parse(pluginCommand(state, claude, env, ["plugin", "list", "--json"]));
  const markets = JSON.parse(pluginCommand(state, claude, env, ["plugin", "marketplace", "list", "--json"]));
  check(Array.isArray(plugins) && Array.isArray(markets), "Claude Code returned an unsupported plugin inventory.");
  return { plugins, markets };
}

export function connectClaude(state, claude, env, endpoint) {
  const setupEnv = withoutWorkspaceKey(env);
  const { plugins, markets } = inventory(state, claude, setupEnv);
  const existing = plugins.find(item => item.id === plugin && item.scope === "user");
  const marketExists = markets.some(item => item.name === marketplace);
  if (!state.value.claude) {
    state.value.claude = { executable: claude, configDir: env.CLAUDE_CONFIG_DIR ?? null,
      addedPlugin: !existing, enabledPlugin: existing?.enabled === false, addedMarket: !marketExists };
    state.save();
  } else if (state.value.claude.executable !== claude) {
    state.value.claude.executable = claude;
    state.save();
  }
  if (!existing) {
    const installArgs = marketExists ? [plugin] : ["origence", "--marketplace", repo];
    pluginCommand(state, claude, setupEnv, ["plugin", "install", "--scope", "user", ...installArgs]);
  } else if (!existing.enabled) {
    pluginCommand(state, claude, setupEnv, ["plugin", "enable", plugin, "--scope", "user"]);
  }
  const listing = pluginCommand(state, claude, env, ["mcp", "list"]);
  const server = listing.split(/\r?\n/).find(line => line.includes("plugin:origence:origence:"));
  // Claude Code uses a different check mark on Windows consoles.
  check(server?.includes(endpoint + "/mcp") && /[✔✓√]\s*Connected\s*$/i.test(server),
    "Claude Code did not connect to the installed HTTP MCP endpoint. Check for disabled plugins, managed settings or conflicting MCP configuration.");
  status("claude-code", "success", "The user-scope plugin reports its HTTP MCP server connected.");
}

export function removeClaude(state, env) {
  const record = state.value.claude;
  if (!record) return;
  // Use the same Claude profile even if uninstall was launched from another shell.
  const clientEnv = withoutWorkspaceKey(env);
  if (record.configDir === null) delete clientEnv.CLAUDE_CONFIG_DIR;
  else clientEnv.CLAUDE_CONFIG_DIR = record.configDir;
  const claude = executable(record.executable);
  check(claude, "Claude Code is missing. Restore the executable to roll back plugin settings.");
  const { plugins, markets } = inventory(state, claude, clientEnv);
  const existing = plugins.find(item => item.id === plugin && item.scope === "user");
  if (existing && record.addedPlugin)
    pluginCommand(state, claude, clientEnv, ["plugin", "uninstall", plugin, "--scope", "user"]);
  else if (existing?.enabled && record.enabledPlugin)
    pluginCommand(state, claude, clientEnv, ["plugin", "disable", plugin, "--scope", "user"]);
  if (record.addedMarket && markets.some(item => item.name === marketplace) &&
      !plugins.some(item => item.id?.endsWith("@" + marketplace) && !(item === existing && record.addedPlugin)))
    pluginCommand(state, claude, clientEnv, ["plugin", "marketplace", "remove", marketplace]);
}

function codexCommand(state, codex, env, args) {
  return execute(codex, args, {
    cwd: state.dir, env, diagnosis: "Codex could not configure Origence. Check the CLI version and plugin policy",
  });
}

function codexInventory(state, codex, env) {
  const plugins = JSON.parse(codexCommand(state, codex, env, ["plugin", "list", "--json"])).installed;
  const markets = JSON.parse(codexCommand(state, codex, env, ["plugin", "marketplace", "list", "--json"])).marketplaces;
  check(Array.isArray(plugins) && Array.isArray(markets), "Codex returned an unsupported plugin inventory.");
  return { plugins, markets };
}

function localMcp(state, codex, env) {
  // Disabling plugins for this read distinguishes a user-defined server from
  // the server supplied by the Origence plugin.
  const servers = JSON.parse(codexCommand(state, codex, env, ["mcp", "list", "--disable", "plugins", "--json"]));
  check(Array.isArray(servers), "Codex returned an unsupported MCP inventory.");
  if (!servers.some(server => server.name === "origence")) return null;
  return JSON.parse(codexCommand(state, codex, env, ["mcp", "get", "origence", "--disable", "plugins", "--json"]));
}

function isExpectedMcp(server, url) {
  const transport = server?.transport;
  return server?.enabled === true && transport?.type === "streamable_http" && transport.url === url &&
    transport.bearer_token_env_var === "OC_API_KEY" && !transport.http_headers && !transport.http_headers_helper &&
    Object.entries(transport.env_http_headers ?? {}).every(([key, value]) =>
      key.toLowerCase() === "x-origence-workspace" && value === "OC_WORKSPACE_ID");
}

function mayRemoveOverride(record, server) {
  if (!record.override || !isExpectedMcp(server, record.override.url)) return false;
  if (record.override.digest) return fingerprint(server) === record.override.digest;
  // Recover the journal if setup exited just after 'mcp add'. A plain
  // installer-created override has no user options or extra headers.
  return !server.transport.env_http_headers &&
    ["enabled_tools", "disabled_tools", "startup_timeout_sec", "tool_timeout_sec"].every(key => server[key] == null);
}

export function connectCodex(state, codex, env, endpoint) {
  const safeEnv = withoutWorkspaceKey(env);
  const { plugins, markets } = codexInventory(state, codex, safeEnv);
  const existing = plugins.find(item => item.pluginId === plugin);
  const marketExists = markets.some(item => item.name === marketplace);
  const current = localMcp(state, codex, safeEnv);
  const url = endpoint + "/mcp";
  const needsOverride = endpoint !== defaultEndpoint;
  const record = state.value.codex ?? {
    executable: codex, configDir: env.CODEX_HOME ?? null, addedPlugin: !existing, addedMarket: !marketExists,
  };
  const ownedOverride = mayRemoveOverride(record, current);
  check(!current || isExpectedMcp(current, url) || ownedOverride,
    "Codex already has a different MCP server named origence. Update or remove that entry before running setup.");
  check(!existing || existing.enabled, "The Origence Codex plugin is disabled. Enable it in Codex before running setup.");

  record.executable = codex;
  state.value.codex = record;
  state.save();
  if (!marketExists) codexCommand(state, codex, safeEnv, ["plugin", "marketplace", "add", repo]);
  if (!existing) codexCommand(state, codex, safeEnv, ["plugin", "add", plugin]);

  if (needsOverride && !isExpectedMcp(current, url)) {
    if (ownedOverride) codexCommand(state, codex, safeEnv, ["mcp", "remove", "origence"]);
    record.override = { url };
    state.save();
    codexCommand(state, codex, safeEnv, ["mcp", "add", "origence", "--url", url, "--bearer-token-env-var", "OC_API_KEY"]);
    const added = localMcp(state, codex, safeEnv);
    check(isExpectedMcp(added, url), "Codex did not save the configured Origence endpoint.");
    record.override.digest = fingerprint(added);
    state.save();
  } else if (!needsOverride && ownedOverride) {
    codexCommand(state, codex, safeEnv, ["mcp", "remove", "origence"]);
    delete record.override;
    state.save();
  }

  const configured = JSON.parse(codexCommand(state, codex, safeEnv, ["mcp", "list", "--json"]));
  check(configured.some(server => server.name === "origence" && isExpectedMcp(server, url)),
    "Codex does not list an enabled Origence HTTP MCP server for this endpoint.");
  status("codex", "success", "Codex lists the verified Origence endpoint. Review the packaged hooks in /hooks.");
}

export function removeCodex(state, env) {
  const record = state.value.codex;
  if (!record) return;
  const safeEnv = withoutWorkspaceKey(env);
  if (record.configDir === null) delete safeEnv.CODEX_HOME;
  else safeEnv.CODEX_HOME = record.configDir;
  const codex = executable(record.executable);
  check(codex, "Codex is missing. Restore the executable to roll back plugin settings.");
  const { plugins, markets } = codexInventory(state, codex, safeEnv);
  const existing = plugins.find(item => item.pluginId === plugin);
  const current = localMcp(state, codex, safeEnv);
  if (mayRemoveOverride(record, current)) codexCommand(state, codex, safeEnv, ["mcp", "remove", "origence"]);
  else if (record.override && current) status("codex-mcp", "skipped", "Kept the Codex MCP entry modified after setup.");
  if (existing && record.addedPlugin) codexCommand(state, codex, safeEnv, ["plugin", "remove", plugin]);
  if (record.addedMarket && markets.some(item => item.name === marketplace) &&
      !plugins.some(item => item.marketplaceName === marketplace && !(item === existing && record.addedPlugin)))
    codexCommand(state, codex, safeEnv, ["plugin", "marketplace", "remove", marketplace]);
}
