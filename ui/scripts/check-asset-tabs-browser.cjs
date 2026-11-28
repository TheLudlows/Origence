// Standalone Chromium regression with the built UI, relay, and loopback HTTP fixture.
const { createFixtureUiServer, fixtureAccounts, fixtureSignIn } = require("./browser-fixture-accounts.cjs");
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
const comparisonLoading = "正在比较正文…";
const comparisonFailure = "合成故障：历史正文暂不可读";
const comparisonDone = (from, to) => `正文比较完成，v${from} 到 v${to}。`;
const held = new Map();
const upstream = createServer((req, res) => {
  const path = req.url;
  const send = (body, status = 200) => {
    if (res.destroyed) return;
    res.writeHead(status, { "Content-Type": "application/json" });
    res.end(JSON.stringify(body));
  };
  if (path === "/v1/whoami") return send({ id: assetId, workspace_id: sourceId, role: "workspace" });
  if (path === sourcePath) {
    return deliver({ content: original });
  }
  if (path === versionPath(5)) return deliver({ error: { message: comparisonFailure } }, 503);
  const version = contents.findIndex((_, i) => path === versionPath(i));
  if (version > 0) return deliver({
    asset_id: assetId, source_event_id: sourceId, kind: "knowledge",
    title: `合成文档 v${version}`, version, content: contents[version],
    normalization_status: "confirmed", identity: null, content_hash: "fixture-hash",
  });
  return send({}, 404);

  function deliver(body, status) {
    const notify = held.get(path);
    if (!notify) return send(body, status);
    held.delete(path);
    notify(() => send(body, status));
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

async function contrast(locator) {
  return locator.evaluate((node) => {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 1;
    const context = canvas.getContext("2d");
    const layers = [];
    for (let layer = node; layer; layer = layer.parentElement)
      layers.unshift(getComputedStyle(layer).backgroundColor);
    // The browser resolves and composites CSS colors, including color-mix().
    const paint = (color) => {
      context.fillStyle = color;
      context.fillRect(0, 0, 1, 1);
    };
    const rgb = () => Array.from(context.getImageData(0, 0, 1, 1).data).slice(0, 3);
    for (const layer of layers) paint(layer);
    const background = rgb();
    paint(getComputedStyle(node).color);
    const foreground = rgb();
    const luminance = (color) => {
      const [r, g, b] = color.map((value) => {
        const srgb = value / 255;
        return srgb <= 0.04045 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
      });
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    const [dark, light] = [luminance(foreground), luminance(background)].sort((left, right) => left - right);
    const hex = (color) => `#${color.map((value) => value.toString(16).padStart(2, "0")).join("")}`;
    return { ratio: (light + 0.05) / (dark + 0.05), foreground: hex(foreground), background: hex(background) };
  });
}

async function checkContrast(measurements, name, locator) {
  const colors = await contrast(locator);
  assert.ok(colors.ratio >= 4.5, `${name}: ${colors.foreground} on ${colors.background} is ${colors.ratio}:1`);
  measurements[name] = Number(colors.ratio.toFixed(4));
}

const comparisonSection = (page) => page.locator("section").filter({
  has: page.getByRole("heading", { name: "版本比较" }),
});

const requestCancelled = (page, path) => page.waitForEvent("requestfailed", {
  predicate: (request) => request.url().endsWith(`/api${path}`),
});

async function workspaceChecks(page, sourceRequests, measurements) {
  const tablist = page.getByRole("tablist", { name: "内容与来源" });
  const body = tablist.getByRole("tab", { name: "正文" });
  const source = tablist.getByRole("tab", { name: "原始来源" });
  const bodyPanel = page.getByRole("tabpanel", { name: "正文" });
  const sourcePanel = page.getByRole("tabpanel", { name: "原始来源" });
  const previous = page.getByRole("button", { name: "读取版本" });
  const compareVersion = page.getByLabel("对比版本");
  const compare = page.getByRole("button", { name: "比较正文" });
  const comparison = comparisonSection(page);
  const status = comparison.getByRole("status");
  assert.equal(await status.textContent(), "");
  const originalStatus = await status.elementHandle();
  const statusHistory = await originalStatus.evaluateHandle((node) => {
    const changes = [];
    new MutationObserver(() => changes.push(node.textContent)).observe(node, { subtree: true, childList: true, characterData: true });
    return changes;
  });

  assert.match(await tablist.ariaSnapshot(), /tablist "内容与来源":[\s\S]*tab "正文" \[selected\][\s\S]*tab "原始来源"/);
  await checkContrast(measurements, "selectedBody", body);
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
  await checkContrast(measurements, "selectedSource", source);
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
  assert.equal(await status.textContent(), comparisonLoading);
  const cancelledCompare = requestCancelled(page, versionPath(1));
  await compareVersion.fill("2");
  await compare.press("Enter");
  assert.equal((await cancelledCompare).failure().errorText, "net::ERR_ABORTED");
  releaseOldCompare();
  await comparison.getByText(/v2 → v3/).waitFor();
  assert.equal(await status.textContent(), comparisonDone(2, 3));
  assert.equal(await status.evaluate((node, previous) => node === previous, originalStatus), true);
  assert.equal(await comparison.locator("del").innerText(), contents[2]);
  assert.equal(await comparison.locator("ins").innerText(), contents[3]);
  assert.equal(await sourcePanel.getByRole("status").count(), 1);
  releaseSource();
  await sourcePanel.getByText(original, { exact: false }).waitFor();
  assert.equal(await comparison.getByText(/v2 → v3/).count(), 1);

  const waitForLeavingSource = hold(sourcePath);
  const waitForLeavingCompare = hold(versionPath(5));
  await source.press("Space");
  await compareVersion.fill("5");
  await compare.press("Enter");
  const releaseLeavingSource = await waitForLeavingSource();
  const releaseLeavingCompare = await waitForLeavingCompare();
  const leavingSource = requestCancelled(page, sourcePath);
  const leavingCompare = requestCancelled(page, versionPath(5));
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
  assert.equal(await comparison.getByRole("alert").count(), 0);
  assert.equal(await status.textContent(), "");
  assert.equal(await originalStatus.evaluate((node) => node.isConnected), false);
  assert.deepEqual(await statusHistory.jsonValue(), [comparisonLoading, comparisonDone(2, 3), comparisonLoading]);
  assert.equal(sourceRequests.length, 3);
}

async function feedbackChecks(page, displayedVersion, measurements) {
  const comparison = comparisonSection(page);
  const status = comparison.getByRole("status");
  const statusNode = await status.elementHandle();
  const version = page.getByLabel("对比版本");
  const compare = comparison.getByRole("button", { name: "比较正文" });
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Accessibility.enable");
  const readStatus = async (expectedText) => {
    const { nodes } = await cdp.send("Accessibility.getFullAXTree");
    const region = nodes.find((node) => !node.ignored && node.role?.value === "status");
    assert.ok(region);
    const property = (name) => region.properties.find((item) => item.name === name)?.value.value;
    assert.equal(property("live"), "polite");
    assert.equal(property("atomic"), true);
    if (expectedText) assert.ok(nodes.some((node) =>
      node.parentId === region.nodeId && node.role?.value === "StaticText" && node.name?.value === expectedText,
    ));
    return region.nodeId;
  };
  const initialAX = await readStatus();
  const begin = async (number) => {
    const heldResponse = hold(versionPath(number));
    await version.fill(String(number));
    await compare.press("Enter");
    const release = await heldResponse();
    assert.equal(await status.textContent(), comparisonLoading);
    assert.equal(await status.evaluate((node, previous) => node === previous, statusNode), true);
    assert.equal(await comparison.getByRole("alert").count(), 0);
    await focusIs(compare);
    return release;
  };
  const releaseFailure = await begin(5);
  assert.equal(await readStatus(comparisonLoading), initialAX);
  await checkContrast(measurements, "loading", status);
  releaseFailure();
  const error = comparison.getByRole("alert").getByText(comparisonFailure, { exact: true });
  await error.waitFor();
  assert.equal(await status.textContent(), "");
  assert.equal(await comparison.locator("del, ins").count(), 0);
  await focusIs(compare);
  await checkContrast(measurements, "error", error);

  const releaseObsoleteSuccess = await begin(1);
  const cancelledRepeat = requestCancelled(page, versionPath(1));
  const releaseSuccess = await begin(1);
  assert.equal((await cancelledRepeat).failure().errorText, "net::ERR_ABORTED");
  releaseObsoleteSuccess();
  releaseSuccess();
  await status.filter({ hasText: comparisonDone(1, displayedVersion) }).waitFor();
  const summary = comparison.getByText(new RegExp(`^v1 → v${displayedVersion}`));
  assert.equal(await summary.evaluate((node) => node.closest('[role="status"]')), null);
  assert.equal(await status.evaluate((node, previous) => node === previous, statusNode), true);
  assert.equal(await readStatus(comparisonDone(1, displayedVersion)), initialAX);
  await focusIs(compare);
  assert.equal(await comparison.locator("del").innerText(), contents[1]);
  assert.equal(await comparison.locator("ins").innerText(), contents[displayedVersion]);
  await checkContrast(measurements, "completed", status);
  await checkContrast(measurements, "summary", summary);
  await checkContrast(measurements, "inserted", comparison.locator("ins"));
  await checkContrast(measurements, "deleted", comparison.locator("del"));
  await cdp.detach();
}

(async () => {
  const { createUiServer } = await import("../dist/server.js");
  let relay, browser;
  try {
    relay = createFixtureUiServer(createUiServer, { apiBaseUrl: await listen(upstream), accounts: fixtureAccounts(sourceId, ["synthetic-workspace"]) });
    const origin = await listen(relay);
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    for (const width of [1024, 736, 375, 320]) {
      for (const colorScheme of ["light", "dark"]) {
        const page = await browser.newPage({ viewport: { width, height: 768 }, colorScheme });
        page.setDefaultTimeout(4_000);
        const sourceRequests = [];
        page.on("request", (request) => {
          if (request.url().endsWith(`/api${sourcePath}`)) sourceRequests.push(request);
        });
        await page.goto(`${origin}/#asset/${assetId}?version=3`);
        await fixtureSignIn(page, "synthetic-workspace");
        await page.getByRole("heading", { name: "合成文档 v3" }).waitFor();
        await page.getByText(contents[3], { exact: true }).waitFor();
        const measurements = {};
        await checkContrast(measurements, "versionBadge", page.getByText("confirmed", { exact: true }));
        await workspaceChecks(page, sourceRequests, measurements);
        await feedbackChecks(page, 4, measurements);
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth === innerWidth), true);
        await page.close();
        console.log(`PASS ${width}px ${colorScheme} contrast ${JSON.stringify(measurements)}`);
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
