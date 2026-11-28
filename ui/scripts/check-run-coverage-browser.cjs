// Browser regression using the built UI, relay, and loopback synthetic HTTP.
const { createFixtureUiServer, fixtureAccounts, fixtureSignIn } = require("./browser-fixture-accounts.cjs");
const assert = require("node:assert/strict");
const { once } = require("node:events");
const { createServer } = require("node:http");
const { chromium } = require("playwright");

const populated = { run_id: "22222222-2222-4222-8222-222222222222", summary: "合成采集 Run", created_at_ms: 1000 };
const empty = { run_id: "55555555-5555-4555-8555-555555555555", summary: "合成空 Run", created_at_ms: 2000 };
const unknown = "已加载事件中未见采集声明，覆盖未知";
const pageResult = (items) => ({ items, next_after: null });
let pageFailure;
let nextPrefixReply;
let nextVerificationReply;
let nextEventsReply;
let verificationRequests = 0;
const evidence = "合成外部核验依据";
const prefixRequests = [];
const runPrefix = (sequence) => ({
  run_id: populated.run_id, through_sequence: sequence, digest: String(sequence).repeat(64),
});

const upstream = createServer((req, res) => {
  const url = new URL(req.url, "http://fixture.invalid");
  const send = (value, status = 200) => {
    res.writeHead(status, { "Content-Type": "application/json" });
    res.end(JSON.stringify(value));
  };
  if (req.headers.authorization !== "Bearer synthetic-owner")
    return send({ error: { message: "合成凭据无效" } }, 401);
  if (url.pathname === "/v1/whoami")
    return pageFailure === 401
      ? send({ error: { message: "合成凭据已撤销" } }, 401)
      : send({ id: populated.run_id, workspace_id: empty.run_id, role: "workspace" });
  if (url.pathname === "/v1/runs") return send(pageResult([populated, empty]));
  if (url.pathname === `/v1/runs/${populated.run_id}/verifications`) {
    verificationRequests++;
    const response = pageResult([{
      assessment: { through_sequence: 1, prefix_digest: runPrefix(1).digest,
        verifier_label: "合成核验者", assessed_at_ms: 1000, conclusion: "inconclusive",
        basis_summary: evidence, basis_sequences: [1], reported_skill_version: null },
      recorder_principal_id: "synthetic", recorded_at_ms: 1000, withdrawal: null,
    }]);
    if (nextVerificationReply) {
      const hold = nextVerificationReply;
      nextVerificationReply = undefined;
      return hold((status) => {
        if (!res.destroyed) send(status === 401 ? { error: { message: "旧核验凭据" } } : response, status);
      });
    }
    return send(response);
  }
  if (url.pathname === `/v1/runs/${populated.run_id}/prefix`) {
    const sequence = Number(url.searchParams.get("through_sequence"));
    prefixRequests.push(sequence);
    if (nextPrefixReply) {
      const hold = nextPrefixReply;
      nextPrefixReply = undefined;
      return hold((payload, status) => { if (!res.destroyed) send(payload, status); });
    }
    return send(runPrefix(sequence));
  }
  if (url.pathname === `/v1/runs/${empty.run_id}`) return send({ run: empty, events: [], next_after: null });
  if (url.pathname === `/v1/runs/${populated.run_id}`) {
    const after = Number(url.searchParams.get("after"));
    if (after && pageFailure) return send({ error: { message: "合成读取拒绝" } }, pageFailure);
    const sequence = after + 1;
    const response = { run: populated, next_after: sequence < 3 ? sequence : null, events: [{
      sequence, recorded_at_ms: 1000, evidence_origin: "reported", references: [],
      report: { reporter: "client", event: {
        kind: "coverage", component: "codex_hook", status: after ? "reported" : "not_collected", summary: "合成声明详情",
      } },
    }] };
    if (nextEventsReply) {
      const hold = nextEventsReply;
      nextEventsReply = undefined;
      return hold(() => send(response));
    }
    return send(response);
  }
  return send({}, 404);
});

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return `http://127.0.0.1:${server.address().port}`;
}

