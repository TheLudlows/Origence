import { createServer } from "node:http";
import { once } from "node:events";
import { test } from "node:test";
import assert from "node:assert/strict";
import { createUiServer } from "../dist/server.js";
import { hashConsolePassword, loadConsoleAccounts } from "../dist/console-accounts.js";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

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
  const ui = await listen(createUiServer({
    apiBaseUrl: upstream,
    accounts: [
      { username: "alice", passwordHash: aliceHash, grants: [
        { workspaceId: a, name: "Alpha", authorization: "Bearer alice-a" },
        { workspaceId: b, name: "Beta", authorization: "Bearer alice-b" },
      ] },
      { username: "bob", passwordHash: bobHash, grants: [
        { workspaceId: b, name: "Beta", authorization: "Bearer bob-b" },
      ] },
    ],
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
  const dir = await mkdtemp(join(tmpdir(), "origence-console-config-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const file = join(dir, "accounts.json");
  const hash = await hashConsolePassword(alicePassword);
  const alice = {
    username: "alice", password_hash: hash,
    workspaces: [{ workspace_id: a, name: "Alpha", key_env: "ALICE_KEY" }],
  };
  await writeFile(file, JSON.stringify({ accounts: [alice, {
    ...alice, username: "bob", workspaces: [{ workspace_id: b, name: "Beta", key_env: "ALICE_KEY" }],
  }] }));
  await assert.rejects(loadConsoleAccounts(file, { ALICE_KEY: "fixture-key" }), /requires its own API key/);
  await writeFile(file, JSON.stringify({ accounts: [alice] }));
  const accounts = await loadConsoleAccounts(file, { ALICE_KEY: "fixture-key" });
  const upstream = await listen(createServer((_req, res) => {
    res.end(JSON.stringify({ id: a, workspace_id: b, role: "reader" }));
  }), t);
  const ui = await listen(createUiServer({ apiBaseUrl: upstream, accounts }), t);
  const login = await fetch(ui + sessionPath, {
    method: "POST",
    headers: { Origin: ui, "Content-Type": "application/json" },
    body: JSON.stringify({ username: alice.username, password: alicePassword }),
  });
  assert.equal(login.status, 502);
  assert.equal(login.headers.get("set-cookie"), null);
});
