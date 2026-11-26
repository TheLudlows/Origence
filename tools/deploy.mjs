import { randomBytes, randomUUID } from "node:crypto";
import { existsSync, renameSync } from "node:fs";
import { isAbsolute, join, relative, resolve } from "node:path";
import { check, execute, home, windows } from "./installer/platform.mjs";
import { activeService, listening, startService, stopService, waitForService } from "./installer/service.mjs";
import { connectMcp, consoleLogin, endpointUrl, identity, isLoopback, uuid } from "./installer/protocol.mjs";
import {
  ask, credentialsPath, defaultEndpoint, grantKey, npm, origenceBinary, repo, rollback, runInstaller,
  saveCredentials, status, tokenPattern, withoutWorkspaceKey,
} from "./installer/command.mjs";

const account = "admin";
const usage = `Usage: origence-deploy install [--endpoint URL] [--token KEY | --from-env]
       origence-deploy install --init NAME [--endpoint URL]
       origence-deploy uninstall

--from-env               Read OC_SERVER_URL and OC_API_KEY
--init NAME              Explicitly initialize a new local workspace
--force                  Refresh deployment settings and reset the console password
                         Use with --init to back up and reinitialize local data
--data-dir PATH          Existing or explicitly initialized local backend data directory
--ui-port PORT           Console port on 127.0.0.1 (default 3000)
--origence PATH          Origence executable, if it is outside PATH and the checkout
--verify-write           Create one labeled test Run and read it through the UI account
--show-secrets           Print the current token and console password to stdout
--credentials-file PATH  Write a new private JSON file outside Git (never overwrites)`;

async function prepareConsole() {
  const entry = join(repo, "ui", "dist", "server.js");
  if (!existsSync(entry) || !existsSync(join(repo, "ui", "dist", "public", "index.html"))) {
    npm("--prefix", "ui", "ci", "--ignore-scripts", "--no-audit", "--no-fund");
    npm("--prefix", "ui", "run", "build");
  }
  return import("../ui/dist/console-accounts.js");
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

async function uninstall(state) {
  const db = join(state.dir, "console-accounts.db");
  await rollback(state, {
    ui: () => stopService(state, "ui"),
    api: () => stopService(state, "api"),
    account: async () => {
      if (state.value.accountOwned && existsSync(db)) {
        const accounts = await prepareConsole();
        accounts.configureConsoleAccount(db, { kind: "remove-account", username: account });
      }
    },
  }, "Stopped the deployment's services and removed its console account. Backend data and backups are preserved.");
}

async function install(state, args) {
  const old = state.value.config;
  const interactive = process.stdin.isTTY && !args["from-env"];
  const endpointInput = args.endpoint ?? (args["from-env"] ? process.env.OC_SERVER_URL : old?.endpoint ?? process.env.OC_SERVER_URL)
    ?? (interactive ? (await ask("Origence API endpoint [" + defaultEndpoint + "]: ")).trim() || defaultEndpoint : defaultEndpoint);
  const endpoint = endpointUrl(endpointInput);
  const sameEndpoint = old?.endpoint === endpoint.origin;
  const dataDir = localDataDir(args["data-dir"], state, sameEndpoint ? old : null);
  const credentialFile = credentialsPath(args["credentials-file"]);
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
  const binary = origenceBinary(args.origence ?? old?.binary);
  const env = withoutWorkspaceKey(process.env);
  execute(binary, ["--version"], { env, diagnosis: "Origence could not be executed" });
  status("executables", "success", "Origence is available.");
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

  const credentials = { endpoint: endpoint.origin, workspace_id: who.workspace_id, role: who.role, ui,
    username: account, password: config.password, token };
  if (credentialFile) saveCredentials(credentialFile, credentials);
  if (args["show-secrets"] || interactive) {
    const displayed = { ...credentials };
    if (!args["show-secrets"]) delete displayed.token;
    status("installation", "success", displayed);
  } else {
    status("installation", "success", { endpoint: endpoint.origin, workspace_id: who.workspace_id, role: who.role,
      ui, username: account, credentials: "Use --show-secrets or --credentials-file to retrieve the login password and key." });
  }
}

await runInstaller({
  name: "origence-deploy", scope: "deploy", usage, install, uninstall,
  strings: ["endpoint", "token", "init", "data-dir", "ui-port", "origence", "credentials-file"],
  flags: ["verify-write", "show-secrets"],
});
