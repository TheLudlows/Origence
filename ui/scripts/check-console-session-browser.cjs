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
  const upstream = createServer((req, res) => {
    const grant = grants.find((item) => item.authorization === req.headers.authorization);
    const path = new URL(req.url, "http://fixture.invalid").pathname;
    const send = (value, status = 200) => {
      res.writeHead(status, { "Content-Type": "application/json" });
      res.end(JSON.stringify(value));
    };
    if (path === "/health/ready") return send({ status: "ready" });
    if (!grant) return send({}, 401);
    if (path === "/v1/whoami")
      return send({ id: grant.workspaceId, workspace_id: grant.workspaceId, role: "reader" });
    if (path === "/v1/assets")
      return send({ next_after: null, items: [{
        asset_id: grant.workspaceId, kind: "knowledge",
        title: `${grant.name}知识夹具`, version: 1, source_event_id: excluded,
      }] });
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
    await page.getByRole("button", { name: "退出", exact: true }).click();
    await page.getByLabel("账号", { exact: true }).waitFor();
    await page.reload();
    await page.getByLabel("账号", { exact: true }).waitFor();
    assert.equal(await page.getByText("运营知识夹具", { exact: true }).count(), 0);
    console.log("PASS local account, authorized workspace switch, mobile reload, and sign-out (synthetic HTTP, Chromium)");
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
