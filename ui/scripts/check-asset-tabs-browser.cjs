// Standalone Chromium regression with the built UI, relay, and loopback HTTP fixture.
const assert = require("node:assert/strict");
const { once } = require("node:events");
const { createServer } = require("node:http");
const { chromium } = require("playwright");

const assetId = "11111111-1111-4111-8111-111111111111";
const sourceId = "22222222-2222-4222-8222-222222222222";
const sourcePath = `/v1/events/${sourceId}`;
const assetPath = `/v1/assets/${assetId}`;
const versionPath = (version) => `${assetPath}?version=${version}`;
const contents = ["", "历史旧词", "上版旧词", "当前正文", "另一个固定版本"];
const original = "合成原始来源";
const held = new Map();
const upstream = createServer((req, res) => {
  const role = req.headers.authorization?.slice("Bearer synthetic-".length);
  const path = req.url;
  const send = (body, status = 200) => {
    if (res.destroyed) return;
    res.writeHead(status, { "Content-Type": "application/json" });
    res.end(JSON.stringify(body));
  };
  if (path === "/v1/whoami") return send({ id: assetId, workspace_id: sourceId, role });
  if (path === sourcePath) {
    if (role !== "writer") return send({ error: { message: "原始来源无权读取" } }, 403);
    return deliver({ content: original });
  }
  const version = contents.findIndex((_, i) => path === versionPath(i));
  if (version > 0) return deliver({
    asset_id: assetId, source_event_id: sourceId, kind: "knowledge",
    title: `合成文档 v${version}`, version, content: contents[version],
    normalization_status: "unnormalized", identity: null, content_hash: "fixture-hash",
  });
  return send({}, 404);

  function deliver(body) {
    const notify = held.get(path);
    if (!notify) return send(body);
    held.delete(path);
    notify(() => send(body));
  }
});

function hold(path) {
  assert.ok(!held.has(path));
  const { promise, resolve } = Promise.withResolvers();
  held.set(path, resolve);
  return async () => {
    const deadline = setTimeout(() => resolve(null), 4_000);
    try {
      const release = await promise;
      assert.ok(release, `Fixture did not receive ${path}`);
      return release;
    } finally {
      clearTimeout(deadline);
      held.delete(path);
    }
  };
}

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return `http://127.0.0.1:${server.address().port}`;
}

const focusIs = async (control) => {
  assert.equal(await control.evaluate((node) => node === document.activeElement), true);
};

const requestCancelled = (page, path) => page.waitForEvent("requestfailed", {
  predicate: (request) => request.url().endsWith(`/api${path}`),
});

