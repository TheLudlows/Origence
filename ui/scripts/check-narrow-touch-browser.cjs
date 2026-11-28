// Browser-only touch regression through the built UI, relay and loopback HTTP fixture.
const { createFixtureUiServer, fixtureAccounts, fixtureSignIn } = require("./browser-fixture-accounts.cjs");
const assert = require("node:assert/strict");
const { once } = require("node:events");
const { createServer } = require("node:http");
const { chromium } = require("playwright");

const assetId = "11111111-1111-4111-8111-111111111111";
const assetPath = `/v1/assets/${assetId}?version=1`;
const assetTitle = "固定版触屏手册";
const quote = "仅用于触屏测试的证据";
const token = "synthetic-touch-writer";

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return `http://127.0.0.1:${server.address().port}`;
}

const previewRoutes = [];
const upstream = createServer(async (req, res) => {
  const send = (value, status = 200) => {
    res.writeHead(status, { "Content-Type": "application/json" });
    res.end(JSON.stringify(value));
  };
  if (req.headers.authorization !== `Bearer ${token}`) return send({}, 401);
  if (req.url === "/v1/whoami")
    return send({ id: assetId, workspace_id: assetId, role: "writer" });
  if (req.url === "/v1/assets?kind=knowledge&limit=25")
    return send({ items: [], next_after: null });
  if (req.method === "POST" && req.url === "/v1/context") {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const body = JSON.parse(Buffer.concat(chunks));
    previewRoutes.push(body.routes.map((route) => route.route));
    return send({
      budget: 4000, used_bytes: Buffer.byteLength(quote), effective_mode: "keyword",
      warnings: [], routes: [], rendered_context: quote,
      items: [{ evidence_id: "touch-evidence", title: "手册", content: quote, route: "knowledge",
        normalization_status: "confirmed",
        provenance: { type: "asset_chunk", asset_id: assetId, asset_version: 1 } }],
    });
  }
  if (req.url === assetPath)
    return send({ asset_id: assetId, kind: "knowledge", version: 1, title: assetTitle,
      content: quote, content_hash: "fixture", normalization_status: "confirmed" });
  return send({}, 404);
});

const targetSize = (locator) => locator.evaluate((node) => {
  const { width, height } = node.getBoundingClientRect();
  return { width: Math.floor(width), height: Math.floor(height) };
});
const focused = (locator) => locator.evaluate((node) => node === document.activeElement);
const assertViewport = (page, width) => page.evaluate((width) => {
  if (window.innerWidth !== width || document.documentElement.scrollWidth > width)
    throw Error(`Viewport/scroll width: ${window.innerWidth}/${document.documentElement.scrollWidth}, expected ${width}`);
  if (!matchMedia("(any-pointer: coarse)").matches)
    throw Error("Coarse-pointer emulation is not active");
}, width);
const tapEdge = async (control, { right = false, bottom = false } = {}) => {
  const { width, height } = await targetSize(control);
  await control.tap({ position: { x: right ? width - 4 : 4, y: bottom ? height - 4 : 4 } });
};

