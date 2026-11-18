// Targeted Chromium check of real built UI + relay with synthetic HTTP responses.
// Run after npm run build. Does not replace the real-Service readback tests.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { spawn } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createUiServer } from '../dist/server.js';
const run = '22222222-2222-4222-8222-222222222222';
let withdrawn = false;
const upstream = createServer((req, res) => {
  res.setHeader('Content-Type', 'application/json');
  const send = (value) => res.end(JSON.stringify(value));
  if (req.headers.authorization !== 'Bearer synthetic-owner' || withdrawn) {
    res.statusCode = 404;
    return send({ error: { message: withdrawn ? 'synthetic source withdrawn' : 'synthetic key denied' } });
  }
  if (req.url.includes('/verifications')) return send({ items: ['passed', 'failed', 'inconclusive'].map((conclusion, i) => ({
    assessment: { verification_id: run, through_sequence: 2, prefix_digest: 'a'.repeat(64),
      verifier_label: '<img src=x> external assessor', assessed_at_ms: 1000, conclusion,
      basis_summary: 'synthetic private assessment', basis_sequences: [1, 2], reported_skill_version: null },
    recorder_principal_id: run, recorded_at_ms: 2000,
    evidence_origin: 'owner_transcribed_external', verifier_authentication: 'unverified_claim',
    platform_verification: 'not_performed',
    withdrawal: i === 0 ? { reason: 'synthetic retraction', recorded_at_ms: 3000 } : null,
  })), next_after: null });
  return send({ run: { run_id: run, summary: 'synthetic private run' }, next_after: null, events: [
    { sequence: 1, recorded_at_ms: 1000, evidence_origin: 'server_prepared',
      report: { kind: 'context_prepared', citations: [{ type: 'asset_chunk', asset_version: 2 }] }, references: [] },
    { sequence: 2, recorded_at_ms: 2000, evidence_origin: 'reported',
      report: { reporter: 'user', event: { kind: 'result_reported', outcome: 'success' } }, references: [] },
  ] });
});
const delay = (ms) => new Promise(resolve => setTimeout(resolve, ms));
async function listen(server) {
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  return `http://127.0.0.1:${server.address().port}`;
}
const profile = await mkdtemp(join(tmpdir(), 'origence-verification-chromium-'));
let browser, socket, ui;
try {
  ui = createUiServer({ apiBaseUrl: await listen(upstream) });
  const origin = await listen(ui);
  browser = spawn(process.env.CHROMIUM ?? 'chromium', ['--headless', '--no-sandbox', '--disable-gpu',
    `--user-data-dir=${profile}`, '--remote-debugging-port=0', 'about:blank'], { stdio: 'ignore' });
  let port;
  for (let i = 0; i < 200; i++) {
    try { port = (await readFile(join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]; break; }
    catch { await delay(50); }
  }
  assert.ok(port, 'Chromium did not start');
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket = new WebSocket(targets.find(t => t.type === 'page').webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let id = 0;
  const pending = new Map();
  socket.onmessage = ({ data }) => {
    const response = JSON.parse(data);
    const item = pending.get(response.id);
    if (item) { pending.delete(response.id); clearTimeout(item.timer);
      response.error ? item.reject(new Error(JSON.stringify(response.error))) : item.resolve(response.result); }
  };
  const call = (method, params = {}) => new Promise((resolve, reject) => {
    const request = ++id;
    const timer = setTimeout(() => { pending.delete(request); reject(new Error(`CDP timeout: ${method}`)); }, 15000);
    pending.set(request, { resolve, reject, timer });
    socket.send(JSON.stringify({ id: request, method, params }));
  });
  const evaluate = async (expression) => {
    const result = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    assert.equal(result.exceptionDetails, undefined, JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  await call('Page.navigate', { url: origin });
  for (let i = 0; i < 100; i++) {
    if (await evaluate('location.origin === ' + JSON.stringify(origin) + ' && document.readyState === "complete"')) break;
    await delay(50);
  }
  const render = async (key) => evaluate(`(async () => {
    const { api } = await import('/client/api.js'); api.setToken(${JSON.stringify(key)});
    const { runsPage } = await import('/client/runs.js');
    document.body.replaceChildren(); const root = document.createElement('main'); document.body.append(root);
    await runsPage(root, { signal: new AbortController().signal, role: 'admin', navigate() {} }, '${run}');
    return root.innerText;
  })()`);
  const text = await render('synthetic-owner');
  for (const expected of ['服务端准备成功', '外部结论：通过', '外部结论：失败', '外部结论：无法确定',
    '已撤销的外部核验', '实际使用的 Skill 版本：未知', '平台未核验其结论', '<img src=x> external assessor']) {
    assert.ok(text.includes(expected), expected);
  }
  assert.equal(await evaluate('document.querySelectorAll("img").length'), 0);
  assert.ok(await evaluate(`document.querySelector('.timeline button:nth-child(2)').click(); document.body.innerText.includes('user；未经独立验证') && document.body.innerText.includes('success')`));
  withdrawn = true;
  await evaluate(`document.querySelectorAll('button').forEach(b => { if(b.textContent === '刷新核验记录') b.click(); })`);
  let after;
  for (let i = 0; i < 100; i++) {
    after = await evaluate('document.body.innerText');
    if (after.includes('synthetic source withdrawn')) break;
    await delay(50);
  }
  assert.ok(after.includes('synthetic source withdrawn'));
  assert.ok(!after.includes('synthetic private assessment'));
  withdrawn = false;
  const denied = await render('synthetic-other');
  assert.ok(denied.includes('synthetic key denied'));
  assert.ok(!denied.includes('synthetic private run'));
  assert.ok(!denied.includes('synthetic private assessment'));
  console.log('PASS Chromium: distinct preparation/report/external conclusions; unverified attribution; unknown version; retraction; escaped text; withdrawn-source refresh clears assessment; cross-Key denial.');
} finally {
  socket?.close();
  if (browser && browser.exitCode === null) { const stopped = once(browser, 'exit'); browser.kill(); await stopped; }
  ui?.closeAllConnections(); ui?.close(); upstream.closeAllConnections(); upstream.close();
  await rm(profile, { recursive: true, force: true });
}
