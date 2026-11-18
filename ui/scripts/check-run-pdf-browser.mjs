// Real Chromium and built UI/relay. Upstream fixture isolates browser behavior.
// The matching Service/HTTP authorization checks are in tests/run_pdf_source.rs.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createUiServer } from "../dist/server.js";

const run = "33333333-3333-4333-8333-333333333333";
const asset = "11111111-1111-4111-8111-111111111111";
const source = "22222222-2222-4222-8222-222222222222";
const definite = "44444444-4444-4444-8444-444444444444";
const uncertain = "55555555-5555-4555-8555-555555555555";
const original = pdf(["V1 cover", "V1 historical needle", ""]);
const latest = pdf(["V2 substituted source"]);
const archivedReference = (id, version, page) => ({
  evidence_ref_id: id,
  source_event_id: source,
  reference: { chunk_id: id, asset_id: asset, asset_version: version },
  locator: { parser: "pdf-oxide-0.3.78-v1", page, byte_start: 3, byte_end: 20 },
});
const requests = [];
let withdrawn = false;
const upstream = createServer((req, res) => {
  const target = new URL(req.url, "http://127.0.0.1");
  requests.push(target.pathname + target.search);
  const send = (value) => {
    res.setHeader("Content-Type", "application/json");
    res.end(JSON.stringify(value));
  };
  if (target.pathname.endsWith("/pdf-source")) {
    if (withdrawn || req.headers.authorization !== "Bearer synthetic-owner") {
      res.statusCode = 404;
      return send({ error: { message: "fixture source unavailable" } });
    }
    const id = target.searchParams.get("evidence_ref_id");
    assert.equal(target.searchParams.get("sequence"), id === definite ? "1" : "2");
    assert.equal(target.searchParams.has("version"), false);
    if (target.searchParams.has("original")) {
      res.setHeader("Content-Type", "application/octet-stream");
      return res.end(original);
    }
    return send(id === definite
      ? { asset_version: 1, page: 2, span: { before: "V1 ", highlight: "historical needle", after: "" } }
      : { asset_version: 1, page: 3, span: null });
  }
  if (target.pathname.includes("/assets/")) return res.end(latest);
  if (target.pathname.endsWith("/verifications")) return send({ items: [], next_after: null });
  const precise = archivedReference(definite, 1, 2);
  return send({
    run: { run_id: run, summary: "synthetic PDF" },
    events: [
      { sequence: 1, recorded_at_ms: 1000, evidence_origin: "server_prepared",
        report: { kind: "context_prepared", citations: [{ type: "asset_chunk", chunk_id: definite,
          asset_id: asset, asset_version: 1, source_event_id: source, locator: precise.locator }] },
        references: [precise] },
      { sequence: 2, recorded_at_ms: 2000, evidence_origin: "reported",
        report: { reporter: "client", event: { kind: "context_reported" } },
        references: [archivedReference(uncertain, 1, 3)] },
    ],
    next_after: null,
  });
});

function pdf(pages) {
  const kids = pages.map((_, i) => `${4 + i * 2} 0 R`).join(" ");
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    `<< /Type /Pages /Kids [${kids}] /Count ${pages.length} >>`,
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
  ];
  for (const [i, text] of pages.entries()) {
    const content = `BT /F1 12 Tf 72 720 Td (${text}) Tj ET`;
    objects.push(
      `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents ${5 + i * 2} 0 R >>`,
      `<< /Length ${content.length} >>\nstream\n${content}\nendstream`,
    );
  }
  let data = "%PDF-1.4\n";
  const offsets = [];
  for (const [i, value] of objects.entries()) {
    offsets.push(data.length);
    data += `${i + 1} 0 obj\n${value}\nendobj\n`;
  }
  const xref = data.length;
  data += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const offset of offsets) data += `${String(offset).padStart(10, "0")} 00000 n \n`;
  return data + `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
}

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return `http://127.0.0.1:${server.address().port}`;
}
async function until(check) {
  for (let i = 0; i < 200; i++) {
    if (await check()) return;
    await delay(50);
  }
  assert.fail("Timed out waiting for Chromium");
}

