const assert = require("node:assert/strict");
const { once } = require("node:events");
const { createServer } = require("node:http");
const { chromium } = require("playwright");

const alpha = "11111111-1111-4111-8111-111111111111";
const beta = "22222222-2222-4222-8222-222222222222";
const excluded = "33333333-3333-4333-8333-333333333333";
const existingChild = "44444444-4444-4444-8444-444444444444";
const password = "console-fixture-密码-01";
const grants = [
  { workspaceId: alpha, name: "研发", authorization: "Bearer synthetic-alpha" },
  { workspaceId: beta, name: "运营", authorization: "Bearer synthetic-beta" },
];
const otherGrant = { workspaceId: excluded, name: "另一个账号的工作区", authorization: "Bearer synthetic-excluded" };

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return `http://127.0.0.1:${server.address().port}`;
}

async function run() {
  const { createUiServer } = await import("../dist/server.js");
  const { hashConsolePassword } = await import("../dist/console-accounts.js");
  const children = new Map([[existingChild, { workspace_id: existingChild, name: "已有子工作区" }]]);
  const creationAttempts = [];
  const upstream = createServer(async (req, res) => {
    const grant = [...grants, otherGrant].find((item) => item.authorization === req.headers.authorization);
    const path = new URL(req.url, "http://fixture.invalid").pathname;
    const send = (value, status = 200) => {
      res.writeHead(status, { "Content-Type": "application/json" });
      res.end(JSON.stringify(value));
    };
    if (path === "/health/ready") return send({ status: "ready" });
    if (!grant) return send({}, 401);
    if (path === "/v1/whoami")
      return send({ id: grant.workspaceId, workspace_id: grant.workspaceId, role: grant.workspaceId === alpha ? "admin" : "reader" });
    if (path === "/v1/workspaces/owned")
      return grant.workspaceId === alpha ? send({ items: [...children.values()], next_after: null }) : send({}, 403);
    if (req.method === "PUT" && path.startsWith("/v1/workspaces/owned/")) {
      if (grant.workspaceId !== alpha || req.headers["x-origence-workspace"]) return send({}, 403);
      const id = path.split("/").at(-1);
      const parts = [];
      for await (const part of req) parts.push(part);
      const { name } = JSON.parse(Buffer.concat(parts).toString());
      creationAttempts.push(id);
      children.set(id, { workspace_id: id, name });
      if (creationAttempts.length === 1) return send({ error: { message: "模拟提交回执丢失" } }, 502);
      return send({ workspace_id: id });
    }
    if (path === "/v1/assets") {
      const scoped = req.headers["x-origence-workspace"];
      const child = scoped ? children.get(scoped) : undefined;
      if (scoped && (!child || grant.workspaceId !== alpha)) return send({}, 403);
      return send({ next_after: null, items: [{
        asset_id: child?.workspace_id ?? grant.workspaceId, kind: "knowledge",
        title: `${child?.name ?? grant.name}知识夹具`, version: 1, source_event_id: excluded,
      }] });
    }
    if (path === "/v1/runs") return send({ items: [], next_after: null });
    return send({}, 404);
  });
  let ui, browser;
  try {
    const passwordHash = await hashConsolePassword(password);
    ui = createUiServer({
      apiBaseUrl: await listen(upstream),
      accounts: [
        { username: "fixture", passwordHash, grants },
        { username: "other", passwordHash, grants: [otherGrant] },
      ],
    });
    const base = await listen(ui);
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    const context = await browser.newContext({ viewport: { width: 375, height: 800 }, hasTouch: true });
    const page = await context.newPage();
    page.setDefaultTimeout(5000);
    await page.goto(base + "/#knowledge");
    await page.getByLabel("账号", { exact: true }).fill("fixture");
    await page.getByLabel("密码", { exact: true }).fill(password);
    assert.equal(await page.getByLabel("API key").count(), 0);
    await page.getByRole("button", { name: "登录", exact: true }).click();
    await page.getByText("研发知识夹具", { exact: true }).waitFor();
    await page.getByRole("heading", { name: "已发布知识", exact: true }).waitFor();
    assert.doesNotMatch(await page.locator("main").innerText(), /资产/);
    const workspaces = page.getByRole("list", { name: "工作区", exact: true });
    const workspace = (name) => workspaces.getByRole("button", { name, exact: true });
    await workspaces.waitFor();
    assert.deepEqual(await workspaces.getByRole("button").allTextContents(), ["研发", "已有子工作区", "运营"]);
    for (const name of ["研发", "已有子工作区", "运营"]) assert.equal(await workspace(name).isVisible(), true);
    assert.equal(await workspace("研发").getAttribute("aria-current"), "true");
    assert.equal(await workspace("另一个账号的工作区").count(), 0);
    await workspace("运营").press("Enter");
    await page.getByRole("heading", { name: "运行总览", exact: true }).waitFor();
    await page.getByRole("button", { name: /知识管理/ }).click();
    await page.getByText("运营知识夹具", { exact: true }).waitFor();
    assert.equal(await page.getByText("研发知识夹具", { exact: true }).count(), 0);
    await page.reload();
    await page.getByText("运营知识夹具", { exact: true }).waitFor();
    await page.getByRole("heading", { name: "已发布知识", exact: true }).waitFor();
    assert.doesNotMatch(await page.locator("main").innerText(), /资产/);
    assert.equal(await workspace("运营").getAttribute("aria-current"), "true");
    assert.deepEqual(await page.evaluate(() => ({
      storage: { ...sessionStorage },
      localStorageSize: localStorage.length,
      readableCookies: document.cookie,
    })), {
      storage: { "origence.selectedWorkspace": beta },
      localStorageSize: 0,
      readableCookies: "",
    });
    assert.equal((await context.cookies()).find((cookie) => cookie.name.startsWith("origence-console-")).httpOnly, true);
    const newWorkspace = page.getByRole("button", { name: "新建工作区", exact: true });
    assert.equal(await newWorkspace.count(), 0);
    await workspace("研发").tap();
    await newWorkspace.click();
    const createDialog = page.getByRole("dialog", { name: "新建工作区", exact: true });
    await createDialog.getByLabel("工作区名称", { exact: true }).fill("试点");
    const create = createDialog.getByRole("button", { name: "创建工作区", exact: true });
    await create.click();
    await createDialog.getByText("模拟提交回执丢失", { exact: true }).waitFor();
    await create.click();
    await createDialog.waitFor({ state: "hidden" });
    await page.getByRole("button", { name: /知识管理/ }).click();
    await page.getByText("试点知识夹具", { exact: true }).waitFor();
    assert.deepEqual(creationAttempts, [creationAttempts[0], creationAttempts[0]]);
    assert.equal(children.size, 2);
    assert.deepEqual(await workspaces.getByRole("button").allTextContents(), ["研发", "已有子工作区", "试点", "运营"]);
    assert.equal(await newWorkspace.count(), 0);
    await page.reload();
    await page.getByText("试点知识夹具", { exact: true }).waitFor();
    assert.equal(await workspace("试点").getAttribute("aria-current"), "true");
    await page.getByRole("button", { name: "退出", exact: true }).click();
    await page.getByLabel("账号", { exact: true }).waitFor();
    await page.reload();
    await page.getByLabel("账号", { exact: true }).waitFor();
    assert.equal(await page.getByText("运营知识夹具", { exact: true }).count(), 0);
    await page.getByLabel("账号", { exact: true }).fill("other");
    await page.getByLabel("密码", { exact: true }).fill(password);
    await page.getByRole("button", { name: "登录", exact: true }).click();
    await workspaces.waitFor();
    assert.deepEqual(await workspaces.getByRole("button").allTextContents(), ["另一个账号的工作区"]);
    assert.equal(await workspace("另一个账号的工作区").getAttribute("aria-current"), "true");
    console.log("PASS console login, visible workspace lists with account isolation, scoped creation and retry, keyboard and mobile switching, reload, and sign-out (synthetic HTTP, Chromium)");
  } finally {
    await browser?.close();
    for (const server of [ui, upstream]) {
      server?.closeAllConnections();
      server?.close();
    }
  }
}

run().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
