const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { once } = require("node:events");
const { chromium } = require("playwright");

const { createFixtureUiServer, password } = require("./browser-fixture-accounts.cjs");
const fixture = JSON.parse(readFileSync(0, "utf8"));
const runPath = `/v1/runs/${fixture.run}`;
const assetHash = `#asset/${fixture.asset}?version=2`;
const assetPath = `/v1/assets/${fixture.asset}`;

async function signIn(page, origin, user) {
  await page.goto(`${origin}/#runs/${fixture.run}`);
  await page.getByLabel("账号", { exact: true }).fill(user);
  await page.getByLabel("密码", { exact: true }).fill(password);
  await page.getByRole("button", { name: "登录", exact: true }).click();
}

async function checkVerificationDisplay(page) {
  await page.getByRole("heading", { name: "Owner 转录的外部核验", exact: true }).waitFor();
  await page.getByText("外部结论：失败", { exact: true }).waitFor();
  await page.getByText("实际使用的 Skill 版本：未知", { exact: true }).waitFor();
  await page.getByRole("row").filter({
    has: page.getByRole("cell", { name: "model_use", exact: true }),
  }).getByRole("cell", { name: "未知", exact: true }).waitFor();
  await page.getByText(`固定前缀：1–2 · ${fixture.prefix}`, { exact: true }).waitFor();
  await page.getByText("依据事件：1, 2", { exact: true }).waitFor();
  await page.getByText("v2 允许 80 元，客户端报告 120 元。", { exact: true }).waitFor();
  await page.locator('.timeline button[data-sequence="2"]').click();
  await page.locator(".badge").filter({ hasText: /^客户端上报；未经独立验证$/ }).waitFor();
  await page.getByText(/"outcome": "success"/).waitFor();
  await page.getByText(/合成客户端自报：建议报销 120 元/).waitFor();
}

async function checkOwner(page, origin) {
  await signIn(page, origin, "fixture-owner");
  await checkVerificationDisplay(page);
  await page.locator('.timeline button[data-sequence="1"]').click();
  await page.getByText("服务端准备成功；未确认收到或使用", { exact: true }).waitFor();
  await page.getByRole("button", { name: / · v2$/ }).click();
  await page.waitForURL(url => url.hash === assetHash);
  await page.getByRole("heading", { name: "合成报销制度 v2", exact: true }).waitFor();
  await page.getByText(fixture.textV2, { exact: true }).waitFor();
  assert.equal(await page.getByText("合成报销制度 v3", { exact: true }).count(), 0);
  await page.goBack();
  await page.getByText("外部结论：失败", { exact: true }).waitFor();
}

async function checkDenied(page, path, navigate) {
  const [denied] = await Promise.all([
    page.waitForResponse(response => new URL(response.url()).pathname === "/api" + path),
    navigate(),
  ]);
  assert.ok([403, 404].includes(denied.status()), "Read must be denied by Origence");
  const { error } = await denied.json();
  await page.getByText(error.message, { exact: true }).waitFor();
  assert.equal(await page.getByText("外部结论：失败", { exact: true }).count(), 0);
  assert.equal(await page.getByText(fixture.textV2, { exact: true }).count(), 0);
}

async function main() {
  const { createUiServer } = await import("../dist/server.js");
  const accounts = [["fixture-owner", fixture.owner], ["fixture-reviewer", fixture.other]]
    .map(([username, key]) => ({ username, key, workspaceId: fixture.workspace }));
  const server = createFixtureUiServer(createUiServer, { apiBaseUrl: fixture.api, accounts });
  let browser;
  try {
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    const origin = `http://127.0.0.1:${server.address().port}`;
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    const owner = await browser.newPage();
    owner.setDefaultTimeout(10000);
    await checkOwner(owner, origin);
    const reviewer = await browser.newPage();
    reviewer.setDefaultTimeout(10000);
    await checkDenied(reviewer, runPath, () => signIn(reviewer, origin, "fixture-reviewer"));

    const withdrawal = await fetch(`${fixture.api}/v1/events/${fixture.sourceV2}`, {
      method: "DELETE",
      headers: { Authorization: `Bearer ${fixture.owner}`, "Idempotency-Key": "trace-fixture-withdraw-v2" },
    });
    assert.ok(withdrawal.ok, "Fixture source withdrawal failed");
    await checkDenied(owner, runPath, () => owner.reload());
    await checkDenied(owner, assetPath, () => owner.goto(origin + "/" + assetHash));
    await owner.goto(`${origin}/#asset/${fixture.asset}`);
    await owner.getByRole("heading", { name: "合成报销制度 v3", exact: true }).waitFor();
    console.log("PASS: native Codex → Origence HTTP MCP → Chromium; pinned v2, fixed prefix, separate outcomes, cross-key denial and source withdrawal");
  } finally {
    await browser?.close();
    server.closeAllConnections();
    server.close();
  }
}

main().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
