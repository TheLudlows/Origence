// Standalone Chromium regression through the built UI and loopback HTTP relay.
const { fixtureAccounts, fixtureSignIn } = require("./browser-fixture-accounts.cjs");
const assert = require("node:assert/strict");
const { once } = require("node:events");
const { createServer } = require("node:http");
const { chromium } = require("playwright");

const asset = "11111111-1111-4111-8111-111111111111";
const source = "22222222-2222-4222-8222-222222222222";
const uploadSource = "33333333-3333-4333-8333-333333333333";
const file = "44444444-4444-4444-8444-444444444444";
const currentSource = "55555555-5555-4555-8555-555555555555";
const uploadedAsset = "66666666-6666-4666-8666-666666666666";
const prefix = "\uFEFF" + "甲".repeat(799);
const quote = "中文 🦀 e\u0301 发布手册";
const original = prefix + quote;
const locator = { parser: "source-ranges-v1", byte_start: Buffer.byteLength(prefix), byte_end: Buffer.byteLength(original) };
const assetPath = `/v1/assets/${asset}`;
const requests = [];
let nextInlineSource;
let denyOriginalFile = false;
const evidence = (title, event, versionAsset = asset, content = quote, range = locator) => ({
  evidence_id: title, title, content, route: "knowledge", normalization_status: "confirmed",
  provenance: { type: "asset_chunk", asset_id: versionAsset, asset_version: 1, source_event_id: event, locator: range },
});

const upstream = createServer((request, response) => {
  request.resume();
  const path = request.url;
  requests.push(path);
  const role = request.headers.authorization === "Bearer synthetic-reader" ? "reader" : "writer";
  const send = (value, status = 200) => {
    response.writeHead(status, { "Content-Type": "application/json" });
    response.end(JSON.stringify(value));
  };
  if (path === "/v1/whoami") return send({ id: asset, workspace_id: file, role });
  if (path === "/v1/context" && request.method === "POST") return send({
    budget: 4000, used_bytes: 120, effective_mode: "keyword", warnings: [], routes: [], rendered_context: quote,
    items: [
      evidence("历史内联文档", source),
      evidence("上传来源", uploadSource, uploadedAsset),
      evidence("引文不匹配", source, asset, "与来源不同"),
      evidence("字节区间切断字符", source, asset, quote, { ...locator, byte_start: locator.byte_start + 1 }),
    ],
  });
  if (path === assetPath || path === `${assetPath}?version=1`) {
    const pinned = path !== assetPath;
    return send({
      asset_id: asset, kind: "knowledge", version: pinned ? 1 : 2,
      title: pinned ? "固定历史文档" : "当前文档", content: pinned ? original : "新版替代内容",
      source_event_id: pinned ? source : currentSource, content_hash: "fixture-hash", normalization_status: "confirmed",
    });
  }
  if (path.startsWith("/v1/events/") || path.startsWith("/v1/files/")) {
    if (role === "reader") return send({ error: { message: "无权访问原文" } }, 403);
    if (path === `/v1/events/${source}`) {
      if (nextInlineSource) {
        const held = nextInlineSource;
        nextInlineSource = undefined;
        return held(send);
      }
      return send({ kind: "knowledge", content: original, file_id: null });
    }
    if (path === `/v1/events/${uploadSource}`) return send({ kind: "knowledge", content: "", file_id: file });
    if (path === `/v1/files/${file}`) {
      if (denyOriginalFile) return send({ error: { message: "该原始文件暂不可读" } }, 403);
      response.writeHead(200, { "Content-Type": "application/octet-stream" });
      return response.end(original);
    }
  }
  return send({ error: { message: "夹具路径不存在" } }, 404);
});

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return `http://127.0.0.1:${server.address().port}`;
}

async function openEvidence(page, origin, role) {
  await page.goto(`${origin}/#contexts`);
  await fixtureSignIn(page, `synthetic-${role}`, "mouse");
  await page.getByLabel("任务与查询").fill("合成发布手册");
  await page.getByRole("button", { name: "预览上下文" }).click();
  const details = (title) => page.locator("details").filter({ hasText: title });
  await details("历史内联文档").locator("summary").click();
  return details;
}

async function checkMark(detail) {
  await detail.getByRole("button", { name: "核验原始引用" }).click();
  const mark = detail.locator("mark");
  await mark.waitFor();
  assert.equal(await mark.textContent(), quote);
  assert.equal(await mark.evaluate((node) => node.parentElement.textContent), original);
}