async function writerChecks(page, sourceRequests) {
  const tablist = page.getByRole("tablist", { name: "内容与来源" });
  const body = tablist.getByRole("tab", { name: "正文" });
  const source = tablist.getByRole("tab", { name: "原始来源" });
  const bodyPanel = page.getByRole("tabpanel", { name: "正文" });
  const sourcePanel = page.getByRole("tabpanel", { name: "原始来源" });
  const previous = page.getByRole("button", { name: "读取版本" });
  const compareVersion = page.getByLabel("对比版本");
  const compare = page.getByRole("button", { name: "比较正文" });
  const comparison = page.locator("section").filter({ has: page.getByRole("heading", { name: "版本比较" }) });

  assert.match(await tablist.ariaSnapshot(), /tablist "内容与来源":[\s\S]*tab "正文" \[selected\][\s\S]*tab "原始来源"/);
  assert.equal(await tablist.locator("[tabindex='0']").count(), 1);
  assert.equal(await tablist.getByRole("button").count(), 0);
  assert.equal(await tablist.getByRole("spinbutton").count(), 0);
  assert.equal(await page.getByRole("tabpanel").count(), 1);
  for (const tab of [body, source]) {
    const panel = page.locator(`#${await tab.getAttribute("aria-controls")}`);
    assert.equal(await panel.getAttribute("role"), "tabpanel");
    assert.equal(await panel.getAttribute("aria-labelledby"), await tab.getAttribute("id"));
  }

  await previous.focus();
  await page.keyboard.press("Tab");
  await focusIs(body);
  await page.keyboard.press("ArrowLeft");
  await focusIs(source);
  assert.equal(await source.evaluate((node) => getComputedStyle(node).outlineStyle), "solid");
  assert.equal(await source.getAttribute("aria-selected"), "false");
  assert.equal(await bodyPanel.innerText(), contents[3]);
  await page.keyboard.press("ArrowRight");
  await focusIs(body);
  await page.keyboard.press("End");
  await focusIs(source);
  await page.keyboard.press("Home");
  await focusIs(body);
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Shift+Tab");
  await focusIs(previous);
  await page.keyboard.press("Tab");
  await focusIs(body);
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Tab");
  await focusIs(bodyPanel);
  await page.keyboard.press("Tab");
  await focusIs(compareVersion);
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("Shift+Tab");
  await focusIs(body);
  assert.equal(sourceRequests.length, 0);

  const waitForOldSource = hold(sourcePath);
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Enter");
  const releaseOldSource = await waitForOldSource();
  assert.equal(await source.getAttribute("aria-selected"), "true");
  assert.match(await sourcePanel.ariaSnapshot(), /^- tabpanel "原始来源":/);
  assert.equal(await sourcePanel.getByRole("status").innerText(), "正在读取原始来源…");
  assert.equal(await bodyPanel.count(), 0);
  await page.keyboard.press("Tab");
  await focusIs(sourcePanel);
  await page.keyboard.press("Shift+Tab");
  await focusIs(source);
  const cancelledSource = requestCancelled(page, sourcePath);
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("Space");
  assert.equal((await cancelledSource).failure().errorText, "net::ERR_ABORTED");
  releaseOldSource();
  assert.equal(await bodyPanel.innerText(), contents[3]);
  assert.equal(await page.getByText(original, { exact: false }).count(), 0);

  const waitForSource = hold(sourcePath);
  await source.click();
  const releaseSource = await waitForSource();
  const waitForOldCompare = hold(versionPath(1));
  await compareVersion.fill("1");
  await compare.press("Enter");
  const releaseOldCompare = await waitForOldCompare();
  const cancelledCompare = requestCancelled(page, versionPath(1));
  await compareVersion.fill("2");
  await compare.press("Enter");
  assert.equal((await cancelledCompare).failure().errorText, "net::ERR_ABORTED");
  releaseOldCompare();
  await comparison.getByText(/v2 → v3/).waitFor();
  assert.equal(await comparison.locator("del").innerText(), contents[2]);
  assert.equal(await comparison.locator("ins").innerText(), contents[3]);
  assert.equal(await sourcePanel.getByRole("status").count(), 1);
  releaseSource();
  await sourcePanel.getByText(original, { exact: false }).waitFor();
  assert.equal(await comparison.getByText(/v2 → v3/).count(), 1);

  const waitForLeavingSource = hold(sourcePath);
  const waitForLeavingCompare = hold(versionPath(2));
  await source.press("Space");
  await compare.press("Enter");
  const releaseLeavingSource = await waitForLeavingSource();
  const releaseLeavingCompare = await waitForLeavingCompare();
  const leavingSource = requestCancelled(page, sourcePath);
  const leavingCompare = requestCancelled(page, versionPath(2));
  await page.getByLabel("读取固定历史版本").fill("4");
  await previous.press("Enter");
  await page.getByRole("heading", { name: "合成文档 v4" }).waitFor();
  assert.equal((await leavingSource).failure().errorText, "net::ERR_ABORTED");
  assert.equal((await leavingCompare).failure().errorText, "net::ERR_ABORTED");
  releaseLeavingSource();
  releaseLeavingCompare();
  assert.equal(await bodyPanel.innerText(), contents[4]);
  assert.equal(await page.getByText(original, { exact: false }).count(), 0);
  assert.equal(await comparison.locator("ins, del").count(), 0);
  assert.equal(sourceRequests.length, 3);
}

async function readerChecks(page, sourceRequests) {
  assert.equal(await page.getByRole("tablist").count(), 0);
  assert.equal(await page.getByRole("button", { name: "原始来源" }).count(), 0);
  await page.getByRole("button", { name: "读取版本" }).focus();
  await page.keyboard.press("Tab");
  await focusIs(page.getByLabel("对比版本"));
  await page.keyboard.press("Tab");
  await page.keyboard.press("Enter");
  await page.getByText(/v2 → v3/).waitFor();
  assert.equal(await page.getByText(contents[3], { exact: true }).count(), 2);
  assert.equal(sourceRequests.length, 0);
}

(async () => {
  const { createUiServer } = await import("../dist/server.js");
  let relay, browser;
  try {
    relay = createUiServer({ apiBaseUrl: await listen(upstream) });
    const origin = await listen(relay);
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    for (const width of [1024, 736, 375, 320]) {
      for (const colorScheme of ["light", "dark"]) {
        for (const role of ["writer", "reader"]) {
          const page = await browser.newPage({ viewport: { width, height: 768 }, colorScheme });
          page.setDefaultTimeout(4_000);
          const sourceRequests = [];
          page.on("request", (request) => {
            if (request.url().endsWith(`/api${sourcePath}`)) sourceRequests.push(request);
          });
          await page.goto(`${origin}/#asset/${assetId}?version=3`);
          await page.getByLabel("API key").fill(`synthetic-${role}`);
          await page.getByRole("button", { name: "连接工作空间" }).press("Enter");
          await page.getByRole("heading", { name: "合成文档 v3" }).waitFor();
          await page.getByText(contents[3], { exact: true }).waitFor();
          if (role === "writer") await writerChecks(page, sourceRequests);
          else await readerChecks(page, sourceRequests);
          assert.equal(await page.evaluate(() => document.documentElement.scrollWidth === innerWidth), true);
          await page.close();
          console.log(`PASS ${width}px ${colorScheme} ${role}`);
        }
      }
    }
  } finally {
    await browser?.close();
    relay?.closeAllConnections();
    relay?.close();
    upstream.closeAllConnections();
    upstream.close();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