async function testWidth(browser, origin, width, colorScheme) {
  const page = await browser.newPage({ viewport: { width, height: 800 }, isMobile: true, hasTouch: true, colorScheme });
  page.setDefaultTimeout(4000);
  const sizes = {};
  try {
    await page.goto(origin + "/#knowledge");
    await fixtureSignIn(page, token, "touch");
    await page.getByRole("heading", { name: "没有匹配的知识" }).waitFor();
    await assertViewport(page, width);
    const logout = page.getByRole("button", { name: "退出", exact: true });
    sizes.logout = await targetSize(logout);
    const create = page.getByRole("button", { name: "＋ 新建 / 上传" });
    await create.tap();
    const editor = page.getByRole("dialog", { name: "新建知识" });
    const title = editor.getByLabel("文档标题");
    sizes.title = await targetSize(title);
    await title.tap();
    assert.equal(await focused(title), true);
    await title.fill("不提交的草稿");
    const close = editor.getByRole("button", { name: "关闭" });
    sizes.close = await targetSize(close);
    await close.tap();
    await editor.waitFor({ state: "hidden" });
    assert.equal(await focused(create), true);
    await create.tap();
    assert.equal(await title.inputValue(), "");
    await close.tap();
    await editor.waitFor({ state: "hidden" });

    const nav = page.getByRole("navigation", { name: "主导航" });
    const contexts = nav.getByRole("button", { name: /上下文工作台$/ });
    await contexts.tap();
    const main = page.getByRole("main");
    await main.getByRole("heading", { name: "上下文工作台" }).waitFor();
    assert.equal(await contexts.getAttribute("aria-current"), "page");
    assert.equal(await focused(main), true);
    await assertViewport(page, width);
    const query = page.getByLabel("任务与查询");
    await query.tap();
    assert.equal(await focused(query), true);
    await query.fill("触屏检查");
    const privateSession = page.getByRole("checkbox", { name: "包含当前 key 的私有会话" });
    const privateSessionLabel = page.locator("label", { has: privateSession });
    sizes.sessionLabel = await targetSize(privateSessionLabel);
    await tapEdge(privateSessionLabel, { right: true, bottom: true });
    assert.equal(await privateSession.isChecked(), true);
    await tapEdge(privateSessionLabel, { right: true });
    assert.equal(await privateSession.isChecked(), false);
    await tapEdge(privateSessionLabel, { right: true, bottom: true });
    assert.equal(await privateSession.isChecked(), true);
    await page.getByRole("button", { name: "预览上下文" }).tap();
    const disclosure = page.locator("details.source");
    const summary = disclosure.locator("summary");
    sizes.disclosure = await targetSize(summary);
    assert.equal(await disclosure.getAttribute("open"), null);
    await tapEdge(summary, { right: true, bottom: true });
    const asset = disclosure.getByRole("button", { name: /· v1$/ });
    await asset.waitFor();
    await assertViewport(page, width);
    await tapEdge(summary, { right: true, bottom: true });
    await asset.waitFor({ state: "hidden" });
    await tapEdge(summary, { right: true, bottom: true });
    await asset.tap();
    await main.getByRole("heading", { name: assetTitle }).waitFor();
    assert.equal(new URL(page.url()).hash, `#asset/${assetId}?version=1`);
    assert.equal(await focused(main), true);
    await assertViewport(page, width);
    await tapEdge(logout, { right: true, bottom: true });
    await page.getByLabel("账号", { exact: true }).waitFor();
    await main.getByRole("heading", { name: "登录控制台" }).waitFor();
    assert.equal(await nav.count(), 0);
    return sizes;
  } finally {
    await page.close();
  }
}

(async () => {
  const { createUiServer } = await import("../dist/server.js");
  let browser, relay;
  try {
    relay = createFixtureUiServer(createUiServer, { apiBaseUrl: await listen(upstream), accounts: fixtureAccounts(assetId, [token]) });
    const origin = await listen(relay);
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    const failures = [];
    for (const [width, scheme] of [[320, "light"], [375, "dark"], [736, "light"]]) {
      const sizes = await testWidth(browser, origin, width, scheme);
      console.log(`${width}px ${scheme}: ${JSON.stringify(sizes)}`);
      for (const [name, size] of Object.entries(sizes)) {
        if (size.width < 44 || size.height < 44)
          failures.push(`${width}px ${name}: ${size.width}×${size.height}`);
      }
    }
    assert.deepEqual(previewRoutes, Array.from({ length: 3 }, () => ["memory", "knowledge", "learning", "session"]));
    assert.deepEqual(failures, [], "Coarse pointer targets must be at least 44×44 CSS px");
  } finally {
    await browser?.close();
    relay?.closeAllConnections();
    relay?.close();
    upstream.closeAllConnections();
    upstream.close();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