async function checkSourceAnnouncements(page, detail) {
  const control = detail.getByRole("button", { name: "核验原始引用" });
  const status = detail.getByRole("status");
  assert.equal(await status.count(), 1, "A live region must exist before activation");
  assert.equal(await status.textContent(), "");
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Accessibility.enable");
  let statusId;
  const assertAxStatus = async (expected) => {
    await status.filter({ hasText: expected }).waitFor();
    assert.equal(await status.textContent(), expected);
    const { nodes } = await cdp.send("Accessibility.getFullAXTree");
    const byId = new Map(nodes.map((node) => [node.nodeId, node]));
    const text = (node) => !node ? "" : node.role?.value === "StaticText" ? node.name?.value : (
      node.childIds ?? []).map((id) => text(byId.get(id))).join("");
    const live = nodes.find((node) => !node.ignored && node.role.value === "status" && text(node) === expected);
    assert.ok(live, `Missing exposed AX status: ${expected}`);
    assert.equal(live.properties.find((p) => p.name === "live")?.value.value, "polite");
    assert.equal(live.properties.find((p) => p.name === "atomic")?.value.value, true);
    if (statusId !== undefined) assert.equal(live.nodeId, statusId, "Status node must survive completion and retry");
    statusId = live.nodeId;
    assert.equal(await control.evaluate((element) => element === document.activeElement), true);
  };

  const attempt = async ({ denied = false } = {}) => {
    const pending = Promise.withResolvers();
    nextInlineSource = pending.resolve;
    const timer = setTimeout(() => pending.reject(new Error("The source fixture received no request")), 5000);
    try {
      const [finish] = await Promise.all([
        pending.promise,
        (async () => {
          await control.press("Enter");
          await assertAxStatus("正在读取原始来源…");
          assert.equal(await detail.locator("mark").count(), 0, "Do not show an earlier result on a fresh read");
        })(),
      ]);
      if (denied) finish({ error: { message: "该来源暂不可读" } }, 403);
      else finish({ kind: "knowledge", content: original, file_id: null });
    } finally {
      clearTimeout(timer);
      nextInlineSource = undefined;
    }
  };
  await attempt();
  await assertAxStatus("已核对原文字节区间，已高亮引用。");
  const mark = detail.locator("mark");
  assert.equal(await mark.textContent(), quote);
  assert.equal(await mark.evaluate((node) => node.parentElement.textContent), original);
  await attempt({ denied: true });
  await assertAxStatus("原始来源读取失败：该来源暂不可读");
  assert.equal(await mark.count(), 0);
  await attempt();
  await assertAxStatus("已核对原文字节区间，已高亮引用。");
  await cdp.detach();
}

async function checkPinnedVersion(page, detail) {
  await detail.getByRole("button", { name: /· v1$/ }).click();
  await page.getByRole("heading", { name: "固定历史文档" }).waitFor();
  assert.equal(new URL(page.url()).hash, `#asset/${asset}?version=1`);
  assert.equal(await page.getByText("新版替代内容", { exact: true }).count(), 0);
  assert.equal(await page.getByText(original, { exact: true }).count(), 1);
}

(async () => {
  const { createUiServer } = await import("../dist/server.js");
  let browser, relay;
  try {
    relay = createUiServer({ apiBaseUrl: await listen(upstream), accounts: await fixtureAccounts(file, ["synthetic-reader", "synthetic-writer"]) });
    const origin = await listen(relay);
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    const writer = await browser.newPage({ viewport: { width: 375, height: 768 } });
    writer.setDefaultTimeout(5000);
    const details = await openEvidence(writer, origin, "writer");
    await checkSourceAnnouncements(writer, details("历史内联文档"));
    assert.equal(requests.includes(`/v1/files/${file}`), false);
    await details("上传来源").locator("summary").click();
    await checkMark(details("上传来源"));
    assert.ok(requests.includes(`/v1/files/${file}`));
    const uploaded = details("上传来源");
    const download = uploaded.getByRole("button", { name: "下载原始文件" });
    assert.equal(await download.count(), 1);
    denyOriginalFile = true;
    await download.press("Enter");
    await uploaded.getByRole("status").filter({ hasText: "原始文件下载失败：该原始文件暂不可读" }).waitFor();
    assert.equal(await uploaded.locator("mark").count(), 0, "A fresh denial must clear the prior raw source");
    assert.equal(await download.count(), 0);
    denyOriginalFile = false;
    for (const title of ["引文不匹配", "字节区间切断字符"]) {
      const unverified = details(title);
      await unverified.locator("summary").click();
      await unverified.getByRole("button", { name: "核验原始引用" }).click();
      const status = unverified.getByRole("status").filter({ hasText: "无法核对原文字节区间，未高亮引用。" });
      await status.waitFor();
      assert.equal(await status.textContent(), "无法核对原文字节区间，未高亮引用。");
      assert.equal(await unverified.locator("mark").count(), 0);
    }
    await checkPinnedVersion(writer, details("历史内联文档"));

    const requestCount = requests.length;
    const reader = await browser.newPage({ viewport: { width: 736, height: 768 } });
    reader.setDefaultTimeout(5000);
    const readerDetails = await openEvidence(reader, origin, "reader");
    assert.equal(await reader.getByRole("button", { name: "核验原始引用" }).count(), 0);
    await checkPinnedVersion(reader, readerDetails("历史内联文档"));
    assert.equal(requests.slice(requestCount).some((path) => /^\/v1\/(events|files)\//.test(path)), false);
    assert.deepEqual(requests.filter((path) => path.startsWith(assetPath)), Array(2).fill(`${assetPath}?version=1`));
    console.log("PASS: AX source announcements and focus, exact v1 links, UTF-8 spans, unverified ranges, reader access");
  } finally {
    await browser?.close();
    relay?.closeAllConnections();
    relay?.close();
    upstream.closeAllConnections();
    upstream.close();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
