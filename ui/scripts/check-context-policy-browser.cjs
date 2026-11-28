// Chromium regression through the built console, real API and worker, and a synthetic extraction endpoint.
const assert = require("node:assert/strict");
const { spawn } = require("node:child_process");
const { once } = require("node:events");
const { mkdtemp, rm } = require("node:fs/promises");
const { createServer } = require("node:http");
const { tmpdir } = require("node:os");
const { join } = require("node:path");
const { setTimeout: sleep } = require("node:timers/promises");
const { chromium } = require("playwright");
const { seedWorkspace } = require("./browser-fixture-workspaces.cjs");
const { createFixtureUiServer, fixtureAccounts, fixtureSignIn } = require("./browser-fixture-accounts.cjs");

const bin = process.env.ORIGENCE_BIN ?? join(__dirname, "../../target/debug/origence");
const predicate = "travel.hotel_cap";
const prefix = "合成通知 🦀：";
const source = (date, value) => `${prefix}云舟境内酒店报销上限自${date}起为每晚${value} 元。`;
const currentSource = source("2026-01-01", 100);
const futureSource = source("2026-12-01", 80);

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return `http://127.0.0.1:${server.address().port}`;
}

async function waitFor(check) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (await check()) return;
    await sleep(100);
  }
  throw new Error("Origence did not reach the expected state");
}

const model = createServer(async (request, response) => {
  const parts = [];
  for await (const part of request) parts.push(part);
  const input = JSON.parse(Buffer.concat(parts).toString());
  const { source: content } = JSON.parse(input.messages[1].content);
  const quote = content.slice(prefix.length);
  const [, date, value] = /自(\d{4}-\d{2}-\d{2})起为每晚(\d+ 元)/.exec(quote);
  const fact = {
    identity: { subject: { kind: "project", stable_id: "云舟" }, predicate, context: { region: "境内" } },
    statement: { quote, value, byte_start: Buffer.byteLength(prefix), byte_end: Buffer.byteLength(content) },
    effective_date: date,
  };
  response.writeHead(200, { "Content-Type": "application/json" });
  response.end(JSON.stringify({ choices: [{ message: { content: JSON.stringify({ facts: [fact] }) } }] }));
});

async function preview(page, origin, role, date) {
  await page.goto(`${origin}/#contexts`);
  await fixtureSignIn(page, role, "mouse");
  await page.getByLabel("任务与查询").fill("云舟境内酒店报销上限是多少？");
  await page.getByLabel("记忆业务日期（UTC）").fill(date);
  await page.getByRole("button", { name: "预览上下文" }).click();
  const memory = page.locator("details.source").filter({ hasText: `云舟 · ${predicate}` });
  await memory.locator("summary").click();
  return memory;
}

(async () => {
  const dir = await mkdtemp(join(tmpdir(), "origence-policy-browser-"));
  const reservation = createServer();
  let host, hostExit, relay, browser;
  try {
    const modelOrigin = await listen(model);
    const apiOrigin = await listen(reservation);
    await new Promise((resolve) => reservation.close(resolve));
    const writer = seedWorkspace(dir, "synthetic policy fixture");
    host = spawn(bin, ["--data-dir", dir, "serve", "--local", "--bind", new URL(apiOrigin).host], {
      env: { ...process.env, OC_ENABLE_MODELS: "true", OC_EMBEDDING_MODEL: "", OC_EXTRACTION_BASE_URL: modelOrigin, OC_EXTRACTION_MODEL: "fixture", OC_MODEL_API_KEY: "fixture", RUST_LOG: "warn" },
      stdio: ["ignore", "ignore", "inherit"],
    });
    hostExit = once(host, "exit");
    await waitFor(async () => {
      if (host.exitCode !== null) throw new Error(`Origence exited with ${host.exitCode}`);
      return (await fetch(`${apiOrigin}/health/ready`).catch(() => null))?.ok;
    });
    const api = (path, body, key, method = body ? "POST" : "GET") => fetch(apiOrigin + path, {
      method,
      headers: { Authorization: `Bearer ${writer.token}`, "Content-Type": "application/json", ...(key ? { "Idempotency-Key": key } : {}) },
      body: body && JSON.stringify(body),
    });
    const issued = await api("/admin/keys", { workspace_id: writer.workspace_id, role: "reader" });
    assert.equal(issued.status, 200);
    const reader = await issued.json();
    assert.equal((await api("/admin/memory-properties", { predicate, cardinality: "single", required_context: ["region"] }, "property")).status, 200);
    const retain = async (content, key) => {
      const response = await api("/v1/memories/retain", { content }, key);
      assert.ok(response.ok);
      const receipt = await response.json();
      await waitFor(async () => {
        const result = await (await api(`/v1/jobs/${receipt.job_id}`)).json();
        assert.notEqual(result.state, "failed");
        return result.state === "completed";
      });
      return receipt.source_event_id;
    };
    await retain(currentSource, "original");
    const futureId = await retain(futureSource, "future");

    const { createUiServer } = await import("../dist/server.js");
    const accounts = fixtureAccounts(writer.workspace_id, [writer.token, reader.token]);
    [accounts[0].username, accounts[1].username] = ["fixture-writer", "fixture-reader"];
    relay = createFixtureUiServer(createUiServer, { apiBaseUrl: apiOrigin, accounts });
    const origin = await listen(relay);
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    const writerPage = await browser.newPage();
    const writerMemory = await preview(writerPage, origin, "writer", "2026-11-30");
    await writerMemory.getByText("2026-11-30 的结论 · 100 元", { exact: true }).waitFor();
    const future = writerMemory.locator(".citation").filter({ hasText: futureId });
    const openOriginal = future.getByRole("button", { name: "打开原文" });
    await openOriginal.click();
    await future.locator("mark").waitFor();
    assert.equal(await future.locator("mark").textContent(), futureSource.slice(prefix.length));
    assert.equal(await future.locator("mark").evaluate((mark) => mark.parentElement.textContent), futureSource);

    const readerPage = await browser.newPage();
    const readerMemory = await preview(readerPage, origin, "reader", "2026-12-01");
    await readerMemory.getByText("2026-12-01 的结论 · 80 元", { exact: true }).waitFor();
    assert.equal(await readerMemory.getByRole("button", { name: "打开原文" }).count(), 0);
    const readerCitation = readerMemory.locator(".citation").filter({ hasText: futureId });
    const verifyExcerpt = readerCitation.getByRole("button", { name: "核验引文" });
    await verifyExcerpt.click();
    await readerCitation.locator("mark").waitFor();
    assert.equal(await readerCitation.locator("mark").textContent(), futureSource.slice(prefix.length));
    assert.equal(await readerCitation.getByText(prefix, { exact: false }).count(), 0);

    assert.ok((await api(`/v1/events/${futureId}`, null, "withdraw-future", "DELETE")).ok);
    await openOriginal.click();
    await future.getByRole("status").filter({ hasText: "原始来源读取失败" }).waitFor();
    assert.equal(await future.locator("mark").count(), 0);
    await verifyExcerpt.click();
    await readerCitation.getByRole("status").filter({ hasText: "无法核验引文" }).waitFor();
    assert.equal(await readerCitation.locator("mark").count(), 0);
    console.log("PASS: dated console answers, original UTF-8 source opening, reader excerpt, and withdrawal on reopen");
  } finally {
    await browser?.close();
    relay?.closeAllConnections();
    relay?.close();
    model.closeAllConnections();
    model.close();
    host?.kill();
    await hostExit;
    await rm(dir, { recursive: true, force: true });
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
