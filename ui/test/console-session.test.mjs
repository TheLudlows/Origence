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
const c = "33333333-3333-4333-8333-333333333333";
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
  assert.throws(() => loadConfiguredConsoleAccounts({ ORIGENCE_UI_ACCOUNTS_DB: join(dir, "wrong-path.db") }), { code: "ENOENT" });

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
  const credentials = { username: "alice", password: alicePassword };
  assert.equal((await signIn(ui, credentials)).status, 503);

  const env = { ...process.env };
  delete env.ORIGENCE_UI_ACCOUNTS_DB;
  execFileSync(process.execPath, [accountsCli, "set-password", credentials.username], {
    cwd: dir, env, input: credentials.password + "\n",
  });
  const accounts = loadConfiguredConsoleAccounts({});
  const configured = await listen(createUiServer({ apiBaseUrl: "http://127.0.0.1:1", accounts }), t);
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

test("account sessions survive reload and enforce grants, tenant credentials, revocation, and sign-out", async (t) => {
  const identities = new Map([
    ["Bearer alice-a", { id: a, workspace_id: a, role: "writer" }],
    ["Bearer alice-b", { id: b, workspace_id: b, role: "reader" }],
    ["Bearer bob-b", { id: c, workspace_id: b, role: "admin" }],
  ]);
  const forwarded = [];
  const upstream = await listen(createServer((req, res) => {
    const authorization = req.headers.authorization;
    const identity = identities.get(authorization);
    res.setHeader("Content-Type", "application/json");
    if (!identity) {
      res.statusCode = 401;
      return res.end("{}");
    }
    if (req.url === "/v1/whoami") return res.end(JSON.stringify(identity));
    forwarded.push({ authorization, path: req.url, method: req.method, cookie: req.headers.cookie });
    res.end(JSON.stringify({ scoped_workspace: identity.workspace_id }));
  }), t);
  const [aliceHash, bobHash] = await Promise.all([
    hashConsolePassword(alicePassword), hashConsolePassword(bobPassword),
  ]);
  const database = await accountDatabase(t);
  configureConsoleAccount(database, { kind: "set-password", username: "alice", passwordHash: aliceHash });
  configureConsoleAccount(database, { kind: "set-password", username: "bob", passwordHash: bobHash });
  configureConsoleAccount(database, { kind: "grant", username: "alice", workspaceId: a, name: "Alpha", keyEnv: "ALICE_A" });
  configureConsoleAccount(database, { kind: "grant", username: "alice", workspaceId: b, name: "Beta", keyEnv: "ALICE_B" });
  configureConsoleAccount(database, { kind: "grant", username: "bob", workspaceId: b, name: "Beta", keyEnv: "BOB_B" });
  const ui = await listen(createUiServer({
    apiBaseUrl: upstream,
    accounts: loadConsoleAccounts(database, { ALICE_A: "alice-a", ALICE_B: "alice-b", BOB_B: "bob-b" }),
  }), t);
  const signIn = (data, cookie, origin = ui) => fetch(ui + sessionPath, {
    method: "POST",
    headers: { Origin: origin, "Content-Type": "application/json", ...(cookie ? { Cookie: cookie } : {}) },
    body: JSON.stringify(data),
  });
  const credentials = { username: "alice", password: alicePassword };
  assert.deepEqual(await (await fetch(ui + sessionPath)).json(), { mode: "account", session: null });
  assert.equal((await signIn({ key: "alice-a" })).status, 400);
  assert.equal((await signIn({ username: "alice", password: "wrong" })).status, 401);
  assert.equal((await signIn(credentials, undefined, "https://another.example")).status, 403);
  const login = await signIn(credentials);
  assert.equal(login.status, 200);
  const cookieHeader = login.headers.get("set-cookie");
  assert.match(cookieHeader, /Path=\/api; Max-Age=28800; HttpOnly; SameSite=Strict/);
  assert.doesNotMatch(cookieHeader, /alice-a|alice-b/);
  const cookie = cookieHeader.split(";")[0];
  const loginBody = await login.text();
  assert.doesNotMatch(loginBody, /alice-a|alice-b|passwordHash/);
  const alice = JSON.parse(loginBody).session;
  assert.deepEqual(alice.workspaces.map((item) => item.name), ["Alpha", "Beta"]);

  const resume = (cookie) => fetch(ui + sessionPath, { headers: { Cookie: cookie } });
  assert.deepEqual((await (await resume(cookie)).json()).session, alice);
  const request = (cookie, session, workspace, options = {}) =>
    fetch(ui + "/api/v1/" + (options.method ? "search" : "assets"), {
      method: options.method,
      headers: {
        Cookie: cookie,
        "X-Origence-Session": session.binding,
        "X-Origence-Workspace": workspace,
        ...(options.origin ? { Origin: options.origin } : {}),
      },
    });
  const both = await Promise.all([a, b].map(async (workspace) =>
    (await (await request(cookie, alice, workspace)).json()).scoped_workspace,
  ));
  assert.deepEqual(both, [a, b]);
  assert.deepEqual(forwarded.map((entry) => [entry.authorization, entry.cookie]), [
    ["Bearer alice-a", undefined], ["Bearer alice-b", undefined],
  ]);
  assert.equal((await request(cookie, alice, c)).status, 403);
  assert.equal((await request(cookie, alice, a, { method: "POST" })).status, 403);
  assert.equal((await request(cookie, alice, a, { method: "POST", origin: ui })).status, 200);
  assert.equal(forwarded.length, 3);

  identities.delete("Bearer alice-a");
  assert.equal((await request(cookie, alice, a)).status, 401);
  assert.deepEqual((await (await resume(cookie)).json()).session.workspaces.map((item) => item.workspace_id), [b]);
  const bobLogin = await signIn({ username: "bob", password: bobPassword }, cookie);
  const bobCookie = bobLogin.headers.get("set-cookie").split(";")[0];
  const bob = (await bobLogin.json()).session;
  assert.deepEqual(bob.workspaces.map((item) => item.workspace_id), [b]);
  assert.equal((await request(bobCookie, alice, b)).status, 401);
  assert.equal((await request(cookie, alice, b)).status, 401);
  assert.equal((await request(bobCookie, bob, b)).status, 200);
  assert.equal(forwarded.at(-1).authorization, "Bearer bob-b");
  const logout = await fetch(ui + sessionPath, { method: "DELETE", headers: { Origin: ui, Cookie: bobCookie } });
  assert.match(logout.headers.get("set-cookie"), /Max-Age=0/);
  assert.equal((await request(bobCookie, bob, b)).status, 401);
  assert.deepEqual(await (await resume(bobCookie)).json(), { mode: "account", session: null });
});