async function checkFixedPrefix(page) {
  const details = page.locator("section").filter({ has: page.getByRole("heading", { name: "节点与上下文证据", exact: true }) });
  const prefix = details.getByRole("button", { name: "获取此处的固定前缀摘要" });
  const digest = (sequence) => details.getByText(String(sequence).repeat(64), { exact: false });
  const event = (sequence) => page.locator(`.timeline button[data-sequence="${sequence}"]`);
  const launch = async () => {
    const hold = Promise.withResolvers();
    nextPrefixReply = hold.resolve;
    const timer = setTimeout(() => hold.reject(new Error("Expected a prefix request")), 4000);
    try {
      const [respond] = await Promise.all([hold.promise, prefix.press("Enter")]);
      return respond;
    } finally {
      clearTimeout(timer);
      nextPrefixReply = undefined;
    }
  };

  for (const staleStatus of [200, 403]) {
    await event(1).press("Enter");
    const firstRequest = (request) => request.url().includes("/prefix?through_sequence=1");
    const settled = Promise.race([
      page.waitForEvent("requestfinished", { predicate: firstRequest }).then(() => "finished"),
      page.waitForEvent("requestfailed", { predicate: firstRequest }).then(() => "aborted"),
    ]);
    const respond = await launch();
    await event(2).press("Enter");
    await prefix.press("Enter");
    await digest(2).waitFor();
    respond(staleStatus === 200 ? runPrefix(1) : { error: { message: "旧选择拒绝" } }, staleStatus);
    const disposition = await settled;
    await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
    assert.equal(await event(2).getAttribute("aria-pressed"), "true");
    assert.equal(await digest(1).count(), 0, "Late prefix must not be shown under another selected event");
    assert.equal(await page.getByText("旧选择拒绝", { exact: true }).count(), 0, "Old failure must not alert a different event");
    assert.equal(disposition, "aborted", "Switching events must cancel the earlier lookup");
  }

  const previousCount = prefixRequests.length;
  const deny = await launch();
  await details.getByRole("status").filter({ hasText: "正在读取固定前缀摘要…" }).waitFor();
  assert.equal(await digest(2).count(), 0, "Fresh read must remove the earlier prefix while pending");
  await prefix.evaluate((control) => control.click());
  deny({ error: { message: "本次前缀无权读取" } }, 403);
  await details.getByRole("status").filter({ hasText: "本次前缀无权读取" }).waitFor();
  assert.equal(prefixRequests.length, previousCount + 1);
  assert.equal(await digest(2).count(), 0);
  assert.equal(await prefix.evaluate((control) => control === document.activeElement), true);

  const wrong = await launch();
  wrong(runPrefix(1), 200);
  await details.getByRole("status").filter({ hasText: "固定前缀响应与当前 Run 或事件不一致" }).waitFor();
  assert.equal(await digest(1).count(), 0);
  await prefix.press("Enter");
  await digest(2).waitFor();
  assert.equal(await digest(2).count(), 1);
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
  await fixtureSignIn(page, "synthetic-owner");
  await coverage.getByText("codex_hook", { exact: true }).waitFor();
  assert.deepEqual(await rows(), [first]);
  assert.match(await coverage.innerText(), /整轮覆盖仍未知[\s\S]*还有后续事件/);

  const verification = page.getByText(evidence, { exact: true });
  const event = (sequence) => page.locator(`.timeline button[data-sequence="${sequence}"]`);
  const prefix = page.getByRole("button", { name: "获取此处的固定前缀摘要" });
  const digest2 = page.locator("pre").getByText(runPrefix(2).digest, { exact: false });
  const focused = (locator) => locator.evaluate((node) => node === document.activeElement);

  await verification.waitFor();
  await more.press("Enter");
  await event(2).waitFor();
  const currentVerificationRequests = verificationRequests;
  await checkFixedPrefix(page);

  const pagination = Promise.withResolvers();
  nextEventsReply = pagination.resolve;
  await more.press("Enter");
  const finishPagination = await pagination.promise;
  await prefix.focus();
  finishPagination();
  await event(3).waitFor();
  assert.equal(await event(2).getAttribute("aria-pressed"), "true", "Appending a page must not select event 1");
  assert.equal(await digest2.count(), 1);
  assert.equal(await focused(prefix), true);
  assert.equal(verificationRequests, currentVerificationRequests, "Normal pagination must not reload verification");
  assert.equal(await verification.count(), 1);
  assert.deepEqual(await rows(), [
    first, ["2", "codex_hook", "已上报", "客户端上报；未经独立验证"],
    ["3", "codex_hook", "已上报", "客户端上报；未经独立验证"],
  ]);
  assert.doesNotMatch(await coverage.innerText(), /还有后续事件/);
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth === innerWidth), true);

  await openRun(populated);
  await verification.waitFor();
  const refresh = Promise.withResolvers();
  nextVerificationReply = refresh.resolve;
  await page.getByRole("button", { name: "刷新核验记录" }).press("Enter");
  const finishRefresh = await refresh.promise;
  const staleVerificationCanceled = page.waitForEvent("requestfailed", {
    predicate: (request) => request.url().includes("/verifications?"),
  });
  pageFailure = 403;
  await more.press("Enter");
  await page.getByRole("alert").getByText("合成读取拒绝").waitFor();
  assert.equal(await coverage.count(), 0);
  assert.equal(await page.getByText(unknown, { exact: true }).count(), 0);
  assert.equal(await page.getByText("codex_hook", { exact: true }).count(), 0);
  assert.equal(await verification.count(), 0);
  assert.equal(await focused(more), true);

  pageFailure = undefined;
  const beforeRetry = verificationRequests;
  await more.press("Enter");
  await coverage.getByText("codex_hook", { exact: true }).waitFor();
  assert.deepEqual(await rows(), [first]);
  await verification.waitFor();
  assert.equal(verificationRequests, beforeRetry + 1);
  finishRefresh(401);
  await staleVerificationCanceled;
  assert.equal(await verification.count(), 1);
  assert.equal(await page.getByLabel("账号", { exact: true }).count(), 0);
  await more.press("Enter");
  await event(2).waitFor();
  assert.equal(await focused(more), true);
  await more.press("Enter");
  await event(3).waitFor();
  assert.equal(await focused(event(3)), true, "When Load more disappears, focus should move to the first new event");
  assert.equal(await event(1).getAttribute("aria-pressed"), "true", "Moving focus is not selection");

  await openRun(empty);
  await coverage.getByText(unknown, { exact: true }).waitFor();
  assert.equal(await page.getByText("codex_hook", { exact: true }).count(), 0);
  await openRun(populated);
  await coverage.getByText("codex_hook", { exact: true }).waitFor();
  pageFailure = 401;
  await more.press("Enter");
  await page.getByText("你还没有获授权的工作区", { exact: true }).waitFor();
  assert.equal(await coverage.count(), 0);
  assert.equal(await page.getByText("codex_hook", { exact: true }).count(), 0);
  pageFailure = undefined;
}

(async () => {
  const { createUiServer } = await import("../dist/server.js");
  let relay, browser;
  try {
    relay = createFixtureUiServer(createUiServer, { apiBaseUrl: await listen(upstream), accounts: fixtureAccounts(empty.run_id, ["synthetic-owner"]) });
    const origin = await listen(relay);
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    for (const width of [1024, 320]) {
      const page = await browser.newPage({ viewport: { width, height: 768 } });
      page.setDefaultTimeout(4000);
      await checkCoverage(page, origin);
      await page.close();
      console.log(`PASS ${width}px: ordered declarations, fixed-prefix isolation, pagination, 403, retry, Run switch, revoked workspace access`);
    }
  } finally {
    await browser?.close();
    relay?.closeAllConnections();
    relay?.close();
    upstream.closeAllConnections();
    upstream.close();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
