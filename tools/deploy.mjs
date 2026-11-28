import { randomBytes, randomUUID } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { check, execute } from "./installer/platform.mjs";
import { activeService, listening, startService, stopService, waitForService } from "./installer/service.mjs";
import { connectMcp, consoleLogin, endpointUrl, identity, isLoopback } from "./installer/protocol.mjs";
import {
  ask, credentialsPath, defaultEndpoint, npm, origenceBinary, repo, rollback, runInstaller,
  saveCredentials, status, tokenPattern, withoutWorkspaceKey,
} from "./installer/command.mjs";

const account = "admin";
const usage = `Usage: origence-deploy install [--endpoint URL] [--token KEY | --from-env]
       origence-deploy uninstall

--from-env               Read OC_SERVER_URL and OC_API_KEY
--force                  Refresh deployment settings and reset a linked account password
--data-dir PATH          Local backend data directory
--ui-port PORT           Console port on 127.0.0.1 (default 3000)
--origence PATH          Origence executable, if it is outside PATH and the checkout
--verify-write           Create one labeled test Run and read it through the console
--show-secrets           Print console and supplied workspace credentials to stdout
--credentials-file PATH  Write a new private JSON file outside Git (never overwrites)`;

function prepareConsole() {
  const entry = join(repo, "ui", "dist", "server.js");
  if (!existsSync(entry) || !existsSync(join(repo, "ui", "dist", "public", "index.html"))) {
    npm("--prefix", "ui", "ci", "--ignore-scripts", "--no-audit", "--no-fund");
    npm("--prefix", "ui", "run", "build");
  }
}

