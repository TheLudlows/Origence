// Deterministic Chromium regression against a loopback HTTP fixture through the real UI relay.
const { createFixtureUiServer, fixtureAccounts, fixtureSignIn } = require("./browser-fixture-accounts.cjs");
const assert = require("node:assert/strict");
const { once } = require("node:events");
const { createServer } = require("node:http");
const { chromium } = require("playwright");

const assetId = "11111111-1111-4111-8111-111111111111";
const jobId = "22222222-2222-4222-8222-222222222222";
const assetPath = `/v1/assets/${assetId}`;
const jobPath = `/v1/jobs/${jobId}`;
const oldTitle = "已发布的旧手册";
const oldContent = "仍可读取的 v1 正文";
const newTitle = "待发布新手册";
const newContent = "重试后发布的 v2 正文";
const token = "synthetic-writer";

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return `http://127.0.0.1:${server.address().port}`;
}

async function scenario(browser, initialState, run) {
  const { createUiServer } = await import("../dist/server.js");
  let version = 1, state = initialState, retryCount = 0, submissionCount = 0, hold;
  const upstream = createServer(async (req, res) => {
    const send = (body, status = 200) => {
      if (res.destroyed) return;
      res.writeHead(status, { "Content-Type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const asset = {
      asset_id: assetId, source_event_id: jobId, kind: "knowledge",
      title: version === 1 ? oldTitle : newTitle, version,
      content: version === 1 ? oldContent : newContent,
      normalization_status: "confirmed", content_hash: `hash-${version}`, identity: null,
    };
    if (req.headers.authorization !== `Bearer ${token}`) return send({}, 401);
    if (req.url === "/v1/whoami") return send({ id: jobId, workspace_id: assetId, role: "workspace" });
    if (req.method === "GET" && req.url === assetPath) return send(asset);
    if (req.method === "GET" && req.url === "/v1/assets?kind=knowledge&limit=25")
      return send({ items: [asset], next_after: null });
    if (req.method === "POST" && req.url === "/v1/knowledge") {
      submissionCount++;
      const parts = [];
      for await (const part of req) parts.push(part);
      const body = JSON.parse(Buffer.concat(parts).toString());
      if (body.asset_id !== assetId || body.expected_version !== 1 ||
          body.title !== newTitle || body.content !== newContent || !req.headers["idempotency-key"])
        return send({ error: { message: "Unexpected test submission" } }, 400);
      return send({ asset_id: assetId, job_id: jobId, state: "pending" }, 202);
    }
    if (req.method === "POST" && req.url === `${jobPath}/retry`) {
      retryCount++;
      if (state !== "failed") return send({}, 409);
      state = "pending";
      return send({ id: jobId, state });
    }
    if (req.method === "GET" && req.url === jobPath) {
      const job = { id: jobId, state, error_code: state === "failed" ? "synthetic_error" : undefined };
      if (hold) {
        const received = hold;
        hold = undefined;
        return received(() => send(job));
      }
      return send(job);
    }
    return send({}, 404);
  });
  const relay = createFixtureUiServer(createUiServer, { apiBaseUrl: await listen(upstream), accounts: fixtureAccounts(assetId, [token]) });
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.setDefaultTimeout(5_000);
  try {
    await page.goto(`${await listen(relay)}/#knowledge`);
    await fixtureSignIn(page, token);
    await page.getByRole("button", { name: "查看详情 →" }).click();
    await page.getByRole("heading", { name: oldTitle }).waitFor();
    const update = page.getByRole("button", { name: "更新内容" });
    const dialog = page.getByRole("dialog", { name: "更新知识" });
    await update.press("Enter");
    await dialog.getByLabel("文档标题").fill(newTitle);
    await dialog.getByLabel("正文", { exact: true }).fill(newContent);
    await dialog.getByRole("button", { name: "提交新版本" }).press("Enter");
    await run({
      page, dialog, update,
      complete() { state = "completed"; version = 2; },
      holdNextRead() {
        assert.ok(!hold);
        const { promise, resolve } = Promise.withResolvers();
        hold = resolve;
        return async () => {
          const timer = setTimeout(() => resolve(null), 4_000);
          try {
            const release = await promise;
            assert.ok(release, "Automatic job refresh did not reach the fixture");
            return release;
          } finally {
            clearTimeout(timer);
          }
        };
      },
    });
    assert.equal(submissionCount, 1);
    assert.equal(retryCount, initialState === "failed" ? 1 : 0);
    assert.deepEqual(errors, []);
  } finally {
    await page.close();
    for (const http of [relay, upstream]) {
      http.closeAllConnections();
      http.close();
    }
  }
}

(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
  try {
    const scenarioName = process.argv[2];
    if (!scenarioName || scenarioName === "retry") await scenario(browser, "failed", async ({ page, dialog, complete }) => {
      await dialog.getByText("synthetic_error", { exact: true }).waitFor();
      await dialog.getByRole("button", { name: "读取当前版本" }).click();
      await dialog.getByText(`当前已发布 v1 · ${oldTitle}`, { exact: true }).waitFor();
      await dialog.getByRole("button", { name: "重试任务" }).press("Enter");
      await dialog.getByText("pending", { exact: true }).waitFor();
      complete();
      // There is no manual refresh: retry must resume polling until the server publishes.
      await dialog.getByText("completed", { exact: true }).waitFor();
      await dialog.getByRole("button", { name: "查看内容 →", exact: true }).click();
      await page.getByRole("heading", { name: newTitle }).waitFor();
      await page.getByRole("tabpanel", { name: "正文" }).getByText(newContent, { exact: true }).waitFor();
      await dialog.waitFor({ state: "detached" });
    });
    if (!scenarioName || scenarioName === "ordering") await scenario(browser, "processing", async ({ page, dialog, update, holdNextRead, complete }) => {
      await dialog.getByText("processing", { exact: true }).waitFor();
      const release = await holdNextRead()();
      const cancelled = page.waitForEvent("requestfailed", {
        predicate: (request) => request.url().endsWith(`/api${jobPath}`),
      });
      complete();
      await dialog.getByRole("button", { name: "刷新状态" }).press("Enter");
      await dialog.getByText("completed", { exact: true }).waitFor();
      // Let the delayed non-terminal response arrive after the terminal one.
      release();
      assert.equal((await cancelled).failure().errorText, "net::ERR_ABORTED");
      assert.equal(await dialog.getByRole("button", { name: "取消任务" }).count(), 0);
      await page.keyboard.press("Escape");
      await dialog.waitFor({ state: "detached" });
      assert.equal(await update.evaluate((node) => document.activeElement === node), true);
      await page.goBack();
      await page.getByRole("heading", { name: "知识管理" }).waitFor();
      await page.goForward();
      await page.getByRole("heading", { name: newTitle }).waitFor();
      assert.equal(await page.getByRole("dialog").count(), 0);
    });
    console.log(`PASS knowledge publication ${scenarioName ?? "retry and status ordering"} (synthetic HTTP, Chromium)`);
  } finally {
    await browser.close();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
