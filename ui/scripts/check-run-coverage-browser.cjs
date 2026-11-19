// Browser regression using the built UI, relay, and loopback synthetic HTTP.
const assert = require("node:assert/strict");
const { once } = require("node:events");
const { createServer } = require("node:http");
const { chromium } = require("playwright");

const populated = { run_id: "22222222-2222-4222-8222-222222222222", summary: "合成采集 Run", created_at_ms: 1000 };
const empty = { run_id: "55555555-5555-4555-8555-555555555555", summary: "合成空 Run", created_at_ms: 2000 };
const unknown = "已加载事件中未见采集声明，覆盖未知";
const pageResult = (items) => ({ items, next_after: null });
let pageFailure;

const upstream = createServer((req, res) => {
  const url = new URL(req.url, "http://fixture.invalid");
  const send = (value, status = 200) => {
    res.writeHead(status, { "Content-Type": "application/json" });
    res.end(JSON.stringify(value));
  };
  if (req.headers.authorization !== "Bearer synthetic-owner")
    return send({ error: { message: "合成凭据无效" } }, 401);
  if (url.pathname === "/v1/whoami")
    return send({ id: populated.run_id, workspace_id: empty.run_id, role: "reader" });
  if (url.pathname === "/v1/runs") return send(pageResult([populated, empty]));
  if (url.pathname === `/v1/runs/${populated.run_id}/verifications`) return send(pageResult([]));
  if (url.pathname === `/v1/runs/${empty.run_id}`) return send({ run: empty, events: [], next_after: null });
  if (url.pathname === `/v1/runs/${populated.run_id}`) {
    const next = url.searchParams.get("after") === "1";
    if (next && pageFailure) return send({ error: { message: "合成读取拒绝" } }, pageFailure);
    return send({ run: populated, next_after: next ? null : 1, events: [{
      sequence: next ? 2 : 1, recorded_at_ms: 1000, evidence_origin: "reported", references: [],
      report: { reporter: "client", event: {
        kind: "coverage", component: "codex_hook", status: next ? "reported" : "not_collected", summary: "合成声明详情",
      } },
    }] });
  }
  return send({}, 404);
});

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return `http://127.0.0.1:${server.address().port}`;
}

async function checkCoverage(page, origin) {
  const coverage = page.locator("section").filter({ has: page.getByRole("heading", { name: "采集声明", exact: true }) });
  const more = page.getByRole("button", { name: "加载后续事件" });
  const rows = () => coverage.locator("tbody tr").evaluateAll((elements) => elements.map(
    (row) => Array.from(row.cells, (cell) => cell.textContent),
  ));
  const back = page.getByRole("button", { name: "返回运行列表" });
  const openRun = async (run) => {
    await back.press("Enter");
    await page.getByRole("row", { name: new RegExp(run.summary) }).getByRole("button").press("Enter");
  };
  const first = ["1", "codex_hook", "未采集", "客户端上报；未经独立验证"];

  await page.goto(`${origin}/#runs/${populated.run_id}`);
  await page.getByLabel("API key").fill("synthetic-owner");
  await page.getByRole("button", { name: "连接工作空间" }).press("Enter");
  await coverage.getByText("codex_hook", { exact: true }).waitFor();
  assert.deepEqual(await rows(), [first]);
  assert.match(await coverage.innerText(), /整轮覆盖仍未知[\s\S]*还有后续事件/);

  pageFailure = 403;
  await more.press("Enter");
  await page.getByRole("alert").getByText("合成读取拒绝").waitFor();
  assert.equal(await coverage.count(), 0);
  assert.equal(await page.getByText(unknown, { exact: true }).count(), 0);
  assert.equal(await page.getByText("codex_hook", { exact: true }).count(), 0);

  pageFailure = undefined;
  await more.press("Enter");
  await coverage.getByText("codex_hook", { exact: true }).waitFor();
  assert.deepEqual(await rows(), [first]);
  await more.press("Enter");
  await coverage.getByText("已上报", { exact: true }).waitFor();
  assert.deepEqual(await rows(), [first, ["2", "codex_hook", "已上报", "客户端上报；未经独立验证"]]);
  assert.doesNotMatch(await coverage.innerText(), /还有后续事件/);
  assert.equal(await page.locator(".timeline button").count(), 2);
  await page.getByText(/合成声明详情/).waitFor();
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth === innerWidth), true);

  await openRun(empty);
  await coverage.getByText(unknown, { exact: true }).waitFor();
  assert.equal(await page.getByText("codex_hook", { exact: true }).count(), 0);
  await openRun(populated);
  await coverage.getByText("codex_hook", { exact: true }).waitFor();
  pageFailure = 401;
  await more.press("Enter");
  await page.getByLabel("API key").waitFor();
  assert.equal(await coverage.count(), 0);
  assert.equal(await page.getByText("codex_hook", { exact: true }).count(), 0);
  pageFailure = undefined;
}

(async () => {
  const { createUiServer } = await import("../dist/server.js");
  let relay, browser;
  try {
    relay = createUiServer({ apiBaseUrl: await listen(upstream) });
    const origin = await listen(relay);
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    for (const width of [1024, 320]) {
      const page = await browser.newPage({ viewport: { width, height: 768 } });
      page.setDefaultTimeout(4000);
      await checkCoverage(page, origin);
      await page.close();
      console.log(`PASS ${width}px: ordered declarations, pagination, 403, retry, Run switch, 401 logout`);
    }
  } finally {
    await browser?.close();
    relay?.closeAllConnections();
    relay?.close();
    upstream.closeAllConnections();
    upstream.close();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