const profile = await mkdtemp(join(tmpdir(), "origence-pdf-chromium-"));
let browser, socket, relay;
try {
  relay = createUiServer({ apiBaseUrl: await listen(upstream) });
  const origin = await listen(relay);
  browser = spawn(process.env.CHROMIUM ?? "chromium", [
    "--headless", "--no-sandbox", "--disable-gpu",
    `--user-data-dir=${profile}`, "--remote-debugging-port=0", "about:blank",
  ], { stdio: "ignore" });
  let port;
  await until(async () => {
    try {
      port = (await readFile(join(profile, "DevToolsActivePort"), "utf8")).split("\n")[0];
      return Boolean(port);
    } catch { return false; }
  });
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket = new WebSocket(targets.find((target) => target.type === "page").webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let requestId = 0;
  const pending = new Map();
  socket.onmessage = ({ data }) => {
    const response = JSON.parse(data);
    const request = pending.get(response.id);
    if (!request) return;
    pending.delete(response.id);
    clearTimeout(request.timer);
    if (response.error) request.reject(new Error(JSON.stringify(response.error)));
    else request.resolve(response.result);
  };
  const command = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++requestId;
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`CDP timeout: ${method}`));
    }, 15_000);
    pending.set(id, { resolve, reject, timer });
    socket.send(JSON.stringify({ id, method, params }));
  });
  async function evaluate(expression) {
    const result = await command("Runtime.evaluate", {
      expression, awaitPromise: true, returnByValue: true, userGesture: true,
    });
    assert.equal(result.exceptionDetails, undefined, JSON.stringify(result.exceptionDetails));
    return result.result.value;
  }
  await command("Page.navigate", { url: origin });
  await until(() => evaluate(`location.origin === ${JSON.stringify(origin)} && document.readyState === "complete"`));
  await evaluate(`(async () => {
    const { api } = await import('/client/api.js'); api.setToken('synthetic-owner');
    const { runsPage } = await import('/client/runs.js');
    const makeUrl = URL.createObjectURL;
    URL.createObjectURL = (blob) => {
      window.openedPdf = blob.text();
      window.openedType = blob.type;
      return makeUrl(blob);
    };
    const root = document.createElement('main');
    document.body.replaceChildren(root);
    window.clickLabel = (label) => [...root.querySelectorAll('button')].find((b) => b.textContent === label).click();
    await runsPage(root, { signal: new AbortController().signal, role: 'admin', navigate() {} }, '${run}');
  })()`);

  await evaluate("clickLabel('核验 PDF 页内引用')");
  await until(() => evaluate("document.querySelector('mark')?.textContent === 'historical needle'"));
  assert.ok(await evaluate("document.body.innerText.includes('v1 · PDF 第 2 页')"));
  await evaluate("clickLabel('打开原件第 2 页')");
  await until(() => evaluate("Boolean(window.openedPdf)"));
  assert.equal(await evaluate("window.openedType"), "application/pdf");
  assert.equal(await evaluate("window.openedPdf"), original);
  await until(async () => {
    const { targetInfos } = await command("Target.getTargets");
    return targetInfos.some(({ url }) => url.startsWith("blob:") && url.endsWith("#page=2"));
  });

  await evaluate("document.querySelector('.timeline button[data-sequence=\"2\"]').click()");
  await evaluate("clickLabel('核验 PDF 页内引用')");
  await until(() => evaluate("document.body.innerText.includes('缺少可核验的页内定位')"));
  assert.ok(await evaluate("document.body.innerText.includes('v1 · PDF 第 3 页')"));
  assert.equal(await evaluate("document.querySelectorAll('mark').length"), 0);
  await evaluate("clickLabel('打开原件第 3 页')");
  await until(async () => {
    const { targetInfos } = await command("Target.getTargets");
    return targetInfos.some(({ url }) => url.startsWith("blob:") && url.endsWith("#page=3"));
  });
  assert.equal(await evaluate("window.openedPdf"), original);

  withdrawn = true;
  await evaluate("clickLabel('打开原件第 3 页')");
  await until(() => evaluate("document.body.innerText.includes('fixture source unavailable')"));
  assert.equal(await evaluate("document.querySelectorAll('mark').length"), 0);
  assert.ok(!await evaluate("document.body.innerText.includes('打开原件第 3 页')"));
  assert.ok(!requests.some((path) => path.includes("/assets/") || path.includes("/files/")));
  console.log("PASS Chromium: archived v1 text and original #page=2, page-only fallback #page=3, withdrawal blocks a new open");
} finally {
  if (browser && browser.exitCode === null) {
    const stopped = once(browser, "exit");
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ id: 1000000, method: "Browser.close" }));
    } else browser.kill();
    await stopped;
  }
  socket?.close();
  relay?.closeAllConnections();
  relay?.close();
  upstream.closeAllConnections();
  upstream.close();
  await rm(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
