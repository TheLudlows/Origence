const assert = require("node:assert/strict");
const { once } = require("node:events");
const { createServer } = require("node:http");
const { chromium } = require("playwright");

const alpha = "11111111-1111-4111-8111-111111111111";
const beta = "22222222-2222-4222-8222-222222222222";
const excluded = "33333333-3333-4333-8333-333333333333";
const password = "console-fixture-密码-01";
const grants = [
  { workspaceId: alpha, name: "研发", authorization: "Bearer synthetic-alpha" },
  { workspaceId: beta, name: "运营", authorization: "Bearer synthetic-beta" },
];

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return `http://127.0.0.1:${server.address().port}`;
}

async function run() {
  const { createUiServer } = await import("../dist/server.js");
  const { hashConsolePassword } = await import("../dist/console-accounts.js");
  const children = new Map();
  const creationAttempts = [];
  const upstream = createServer(async (req, res) => {
    const grant = grants.find((item) => item.authorization === req.headers.authorization);
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
    ui = createUiServer({
      apiBaseUrl: await listen(upstream),
      accounts: [{ username: "fixture", passwordHash: await hashConsolePassword(password), grants }],
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
    const picker = page.getByLabel("工作区", { exact: true });
    await picker.waitFor();
    assert.deepEqual(await picker.locator("option").allTextContents(), ["研发", "运营"]);
    await picker.selectOption(beta);
    await page.getByRole("heading", { name: "运行总览", exact: true }).waitFor();
    await page.getByRole("button", { name: /知识管理/ }).click();
    await page.getByText("运营知识夹具", { exact: true }).waitFor();
    assert.equal(await page.getByText("研发知识夹具", { exact: true }).count(), 0);
    await page.reload();
    await page.getByText("运营知识夹具", { exact: true }).waitFor();
    await page.getByRole("heading", { name: "已发布知识", exact: true }).waitFor();
    assert.doesNotMatch(await page.locator("main").innerText(), /资产/);
    assert.equal(await picker.inputValue(), beta);
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
    await picker.selectOption(alpha);
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
    assert.equal(children.size, 1);
    assert.equal(await newWorkspace.count(), 0);
    await page.reload();
    await page.getByText("试点知识夹具", { exact: true }).waitFor();
    assert.equal(await picker.inputValue(), creationAttempts[0]);
    await page.getByRole("button", { name: "退出", exact: true }).click();
    await page.getByLabel("账号", { exact: true }).waitFor();
    await page.reload();
    await page.getByLabel("账号", { exact: true }).waitFor();
    assert.equal(await page.getByText("运营知识夹具", { exact: true }).count(), 0);
    console.log("PASS console login, scoped workspace creation and retry, mobile switch, reload, and sign-out (synthetic HTTP, Chromium)");
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
