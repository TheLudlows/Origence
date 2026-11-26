import { randomBytes, randomUUID } from "node:crypto";
import { closeSync, existsSync, openSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { delimiter, dirname, isAbsolute, join, relative, resolve } from "node:path";
import { createInterface } from "node:readline/promises";
import { Writable } from "node:stream";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import {
  atomicWrite, check, executable, execute, home, outsideRepository, prependPath,
  privateDirectory, restoreUserEnvironment, windows, windowsSettings, writeUserEnvironment,
} from "./installer/platform.mjs";
import { activeService, listening, startService, stopService, waitForService } from "./installer/service.mjs";
import { connectMcp, consoleLogin, endpointUrl, identity, isLoopback, uuid } from "./installer/protocol.mjs";

const repo = resolve(fileURLToPath(new URL("../", import.meta.url)));
const plugin = "origence@origence-tools";
const marketplace = "origence-tools";
const account = "admin";
const grantKey = "ORIGENCE_DEMO_KEY";
const defaultEndpoint = "http://127.0.0.1:8080";
const status = (step, state, detail) => console.log(JSON.stringify({ step, status: state, detail }));
const tokenPattern = /^[\x21-\x7e]{1,512}$/;

const usage = `Usage: origence-setup install [--endpoint URL] [--token KEY | --from-env]
       origence-setup install --init NAME [--endpoint URL]
       origence-setup uninstall

--from-env               Read OC_SERVER_URL and OC_API_KEY
--init NAME              Explicitly initialize a new local workspace
--force                  Refresh managed settings and reset the console password
                         Use with --init to back up and reinitialize local data
--data-dir PATH          Existing or explicitly initialized local backend data directory
--ui-port PORT           Console port on 127.0.0.1 (default 3000)
--origence PATH          Origence executable, if it is outside PATH and the checkout
--claude PATH            Claude Code executable, if it is outside PATH
--verify-write           Create one labeled test Run and read it through the UI account
--show-secrets           Print the current token and console password to stdout
--credentials-file PATH  Write a new private JSON file outside Git (never overwrites)`;

function options() {
  const args = process.argv.slice(2);
  const command = ["install", "uninstall"].includes(args[0]) ? args.shift() : undefined;
  const strings = ["endpoint", "token", "init", "data-dir", "ui-port", "origence", "claude", "credentials-file"];
  const bools = ["from-env", "force", "verify-write", "show-secrets", "uninstall", "help"];
  let values;
  try {
    ({ values } = parseArgs({ args, options: Object.fromEntries([
      ...strings.map(name => [name, { type: "string" }]),
      ...bools.map(name => [name, { type: "boolean" }]),
    ]) }));
  } catch {
    throw new Error("Invalid arguments. Run origence-setup --help.");
  }
  check(command !== "install" || !values.uninstall, "Use origence-setup uninstall to roll back the installation.");
  if (command === "uninstall" || values.uninstall) {
    check(Object.keys(values).every(name => name === "uninstall" || name === "help"),
      "Uninstall accepts no setup options.");
    values.uninstall = true;
  }
  return values;
}

async function ask(prompt, secret = false) {
  const output = secret ? new Writable({ write(_chunk, _encoding, done) { done(); } }) : process.stderr;
  const terminal = createInterface({ input: process.stdin, output, terminal: true });
  try {
    if (secret) process.stderr.write(prompt);
    return await terminal.question(secret ? "" : prompt, { signal: AbortSignal.timeout(300_000) });
  } finally {
    terminal.close();
    if (secret) process.stderr.write("\n");
  }
}

function stateDirectory() {
  const parent = windows ? process.env.LOCALAPPDATA || join(home, "AppData", "Local") : home;
  return join(parent, windows ? "Origence" : ".origence", "installer");
}

function installerState(dir) {
  const path = join(dir, "state.json");
  const value = existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : { version: 1 };
  check(value.version === 1, "This Origence installer state version is unsupported.");
  return { dir, path, value, save: () => atomicWrite(path, JSON.stringify(value, null, 2) + "\n") };
}

function lock(dir) {
  const file = join(dir, "install.lock");
  try { writeFileSync(file, String(process.pid), { flag: "wx", mode: 0o600 }); }
  catch (error) {
    if (error.code !== "EEXIST") throw error;
    throw new Error("Another installation is active. After confirming it exited, remove the installer directory's install.lock and retry.");
  }
  return () => rmSync(file, { force: true });
}

function withoutWorkspaceKey(env) {
  return Object.fromEntries(Object.entries(env).filter(([name]) =>
    !["OC_API_KEY", grantKey].includes(name.toUpperCase())));
}

function npm(...args) {
  const program = executable("npm");
  check(program, "npm is required to build the console or install Claude Code. Install Node.js 24 with npm.");
  execute(program, args, {
    cwd: repo, env: withoutWorkspaceKey(process.env), timeout: 300_000, diagnosis: "npm could not provision the dependency. Check network and registry access",
  });
}

async function prepareConsole() {
  const entry = join(repo, "ui", "dist", "server.js");
  if (!existsSync(entry) || !existsSync(join(repo, "ui", "dist", "public", "index.html"))) {
    npm("--prefix", "ui", "ci", "--ignore-scripts", "--no-audit", "--no-fund");
    npm("--prefix", "ui", "run", "build");
  }
  return import("../ui/dist/console-accounts.js");
}

function origenceBinary(selected) {
  if (selected) {
    const binary = executable(resolve(selected));
    check(binary, "The requested Origence executable could not be found. Check --origence.");
    return binary;
  }
  for (const file of [
    executable("origence"), executable(join(repo, "target", "release", "origence")),
    executable(join(repo, "target", "debug", "origence")),
  ]) if (file) return file;
  const cargo = executable("cargo");
  check(cargo, "Origence is missing. Provide --origence, or install the Rust toolchain and build dependencies.");
  const target = resolve(process.env.CARGO_TARGET_DIR ?? join(repo, "target"));
  execute(cargo, ["build", "--locked", "--bin", "origence", "--target-dir", target], {
    cwd: repo, env: withoutWorkspaceKey(process.env), timeout: 1_800_000, diagnosis: "Origence could not be built. Check the Rust toolchain, native build tools and Cargo registry",
  });
  const binary = executable(join(target, "debug", "origence"));
  check(binary, "Cargo did not produce the expected Origence binary. Pass its location with --origence.");
  return binary;
}

function claudeBinary(state, selected) {
  if (selected) {
    const binary = executable(resolve(selected));
    check(binary, "The requested Claude Code executable could not be found. Check --claude.");
    return binary;
  }
  const prefix = join(state.dir, "claude");
  const bin = join(prefix, "node_modules", ".bin", "claude");
  const installed = executable("claude") ?? executable(bin);
  if (installed) return installed;
  npm("install", "--prefix", prefix, "--no-audit", "--no-fund", "@anthropic-ai/claude-code@2.1.330");
  const binary = executable(bin);
  check(binary, "Claude Code was not installed. Install Claude Code or pass its native executable with --claude.");
  return binary;
}

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

function connectClaude(state, claude, env, endpoint) {
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

function removeClaude(state, env) {
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

function localDataDir(input, state, previous) {
  if (input) return resolve(input);
  if (previous) return previous.dataDir;
  if (process.env.OC_DATA_DIR) return resolve(process.env.OC_DATA_DIR);
  if (existsSync(join(repo, ".data", "context.db"))) return join(repo, ".data");
  return join(state.dir, "data");
}

function safeToBackup(path, state) {
  for (const preserve of [home, repo, state.dir]) {
    const below = relative(path, preserve);
    check(below && (below === ".." || below.startsWith(".." + (windows ? "\\" : "/")) || isAbsolute(below)),
      "Choose a dedicated backend directory. The installer cannot move a parent of your home, repository or installer state.");
  }
}

async function bootstrap(state, binary, endpoint, dataDir, name, force, env) {
  check(isLoopback(endpoint) && endpoint.protocol === "http:", "Initialization requires a local HTTP endpoint.");
  check(name.trim() && Buffer.byteLength(name) <= 256 && !name.includes("\0"),
    "Choose a workspace name between 1 and 256 UTF-8 bytes.");
  const previous = state.value.bootstrap;
  if (previous?.endpoint === endpoint.origin && previous.dataDir === dataDir && !force) return previous;
  if (state.value.config?.endpoint === endpoint.origin && force) await stopService(state, "api");
  check(!await listening(endpoint), "The endpoint is in use. Stop the external service before initializing or supply its existing token.");
  if (existsSync(dataDir)) {
    check(force, "The backend directory already exists. Supply its current key, or use --force --init to preserve it in a backup before creating a new workspace.");
    safeToBackup(dataDir, state);
    if (existsSync(join(dataDir, "context.db")))
      execute(binary, ["--data-dir", dataDir, "--offline", "identity-check"], {
        env, cwd: repo, diagnosis: "Cannot confirm the existing store is stopped and compatible. Stop all services using the directory",
      });
    const backup = dataDir + ".bak-" + new Date().toISOString().replaceAll(":", "-") + "-" + randomUUID().slice(0, 8);
    renameSync(dataDir, backup);
    status("backup", "success", backup);
  }
  const result = JSON.parse(execute(binary, ["--data-dir", dataDir, "--offline", "workspace-create", "--", name], {
    env, cwd: repo, diagnosis: "Offline workspace creation failed. Check data directory permissions and ensure no host is using it",
  }));
  check(uuid.test(result.workspace_id) && tokenPattern.test(result.token), "Workspace creation did not return a valid workspace and token.");
  state.value.bootstrap = { endpoint: endpoint.origin, dataDir, ...result };
  state.save();
  status("bootstrap", "success", result.workspace_id);
  return state.value.bootstrap;
}

async function ensureApi(state, endpoint, dataDir, binary, env) {
  if (!isLoopback(endpoint) || endpoint.protocol !== "http:") {
    await identityReady(endpoint.origin);
    status("api", "skipped", "Using the supplied running remote/TLS API.");
    return;
  }
  if (await listening(endpoint)) {
    await identityReady(endpoint.origin);
    status("api", "skipped", "Reusing the ready API listener.");
    return;
  }
  check(existsSync(join(dataDir, "context.db")),
    "The local API is unreachable and no initialized data was found. Start the existing API or supply its --data-dir. For a new instance, use --init.");
  await stopService(state, "api");
  const host = endpoint.hostname === "localhost" ? "127.0.0.1" : endpoint.hostname;
  const cleaned = { ...env };
  delete cleaned.OC_MCP_PUBLIC_ORIGIN;
  const bind = host + ":" + (endpoint.port || "80");
  const managed = startService(state, "api", binary, ["--data-dir", dataDir, "serve", "--local", "--bind", bind], repo, cleaned);
  await waitForService(endpoint.origin, "/health/ready", managed, "Origence API");
  state.value.apiEndpoint = endpoint.origin;
  state.save();
  status("api", "success", "Started the local API and verified /health/ready.");
}

async function identityReady(endpoint) {
  let response;
  try { response = await fetch(endpoint + "/health/ready", { redirect: "manual", signal: AbortSignal.timeout(5000) }); }
  catch { throw new Error("The Origence endpoint is unreachable. Check the listener, port and TLS configuration."); }
  const ready = response.status === 200 && (await response.json().catch(() => null))?.status === "ready";
  check(ready, "The endpoint is occupied or Origence is not ready. Check /health/ready and the service logs.");
}

async function ensureConsole(state, config, old, env) {
  const ui = config.ui;
  const url = new URL(ui);
  const needsRestart = state.value.uiBinding !== config.binding;
  if (needsRestart && state.value.services?.ui) await stopService(state, "ui");
  if (await listening(url)) {
    check(old?.ui === ui && !needsRestart && activeService(state, "ui"),
      "The console port is already in use by a service outside this installation. Select another --ui-port.");
    status("ui", "skipped", "Reusing the managed console listener.");
    return;
  }
  await stopService(state, "ui");
  const dir = startService(state, "ui", process.execPath, [
    join(repo, "ui", "dist", "server.js"), "--host", "127.0.0.1", "--port", url.port, "--strictPort",
  ], repo, env);
  await waitForService(ui, "/", dir, "Origence console");
  state.value.uiBinding = config.binding;
  state.save();
  status("ui", "success", "Started the local browser console.");
}

function saveCredentials(path, credentials) {
  const file = resolve(path);
  outsideRepository(file);
  closeSync(openSync(file, "wx", 0o600));
  try {
    if (windows) windowsSettings({ action: "protect", path: file, file: true });
    writeFileSync(file, JSON.stringify(credentials, null, 2) + "\n");
  } catch (error) {
    rmSync(file, { force: true });
    throw error;
  }
  status("credentials-file", "success", "Private credential file written. Keep it outside Git and do not share it.");
}

async function uninstall(state) {
  const db = join(state.dir, "console-accounts.db");
  const failures = [];
  const rollback = {
    ui: () => stopService(state, "ui"),
    api: () => stopService(state, "api"),
    claude: () => removeClaude(state, process.env),
    environment: () => restoreUserEnvironment(state),
    account: async () => {
      if (state.value.accountOwned && existsSync(db)) {
        const accounts = await prepareConsole();
        accounts.configureConsoleAccount(db, { kind: "remove-account", username: account });
      }
    },
  };
  for (const [step, undo] of Object.entries(rollback)) {
    try { await undo(); }
    catch (error) {
      failures.push(step);
      status("rollback-" + step, "failure", error.message);
    }
  }
  check(!failures.length, "Uninstall kept the rollback journal. Resolve the reported steps and retry.");
  rmSync(state.path, { force: true });
  status("uninstall", "success", "Restored installer-owned settings and stopped its processes. Backend data, backups and explicitly saved credential files are preserved.");
}

async function install(state, args) {
  const old = state.value.config;
  const interactive = process.stdin.isTTY && !args["from-env"];
  if (args["from-env"]) check(!args.endpoint && !args.token, "Use --from-env separately from --endpoint and --token.");
  const endpointInput = args.endpoint ?? (args["from-env"] ? process.env.OC_SERVER_URL : old?.endpoint ?? process.env.OC_SERVER_URL)
    ?? (interactive ? (await ask("Origence API endpoint [" + defaultEndpoint + "]: ")).trim() || defaultEndpoint : defaultEndpoint);
  const endpoint = endpointUrl(endpointInput);
  const sameEndpoint = old?.endpoint === endpoint.origin;
  const dataDir = localDataDir(args["data-dir"], state, sameEndpoint ? old : null);
  if (args["credentials-file"]) {
    const file = resolve(args["credentials-file"]);
    outsideRepository(file);
    check(!existsSync(file) && existsSync(dirname(file)) && statSync(dirname(file)).isDirectory(),
      "Choose a new credential file with an existing parent directory.");
  }
  const port = Number(args["ui-port"] ?? (sameEndpoint ? new URL(old.ui).port : 3000));
  check(Number.isInteger(port) && port > 0 && port <= 65535, "Specify a console port between 1 and 65535.");
  const ui = "http://127.0.0.1:" + port;
  check(!isLoopback(endpoint) || Number(endpoint.port || (endpoint.protocol === "http:" ? 80 : 443)) !== port,
    "The API and console need different ports. Choose another --ui-port.");
  const explicitToken = args.token ?? (args["from-env"] ? process.env.OC_API_KEY : undefined);
  check(!args.init || !explicitToken, "Specify either a token or --init.");
  let token = explicitToken ?? (sameEndpoint ? old.token : undefined);
  let init = args.init;
  const recovered = state.value.bootstrap;
  if (!token && recovered?.endpoint === endpoint.origin && recovered.dataDir === dataDir) token = recovered.token;
  if (!token && !init && interactive) {
    token = (await ask("Workspace API key (leave empty to initialize locally): ", true)).trim();
    if (!token && isLoopback(endpoint) && !await listening(endpoint) && !existsSync(dataDir))
      init = (await ask("New workspace name (leave empty to cancel): ")).trim();
  }
  const initialize = init && (!sameEndpoint || !old?.initialized || args.force);
  if (old && sameEndpoint && isLoopback(endpoint) && dataDir !== old.dataDir && !initialize)
    check(!await listening(endpoint), "Stop the local API before selecting a different --data-dir.");
  check(initialize || token, "A workspace API key is missing. Use --token, --from-env or --init for a fresh local instance.");
  check(!token || tokenPattern.test(token), "The workspace token is not valid printable credential text.");
  check(!old || args.force || sameEndpoint && old.token === token && old.ui === ui && old.dataDir === dataDir,
    "This installation is bound to another endpoint, key or local path. Use --force to update it.");
  if (await listening(new URL(ui)))
    check(old?.ui === ui && activeService(state, "ui"),
      "The console port is already in use by a service outside this installation. Select another --ui-port.");
  const currentProfile = process.env.CLAUDE_CONFIG_DIR ? resolve(process.env.CLAUDE_CONFIG_DIR) : null;
  check(!state.value.claude || state.value.claude.configDir === currentProfile,
    "Use the Claude profile used for this installation, or uninstall before selecting another profile.");

  const binary = origenceBinary(args.origence ?? old?.binary);
  const claude = claudeBinary(state, args.claude ?? state.value.claude?.executable);
  const launchers = join(repo, "bin");
  check(executable(join(launchers, "origence-setup")), "The installer package is missing its origence-setup launcher.");
  const dirs = [...new Set([launchers, dirname(binary), dirname(claude)])];
  const path = prependPath(process.env.PATH, dirs);
  check(executable("origence", path) === binary, "Select a binary named origence (origence.exe on Windows) for the plugin hook PATH.");
  const env = { ...withoutWorkspaceKey(process.env), PATH: path };
  if (currentProfile) env.CLAUDE_CONFIG_DIR = currentProfile;
  execute(binary, ["--version"], { env, diagnosis: "Origence could not be executed" });
  execute(binary, ["plugin-hook", "--help"], { env, diagnosis: "The Origence binary does not support Claude Code hooks" });
  status("executables", "success", "Origence and Claude Code are available.");
  if (initialize) {
    const bootstrapResult = await bootstrap(state, binary, endpoint, dataDir, init, args.force, env);
    token = bootstrapResult.token;
    if (interactive && !bootstrapResult.announced && !args["show-secrets"]) {
      console.log(JSON.stringify({ endpoint: endpoint.origin, workspace_id: bootstrapResult.workspace_id, token }));
      bootstrapResult.announced = true;
      state.save();
    }
  } else if (init) status("bootstrap", "skipped", "Reusing the initialized workspace.");
  await ensureApi(state, endpoint, dataDir, binary, env);
  const who = await identity(endpoint.origin, token);
  if (state.value.bootstrap?.token === token)
    check(who.workspace_id === state.value.bootstrap.workspace_id, "The initialized API resolved to a different workspace.");
  status("whoami", "success", { workspace_id: who.workspace_id, role: who.role });
  const mcp = await connectMcp(endpoint.origin, token, who.workspace_id, who.id);
  if (old && !sameEndpoint && state.value.apiEndpoint !== endpoint.origin) await stopService(state, "api");
  status("http-mcp", "success", { tools: mcp.toolCount, workspace_id: who.workspace_id });

  const reconfigure = !old || args.force || old.workspace !== who.workspace_id || old.key !== who.id || old.endpoint !== endpoint.origin || old.ui !== ui;
  const config = { endpoint: endpoint.origin, token, workspace: who.workspace_id, key: who.id, ui,
    password: old && !args.force ? old.password : randomBytes(24).toString("base64url"),
    binding: reconfigure ? randomUUID() : old.binding,
    dataDir, binary, initialized: Boolean(initialize || recovered?.token === token || sameEndpoint && old?.initialized) };
  if (reconfigure && await listening(new URL(ui)) && !(old?.ui === ui && activeService(state, "ui")))
    throw new Error("The console port is already in use. Select another --ui-port.");
  state.value.config = config;
  delete state.value.bootstrap;
  state.save();

  const db = join(state.dir, "console-accounts.db");
  const accounts = await prepareConsole();
  if (state.value.accountBinding !== config.binding || !existsSync(db)) {
    accounts.prepareConsoleAccounts(db);
    state.value.accountOwned = true;
    state.save();
    accounts.configureConsoleAccount(db, { kind: "set-password", username: account,
      passwordHash: await accounts.hashConsolePassword(config.password) });
    if (state.value.accountWorkspace && state.value.accountWorkspace !== who.workspace_id)
      accounts.configureConsoleAccount(db, { kind: "revoke", username: account, workspaceId: state.value.accountWorkspace });
    accounts.configureConsoleAccount(db, { kind: "grant", username: account,
      workspaceId: who.workspace_id, name: "Origence workspace", keyEnv: grantKey });
    state.value.accountBinding = config.binding;
    state.value.accountWorkspace = who.workspace_id;
    state.save();
  }
  const vars = { OC_API_KEY: token, OC_SERVER_URL: endpoint.origin, OC_WORKSPACE_ID: who.workspace_id };
  const uiEnv = { ...env, ORIGENCE_UI_ACCOUNTS_DB: db, ORIGENCE_API_BASE_URL: endpoint.origin, [grantKey]: token };
  delete uiEnv.ORIGENCE_UI_ORIGIN;
  await ensureConsole(state, config, old, uiEnv);
  const consoleSession = await consoleLogin(ui, account, config.password, config.workspace, config.key);
  try {
    status("console-account", "success", { account, workspace_id: config.workspace, role: who.role });
    if (args["verify-write"] && ["writer", "reviewer", "admin"].includes(who.role)) {
      state.value.probes ??= {};
      const scope = [endpoint.origin, who.workspace_id, who.id].join("/");
      const run = state.value.probes[scope] ??= randomUUID();
      state.save();
      await mcp.call("context_run_create", { run_id: run, summary: "Origence installer connectivity check" });
      const read = await consoleSession.read("/v1/runs/" + run);
      check(read.run?.run_id === run, "The console did not return the written Run.");
      status("write-check", "success", { run_id: run, url: ui + "/#runs/" + run });
    } else status("write-check", "skipped", args["verify-write"] ? "A writer or admin key is required." : "Use --verify-write to record one labeled test Run.");
  } finally {
    await consoleSession.logout().catch(() => {});
  }

  connectClaude(state, claude, { ...env, ...vars }, endpoint.origin);
  if (env.OC_PLUGIN_RUN_HOOK === "0")
    status("run-hook", "skipped", "This launch environment disables Run reporting with OC_PLUGIN_RUN_HOOK=0.");
  else if (who.role === "reader")
    status("run-hook", "skipped", "The plugin includes the Run hook. This reader key cannot report Runs.");
  else status("run-hook", "success", "The installed plugin includes the default Run hook. Use Claude Code to generate a Run.");
  writeUserEnvironment(state, vars, dirs);
  status("user-environment", "success", "Saved user environment and PATH. Restart your terminal and Claude Code. To roll back, run origence-setup uninstall.");
  const credentials = { endpoint: endpoint.origin, workspace_id: who.workspace_id, role: who.role, ui,
    username: account, password: config.password, token };
  if (args["credentials-file"]) saveCredentials(args["credentials-file"], credentials);
  if (args["show-secrets"] || interactive) {
    const displayed = { ...credentials };
    if (!args["show-secrets"]) delete displayed.token;
    status("installation", "success", displayed);
  } else {
    status("installation", "success", { endpoint: endpoint.origin, workspace_id: who.workspace_id, role: who.role,
      ui, username: account, credentials: "Use --show-secrets or --credentials-file to retrieve the login password and key." });
  }
}

try {
  const args = options();
  if (args.help) console.log(usage);
  else {
    check(Number(process.versions.node.split(".")[0]) >= 24, "Use Node.js 24 or later.");
    const dir = stateDirectory();
    outsideRepository(dir);
    if (args.uninstall && !existsSync(join(dir, "state.json"))) status("uninstall", "skipped", "No installer settings were found.");
    else {
      privateDirectory(dir);
      const release = lock(dir);
      try {
        const state = installerState(dir);
        if (args.uninstall) await uninstall(state);
        else await install(state, args);
      } finally { release(); }
    }
  }
} catch (error) {
  status("installation", "failure", error.message);
  process.exitCode = 1;
}
