import { createServer } from "node:http";
import { once } from "node:events";
import { execFileSync } from "node:child_process";
import { test } from "node:test";
import assert from "node:assert/strict";
import { createUiServer } from "../dist/server.js";
import {
  configureConsoleAccount,
  hashConsolePassword,
  initializeConsoleAccounts,
  loadConsoleAccounts,
} from "../dist/console-accounts.js";
import { DatabaseSync } from "node:sqlite";
import { cp, mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const a = "11111111-1111-4111-8111-111111111111";
const b = "22222222-2222-4222-8222-222222222222";
const adminKey = "oc_admin_" + "b".repeat(64);
const alicePassword = "fixture-Al!ce-密码-15";
const bobPassword = "fixture-Bob-密码-15";
const sessionPath = "/api/console/session";

async function listen(server, t) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => {
    server.closeAllConnections();
    server.close();
  });
  return `http://127.0.0.1:${server.address().port}`;
}

async function accountDatabase(t) {
  const dir = await mkdtemp(join(tmpdir(), "origence-console-config-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const db = join(dir, "accounts.db");
  initializeConsoleAccounts(db);
  return db;
}

test("default console account login needs no database path or initialization command", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "origence-console-default-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  for (const folder of ["dist", "scripts"]) await mkdir(join(dir, "ui", folder), { recursive: true });
  const accountsModule = join(dir, "ui", "dist", "console-accounts.js");
  const accountsCli = join(dir, "ui", "scripts", "console-accounts.mjs");
  await cp(new URL("../dist/console-accounts.js", import.meta.url), accountsModule);
  await cp(new URL("../scripts/console-accounts.mjs", import.meta.url), accountsCli);
  await cp(new URL("../package.json", import.meta.url), join(dir, "ui", "package.json"));
  const { loadConfiguredConsoleAccounts } = await import(pathToFileURL(accountsModule).href);
  assert.deepEqual(loadConfiguredConsoleAccounts({}), []);
  assert.throws(() => loadConfiguredConsoleAccounts({ ORIGENCE_UI_DEPLOYMENT_ADMIN_KEY: adminKey, ORIGENCE_UI_ACCOUNTS_DB: join(dir, "wrong-path.db") }), { code: "ENOENT" });

  const ui = await listen(createUiServer({ apiBaseUrl: "http://127.0.0.1:1" }), t);
  assert.deepEqual(await (await fetch(ui + sessionPath)).json(), {
    mode: "account", session: null, setupRequired: true,
  });
  const signIn = (origin, credentials) => fetch(origin + sessionPath, {
    method: "POST",
    headers: { Origin: origin, "Content-Type": "application/json" },
    body: JSON.stringify(credentials),
  });
  assert.equal((await signIn(ui, { key: "fixture-key" })).status, 400);
  const credentials = { username: "admin", password: alicePassword };
  assert.equal((await signIn(ui, credentials)).status, 503);

  const env = { ...process.env };
  delete env.ORIGENCE_UI_ACCOUNTS_DB;
  execFileSync(process.execPath, [accountsCli, "set-password", credentials.username], {
    cwd: dir, env, input: credentials.password + "\n",
  });
  const accounts = loadConfiguredConsoleAccounts({ ORIGENCE_UI_DEPLOYMENT_ADMIN_KEY: adminKey });
  const upstream = await listen(createServer((req, res) => {
    assert.equal(req.url, "/admin/deployment/me");
    assert.equal(req.headers.authorization, `Bearer ${adminKey}`);
    res.end(JSON.stringify({ role: "deployment_admin" }));
  }), t);
  const configured = await listen(createUiServer({ apiBaseUrl: upstream, accounts }), t);
  const login = await signIn(configured, credentials);
  assert.equal(login.status, 200);
  assert.equal((await login.json()).session.account, credentials.username);
});

test("a workspace Key signs in without an account and stays scoped after reload", async (t) => {
  const key = "oc_" + "a".repeat(64);
  let active = true;
  const seen = [];
  const upstream = await listen(createServer((req, res) => {
    seen.push(req.url);
    res.setHeader("Content-Type", "application/json");
    if (!active || req.headers.authorization !== `Bearer ${key}`) {
      res.statusCode = 401;
      return res.end("{}");
    }
    if (req.url === "/admin/agents") {
      assert.equal(req.headers["x-origence-workspace"], a);
      return res.end(JSON.stringify({ workspace_id: a, items: [] }));
    }
    res.end(JSON.stringify({ id: b, workspace_id: a, role: "workspace" }));
  }), t);
  const ui = await listen(createUiServer({ apiBaseUrl: upstream }), t);
  const signedIn = await fetch(ui + sessionPath, {
    method: "POST",
    headers: { Origin: ui, "Content-Type": "application/json" },
    body: JSON.stringify({ workspace_key: key }),
  });
  assert.equal(signedIn.status, 200);
  const cookie = signedIn.headers.get("Set-Cookie").split(";")[0];
  const payload = await signedIn.text();
  assert.ok(!payload.includes(key) && !cookie.includes(key));
  const { mode, session } = JSON.parse(payload);
  assert.equal(mode, "workspace");
  assert.deepEqual(session.workspaces.map((value) => value.workspace_id), [a]);
  assert.deepEqual(seen, ["/v1/whoami"]);
  const request = (path, workspace = a) => fetch(ui + path, {
    headers: { Cookie: cookie, "X-Origence-Session": session.binding, "X-Origence-Workspace": workspace },
  });
  assert.equal((await request("/api/v1/assets", b)).status, 403);
  assert.deepEqual(await (await request("/api/admin/agents")).json(), { workspace_id: a, items: [] });
  assert.equal((await request("/api/admin/agents", b)).status, 403);
  assert.equal((await request("/api/admin/keys")).status, 404);
  assert.equal((await request("/api/admin/deployment/workspaces")).status, 403);
  assert.equal((await fetch(`${ui}/api/admin/deployment/global-knowledge/${a}/review`, {
    method: "POST",
    headers: { Origin: ui, Cookie: cookie, "X-Origence-Session": session.binding, "Content-Type": "application/json" },
    body: JSON.stringify({ decision: "approve" }),
  })).status, 403);
  assert.equal((await request("/api/v1/assets")).status, 200);
  assert.deepEqual((await (await request(sessionPath)).json()).session, session);
  active = false;
  assert.equal((await request("/api/v1/assets")).status, 401);
  assert.equal((await (await request(sessionPath)).json()).session, null);
});

test("administrator account sessions survive reload and enforce revocation and sign-out", async (t) => {
  let active = true;
  const upstream = await listen(createServer((req, res) => {
    assert.equal(req.headers.authorization, `Bearer ${adminKey}`);
    assert.equal(req.headers.cookie, undefined);
    if (!active) {
      res.statusCode = 401;
      return res.end("{}");
    }
    res.end(JSON.stringify(req.url === "/admin/deployment/me" ? { role: "deployment_admin" } : { items: [] }));
  }), t);
  const account = { username: "admin", passwordHash: await hashConsolePassword(alicePassword), deploymentAuthorization: `Bearer ${adminKey}` };
  const ui = await listen(createUiServer({ apiBaseUrl: upstream, accounts: [account] }), t);
  const signIn = (data, origin = ui) => fetch(ui + sessionPath, {
    method: "POST", headers: { Origin: origin, "Content-Type": "application/json" }, body: JSON.stringify(data),
  });
  const credentials = { username: "admin", password: alicePassword };
  assert.equal((await signIn({ deployment_key: adminKey })).status, 400);
  assert.equal((await signIn({ ...credentials, password: "incorrect" })).status, 401);
  assert.equal((await signIn(credentials, "https://another.example")).status, 403);
  const login = await signIn(credentials);
  assert.equal(login.status, 200);
  const cookieHeader = login.headers.get("set-cookie");
  assert.match(cookieHeader, /Path=\/api; Max-Age=28800; HttpOnly; SameSite=Strict/);
  const cookie = cookieHeader.split(";")[0];
  const payload = await login.text();
  assert.ok(!payload.includes(adminKey) && !cookie.includes(adminKey));
  const profile = JSON.parse(payload);
  assert.equal(profile.mode, "deployment");
  assert.deepEqual(profile.session.workspaces, []);
  const headers = { Cookie: cookie, "X-Origence-Session": profile.session.binding };
  const resume = () => fetch(ui + sessionPath, { headers });
  assert.deepEqual(await (await resume()).json(), profile);
  const adminPath = ui + "/api/admin/deployment/workspaces";
  assert.equal((await fetch(adminPath, { headers })).status, 200);
  assert.equal((await fetch(ui + "/api/v1/assets", { headers: { ...headers, "X-Origence-Workspace": a } })).status, 403);
  active = false;
  assert.equal((await fetch(adminPath, { headers })).status, 401);
  assert.equal((await (await resume()).json()).session, null);
  const logout = await fetch(ui + sessionPath, { method: "DELETE", headers: { Origin: ui, Cookie: cookie } });
  assert.match(logout.headers.get("set-cookie"), /Max-Age=0/);
  assert.equal((await fetch(adminPath, { headers })).status, 401);
});

test("configuration tables retain only account credentials across reopen", async (t) => {
  const database = await accountDatabase(t);
  assert.throws(() => initializeConsoleAccounts(database), { code: "EEXIST" });
  assert.deepEqual(loadConsoleAccounts(database), []);
  const original = await hashConsolePassword(alicePassword);
  const replacement = await hashConsolePassword(bobPassword);
  configureConsoleAccount(database, { kind: "set-password", username: "alice", passwordHash: original });
  configureConsoleAccount(database, { kind: "set-password", username: "alice", passwordHash: replacement });
  const db = new DatabaseSync(database, { readOnly: true });
  try {
    const account = db.prepare("SELECT username, password_hash FROM oc_console_accounts").get();
    assert.deepEqual({ ...account }, { username: "alice", password_hash: replacement });
    assert.equal(db.prepare("SELECT name FROM sqlite_schema WHERE name = ?").get("oc_console_workspace_grants"), undefined);
  } finally {
    db.close();
  }
  assert.deepEqual(loadConsoleAccounts(database), [{ username: "alice", passwordHash: replacement }]);
  configureConsoleAccount(database, { kind: "remove-account", username: "alice" });
  assert.deepEqual(loadConsoleAccounts(database), []);
});