test("account provisioning rejects reusable keys and login rejects mismatched workspace identities", async (t) => {
  const database = await accountDatabase(t);
  const hash = await hashConsolePassword(alicePassword);
  configureConsoleAccount(database, { kind: "set-password", username: "alice", passwordHash: hash });
  configureConsoleAccount(database, { kind: "set-password", username: "bob", passwordHash: hash });
  configureConsoleAccount(database, { kind: "grant", username: "alice", workspaceId: a, name: "Alpha", keyEnv: "ALICE_KEY" });
  assert.throws(() =>
    configureConsoleAccount(database, { kind: "grant", username: "bob", workspaceId: b, name: "Beta", keyEnv: "ALICE_KEY" }),
    /UNIQUE constraint failed/,
  );
  configureConsoleAccount(database, { kind: "grant", username: "bob", workspaceId: b, name: "Beta", keyEnv: "BOB_KEY" });
  assert.throws(
    () => loadConsoleAccounts(database, { ALICE_KEY: "same-fixture-key", BOB_KEY: "same-fixture-key" }),
    /requires its own API key/,
  );
  configureConsoleAccount(database, { kind: "revoke", username: "bob", workspaceId: b });
  const accounts = loadConsoleAccounts(database, { ALICE_KEY: "fixture-key" });
  assert.equal(accounts.find((account) => account.username === "bob").grants.length, 0);
  const upstream = await listen(createServer((_req, res) => {
    res.end(JSON.stringify({ id: a, workspace_id: b, role: "reader" }));
  }), t);
  const ui = await listen(createUiServer({ apiBaseUrl: upstream, accounts }), t);
  const login = await fetch(ui + sessionPath, {
    method: "POST",
    headers: { Origin: ui, "Content-Type": "application/json" },
    body: JSON.stringify({ username: "alice", password: alicePassword }),
  });
  assert.equal(login.status, 502);
  assert.equal(login.headers.get("set-cookie"), null);
});

test("configuration tables retain password hashes and workspace references across reopen", async (t) => {
  const database = await accountDatabase(t);
  assert.throws(() => initializeConsoleAccounts(database), { code: "EEXIST" });
  assert.deepEqual(loadConsoleAccounts(database, {}), []);
  assert.throws(
    () => configureConsoleAccount(database, { kind: "grant", username: "unknown", workspaceId: a, name: "Alpha", keyEnv: "ALPHA_KEY" }),
    /FOREIGN KEY constraint failed/,
  );
  const original = await hashConsolePassword(alicePassword);
  const replacement = await hashConsolePassword(bobPassword);
  configureConsoleAccount(database, { kind: "set-password", username: "alice", passwordHash: original });
  configureConsoleAccount(database, { kind: "grant", username: "alice", workspaceId: a, name: "Alpha", keyEnv: "ALPHA_KEY" });
  configureConsoleAccount(database, { kind: "set-password", username: "alice", passwordHash: replacement });
  const db = new DatabaseSync(database, { readOnly: true });
  try {
    const account = db.prepare("SELECT username, password_hash FROM oc_console_accounts").get();
    const grant = db.prepare("SELECT username, workspace_id, name, key_env FROM oc_console_workspace_grants").get();
    assert.deepEqual({ ...account }, { username: "alice", password_hash: replacement });
    assert.deepEqual({ ...grant }, { username: "alice", workspace_id: a, name: "Alpha", key_env: "ALPHA_KEY" });
  } finally {
    db.close();
  }
  assert.equal(loadConsoleAccounts(database, { ALPHA_KEY: "fixture-key" })[0].grants.length, 1);
  configureConsoleAccount(database, { kind: "remove-account", username: "alice" });
  assert.deepEqual(loadConsoleAccounts(database, {}), []);
});