function localDataDir(input, state, previous) {
  if (input) return resolve(input);
  if (previous) return previous.dataDir;
  if (process.env.OC_DATA_DIR) return resolve(process.env.OC_DATA_DIR);
  if (existsSync(join(repo, ".data", "context.db"))) return join(repo, ".data");
  return join(state.dir, "data");
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
        prepareConsole();
        const accounts = await import("../ui/dist/console-accounts.js");
        accounts.configureConsoleAccount(db, { kind: "remove-account", username: account });
      }
    },
  }, "Stopped the deployment's services and removed any managed console account. Backend data and backups are preserved.");
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
  const local = isLoopback(endpoint) && endpoint.protocol === "http:";
  const token = args.token ?? (args["from-env"] ? process.env.OC_API_KEY : undefined)
    ?? (sameEndpoint ? old.token : undefined)
    ?? (interactive && !local ? (await ask("Workspace API key (optional with a deployment key): ", true)).trim() || undefined : undefined);
  check(!token || tokenPattern.test(token), "The workspace token is not valid printable credential text.");
  if (old && sameEndpoint && isLoopback(endpoint) && dataDir !== old.dataDir)
    check(!await listening(endpoint), "Stop the local API before selecting a different --data-dir.");
  check(!old || args.force || sameEndpoint && old.token === token && old.ui === ui && old.dataDir === dataDir,
    "This installation is bound to another endpoint, key or local path. Use --force to update it.");
  if (await listening(new URL(ui)))
    check(old?.ui === ui && activeService(state, "ui"),
      "The console port is already in use by a service outside this installation. Select another --ui-port.");
  const binary = origenceBinary(args.origence ?? old?.binary);
  const env = withoutWorkspaceKey(process.env);
  const suppliedDeploymentKey = env.ORIGENCE_UI_DEPLOYMENT_ADMIN_KEY || env.OC_DEPLOYMENT_ADMIN_KEY || undefined;
  delete env.ORIGENCE_UI_DEPLOYMENT_ADMIN_KEY;
  execute(binary, ["--version"], { env, diagnosis: "Origence could not be executed" });
  status("executables", "success", "Origence is available.");
  await ensureApi(state, endpoint, dataDir, binary, env);
  const managedKeyFile = join(dataDir, "deployment-admin.key");
  const managed = local && activeService(state, "api") && state.value.apiEndpoint === endpoint.origin;
  const deploymentKey = suppliedDeploymentKey
    ?? (sameEndpoint && old?.dataDir === dataDir ? old.deploymentKey : undefined)
    ?? (!token && managed && existsSync(managedKeyFile) ? readFileSync(managedKeyFile, "utf8").trimEnd() : undefined);
  check(token || deploymentKey, "Supply a workspace API key or a deployment administrator credential.");
  if (deploymentKey) {
    check(/^oc_admin_[a-f0-9]{64}$/.test(deploymentKey), "Invalid deployment administrator credential format.");
    const response = await fetch(new URL("/admin/deployment/me", endpoint), {
      headers: { Authorization: `Bearer ${deploymentKey}` }, redirect: "manual", signal: AbortSignal.timeout(5000),
    });
    check(response.ok && (await response.json())?.role === "deployment_admin",
      "The backend did not authorize the deployment administrator credential.");
  }
  const who = token ? await identity(endpoint.origin, token) : null;
  if (who) check(who.role === "workspace", "Use the workspace Key for the selected workspace.");
  const mcp = who ? await connectMcp(endpoint.origin, token, who.workspace_id) : null;
  if (old && !sameEndpoint && state.value.apiEndpoint !== endpoint.origin) await stopService(state, "api");
  if (who) {
    status("whoami", "success", { workspace_id: who.workspace_id, role: who.role });
    status("http-mcp", "success", { tools: mcp.toolCount, workspace_id: who.workspace_id });
  }

  const mode = deploymentKey ? "deployment" : "workspace";
  const reconfigure = !old || args.force || old.mode !== mode || old.deploymentKey !== deploymentKey || old.workspace !== who?.workspace_id || old.key !== who?.id || old.endpoint !== endpoint.origin || old.ui !== ui;
  const config = { endpoint: endpoint.origin, token, workspace: who?.workspace_id, key: who?.id, ui,
    mode,
    ...(mode !== "workspace" ? { password: !args.force && old?.password || randomBytes(24).toString("base64url") } : {}),
    ...(deploymentKey ? { deploymentKey } : {}),
    binding: reconfigure ? randomUUID() : old.binding,
    dataDir, binary };
  if (reconfigure && await listening(new URL(ui)) && !(old?.ui === ui && activeService(state, "ui")))
    throw new Error("The console port is already in use. Select another --ui-port.");
  state.value.config = config;
  state.save();

  const db = join(state.dir, "console-accounts.db");
  prepareConsole();
  if (mode === "workspace") {
    if (state.value.accountOwned) {
      if (existsSync(db)) {
        const accounts = await import("../ui/dist/console-accounts.js");
        accounts.configureConsoleAccount(db, { kind: "remove-account", username: account });
      }
      delete state.value.accountOwned;
      delete state.value.accountBinding;
      delete state.value.accountWorkspace;
      state.save();
    }
  } else if (state.value.accountBinding !== config.binding || !existsSync(db)) {
    const accounts = await import("../ui/dist/console-accounts.js");
    accounts.prepareConsoleAccounts(db);
    state.value.accountOwned = true;
    state.save();
    accounts.configureConsoleAccount(db, { kind: "set-password", username: account,
      passwordHash: await accounts.hashConsolePassword(config.password) });
    state.value.accountBinding = config.binding;
    state.save();
  }
  const uiEnv = { ...env, ORIGENCE_API_BASE_URL: endpoint.origin };
  delete uiEnv.OC_DEPLOYMENT_ADMIN_KEY;
  delete uiEnv.ORIGENCE_UI_DEPLOYMENT_ADMIN_KEY;
  if (mode !== "workspace") {
    uiEnv.ORIGENCE_UI_ACCOUNTS_DB = db;
    uiEnv.ORIGENCE_UI_DEPLOYMENT_ADMIN_KEY = deploymentKey;
  }
  delete uiEnv.ORIGENCE_UI_ORIGIN;
  await ensureConsole(state, config, old, uiEnv);
  const login = mode === "workspace" ? { workspace_key: token } : { username: account, password: config.password };
  const consoleSession = await consoleLogin(ui, login, config.workspace, config.key, mode);
  try {
    status("console-session", "success", { mode, workspace_id: config.workspace, role: who?.role ?? "deployment_admin" });
    if (deploymentKey) status("deployment-admin-session", "success", "Verified the deployment administrator account.");
    if (args["verify-write"] && mcp) {
      state.value.probes ??= {};
      const scope = [endpoint.origin, who.workspace_id, who.id].join("/");
      const run = state.value.probes[scope] ??= randomUUID();
      state.save();
      await mcp.call("context_run_create", { run_id: run, summary: "Origence installer connectivity check" });
      const read = await consoleSession.read(mode === "deployment"
        ? `/admin/deployment/workspaces/${config.workspace}/runs/${run}`
        : "/v1/runs/" + run);
      check(read.run?.run_id === run, "The console did not return the written Run.");
      status("write-check", "success", { run_id: run, url: mode === "deployment" ? ui : ui + "/#runs/" + run });
    } else status("write-check", "skipped", "Use --verify-write to record one labeled test Run.");
  } finally {
    await consoleSession.logout().catch(() => {});
  }

  const credentials = { endpoint: endpoint.origin, workspace_id: who?.workspace_id, role: who?.role ?? "deployment_admin", ui,
    login_mode: mode, token, ...(mode !== "workspace" ? login : {}),
    ...(deploymentKey ? { deployment_admin_key: deploymentKey } : {}) };
  if (credentialFile) saveCredentials(credentialFile, credentials);
  if (args["show-secrets"] || interactive) {
    const displayed = { ...credentials };
    if (!args["show-secrets"]) {
      delete displayed.token;
      delete displayed.deployment_admin_key;
    }
    status("installation", "success", displayed);
  } else {
    status("installation", "success", { endpoint: endpoint.origin, workspace_id: who?.workspace_id, role: who?.role ?? "deployment_admin",
      ui, login_mode: mode, credentials: "Use --show-secrets or --credentials-file to obtain console credentials." });
  }
  delete state.value.bootstrap;
  state.save();
}

await runInstaller({
  name: "origence-deploy", scope: "deploy", usage, install, uninstall,
  strings: ["endpoint", "token", "data-dir", "ui-port", "origence", "credentials-file"],
  flags: ["verify-write", "show-secrets"],
});
