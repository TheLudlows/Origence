// Browser-only regression using the built UI, relay, and loopback synthetic HTTP.
const assert = require('node:assert/strict');
const { createServer } = require('node:http');
const { once } = require('node:events');
const { resolve } = require('node:path');
const { pathToFileURL } = require('node:url');
const { chromium } = require('playwright');

async function listen(server) {
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  return 'http://127.0.0.1:' + server.address().port;
}

(async () => {
  const dist = resolve(__dirname, '../dist');
  const { createUiServer } = await import(pathToFileURL(resolve(dist, 'server.js')));
  let onKnowledgeRequest;
  const upstream = createServer((request, response) => {
    response.setHeader('Content-Type', 'application/json');
    const url = new URL(request.url, 'http://fixture.invalid');
    if (url.pathname === '/v1/whoami') return response.end(JSON.stringify({
      id: '11111111-1111-4111-8111-111111111111',
      workspace_id: '22222222-2222-4222-8222-222222222222',
      role: 'admin',
    }));
    if (url.pathname === '/health/ready') return response.end('{"status":"ready"}');
    if (url.pathname === '/v1/assets' || url.pathname === '/v1/runs') {
      const sendPage = () => response.end('{"items":[],"next_after":null}');
      if (url.searchParams.get('kind') === 'knowledge' && onKnowledgeRequest) {
        const notify = onKnowledgeRequest;
        onKnowledgeRequest = undefined;
        notify(sendPage);
        return;
      }
      return sendPage();
    }
    response.statusCode = 404;
    response.end('{}');
  });
  let ui, browser;
  try {
    ui = createUiServer({ apiBaseUrl: await listen(upstream) });
    const origin = await listen(ui);
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    for (const width of [1024, 320]) {
      const page = await browser.newPage({ viewport: { width, height: 768 } });
      page.setDefaultTimeout(4000);
      await page.goto(origin + '/#overview');
      await page.getByLabel('API key').fill('synthetic-key');
      await page.getByRole('button', { name: '连接工作空间' }).press('Enter');

      const nav = (name) => page.getByRole('navigation').getByRole('button', { name });
      const expectMain = async (title) => {
        await page.getByRole('heading', { name: title, exact: true }).waitFor();
        assert.equal(await page.evaluate(() => document.activeElement === document.querySelector('main')), true, title);
        assert.equal(await page.locator('main').getAttribute('tabindex'), '-1');
      };
      await expectMain('运行总览');
      const heldKnowledge = new Promise((resolve) => { onKnowledgeRequest = resolve; });
      await nav(/知识管理/).press('Enter');
      await expectMain('知识管理');
      const query = page.getByLabel('查询', { exact: true });
      await query.fill('持续输入');
      const response = page.waitForResponse((r) => r.url().includes('kind=knowledge'));
      const releaseKnowledge = await heldKnowledge;
      releaseKnowledge();
      await response;
      await page.getByText('没有匹配的资产', { exact: true }).waitFor();
      assert.equal(await query.evaluate((node) => node === document.activeElement), true);
      assert.equal(await query.inputValue(), '持续输入');

      await nav(/上下文工作台/).press('Enter');
      await expectMain('上下文工作台');
      const previous = await page.locator('main').elementHandle();
      await nav(/上下文工作台/).press('Enter');
      await page.waitForFunction((node) => !node.isConnected, previous);
      await expectMain('上下文工作台');
      await page.goBack();
      await expectMain('知识管理');
      await page.goForward();
      await expectMain('上下文工作台');

      await nav(/知识管理/).press('Enter');
      await expectMain('知识管理');
      const open = page.getByRole('button', { name: '＋ 新建 / 上传' });
      await open.press('Enter');
      const dialog = page.getByRole('dialog', { name: '新建知识' });
      await dialog.waitFor();
      assert.match(await dialog.ariaSnapshot(), /^- dialog "新建知识":/);
      assert.equal(await page.getByRole('button', { name: '关闭', exact: true })
        .evaluate((node) => node === document.activeElement), true);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth === innerWidth), true);
      await page.keyboard.press('Escape');
      await dialog.waitFor({ state: 'detached' });
      assert.equal(await open.evaluate((node) => node === document.activeElement), true);
      await open.press('Enter');
      await dialog.getByRole('button', { name: '关闭', exact: true }).press('Enter');
      await dialog.waitFor({ state: 'detached' });
      assert.equal(await open.evaluate((node) => node === document.activeElement), true);

      await nav(/记忆管理/).press('Enter');
      await expectMain('记忆管理');
      const capture = page.getByRole('button', { name: '从文本提取' });
      await capture.press('Enter');
      const captureDialog = page.getByRole('dialog', { name: '从文本提取记忆' });
      await captureDialog.waitFor();
      assert.match(await captureDialog.ariaSnapshot(), /^- dialog "从文本提取记忆":/);
      await page.keyboard.press('Escape');
      await captureDialog.waitFor({ state: 'detached' });
      assert.equal(await capture.evaluate((node) => node === document.activeElement), true);

      await page.evaluate(() => { location.hash = '#asset/%ZZ'; });
      await page.getByText('地址无效', { exact: true }).waitFor();
      assert.equal(await page.evaluate(() => document.activeElement.textContent.includes('地址无效')), true);
      await page.evaluate(() => { location.hash = '#asset/one?version=0'; });
      await page.getByText('版本无效', { exact: true }).waitFor();
      assert.equal(await page.evaluate(() => document.activeElement.textContent.includes('版本无效')), true);
      console.log('PASS ' + width + 'px: route focus, delayed response, same-route reload, Back/Forward, dialog names, Escape/close focus return, invalid routes, no viewport overflow');
      await page.close();
    }
  } finally {
    await browser?.close();
    ui?.closeAllConnections();
    ui?.close();
    upstream.closeAllConnections();
    upstream.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
