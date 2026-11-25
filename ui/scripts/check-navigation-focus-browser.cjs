// Browser-only regression using the built UI, relay, and loopback synthetic HTTP.
const { fixtureAccounts, fixtureSignIn, password } = require("./browser-fixture-accounts.cjs");
const assert = require('node:assert/strict');
const { createServer } = require('node:http');
const { once } = require('node:events');
const { resolve } = require('node:path');
const { pathToFileURL } = require('node:url');
async function listen(server, port = 0) {
  server.listen(port, '127.0.0.1');
  await once(server, 'listening');
  return 'http://127.0.0.1:' + server.address().port;
}

const overviewStatus = '合成创建量暂不可用';
const isFocused = (control) => control.evaluate((node) => document.activeElement === node);

async function connectOverview(page, origin) {
  await page.goto(origin + '/#overview');
  await fixtureSignIn(page, 'synthetic-key');
}

async function checkResponsiveNavigation(page, width) {
  const main = page.getByRole('main');
  const nav = page.getByRole('navigation', { name: '主导航' });
  const theme = page.getByRole('button', { name: '切换明暗主题' });
  const exit = page.getByRole('button', { name: '退出', exact: true });
  const labels = ['运行总览', '调用链路', '记忆管理', '知识管理', '上下文工作台', 'API Key'];
  const buttons = labels.map((label) => nav.getByRole('button', { name: new RegExp(label + '$') }));
  const context = buttons[labels.indexOf('上下文工作台')];
  const expectLayout = async () => {
    const navigation = await nav.boundingBox();
    const header = await page.locator('.topbar').boundingBox();
    assert.ok(navigation && header);
    if (width === 736) assert.ok(navigation.x + navigation.width <= header.x, 'navigation belongs beside the workspace');
    else assert.ok(navigation.y + navigation.height <= header.y, 'navigation belongs above the workspace');
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth === innerWidth), true);
  };
  const step = async (key, control) => {
    await page.keyboard.press(key);
    assert.equal(await isFocused(control), true, `Focus after ${key}: ${await control.innerText()}`);
    assert.ok(await control.evaluate((node) => {
      const { left, right, top, bottom } = node.getBoundingClientRect();
      const { outlineStyle, outlineWidth, outlineOffset } = getComputedStyle(node);
      const stroke = Number.parseFloat(outlineWidth);
      const outset = Math.max(0, stroke + Number.parseFloat(outlineOffset));
      return node.matches(':focus-visible') && outlineStyle === 'solid' && stroke > 0 &&
        left - outset >= 0 && right + outset <= innerWidth &&
        top - outset >= 0 && bottom + outset <= innerHeight;
    }), `Visible focus after ${key}: ${await control.innerText()}`);
  };

  await main.getByRole('heading', { name: '运行总览', exact: true }).waitFor();
  await main.getByText('新建运行 3 条', { exact: true }).waitFor();
  assert.equal(await isFocused(main), true);
  assert.equal(await buttons[0].getAttribute('aria-current'), 'page');
  await expectLayout();
  for (const control of [theme, exit, ...buttons.toReversed()]) await step('Shift+Tab', control);
  for (const control of [...buttons.slice(1), exit, theme]) await step('Tab', control);
  await step('Shift+Tab', exit);
  await step('Shift+Tab', buttons.at(-1));
  await step('Shift+Tab', context);
  await page.keyboard.press('Enter');
  await main.getByRole('heading', { name: '上下文工作台', exact: true }).waitFor();
  assert.equal(await isFocused(main), true);
  assert.equal(await context.getAttribute('aria-current'), 'page');
  assert.equal(await nav.locator('[aria-current="page"]').count(), 1);
  await step('Tab', main.getByLabel('任务与查询'));
  await expectLayout();
  return page.evaluate(() => getComputedStyle(document.documentElement).backgroundColor);
}

async function checkOverviewRetry(page, receiveInitial, holdOverview, requestCount) {
  const trend = page.locator('section').filter({ has: page.getByRole('heading', { name: '运行创建趋势' }) });
  const range = trend.getByRole('combobox', { name: '统计范围' });
  const status = trend.getByRole('status');
  const retry = trend.getByRole('button', { name: '重试读取创建量' });
  const error = trend.getByRole('alert').getByText(overviewStatus, { exact: true });
  const openTrace = page.getByRole('button', { name: '查看调用链 →' });
  const main = page.getByRole('main');
  const focusRetry = async () => {
    await range.focus();
    await page.keyboard.press('Tab');
    assert.equal(await isFocused(retry), true);
  };
  const focusSnapshot = (node) => node.evaluate((button) => ({
    attached: button.isConnected,
    focused: document.activeElement === button,
    activeTag: document.activeElement?.tagName,
    disabled: button.getAttribute('aria-disabled'),
    outline: button.isConnected && getComputedStyle(button).outlineStyle === 'solid',
  }));

  (await receiveInitial())(false);
  await error.waitFor();
  assert.equal(await status.innerText(), '创建量暂不可用');
  assert.equal(await isFocused(main), true);
  await focusRetry();
  const originalRetry = await retry.elementHandle();
  const receiveFailure = holdOverview();
  await page.keyboard.press('Enter');
  const sendFailure = await receiveFailure();
  assert.equal(await status.innerText(), '正在读取创建量…');
  const during = await focusSnapshot(originalRetry);
  await page.keyboard.press('Space');
  sendFailure(false);
  await error.waitFor();
  const afterFailure = {
    ...await focusSnapshot(originalRetry),
    sameButton: await retry.evaluate((node, before) => node === before, originalRetry),
  };

  // Finish the recovery probe even on the baseline that loses focus.
  if (!afterFailure.focused) await focusRetry();
  const receiveSuccess = holdOverview();
  await page.keyboard.press('Space');
  (await receiveSuccess())(true);
  await trend.getByText('新建运行 3 条', { exact: true }).waitFor();
  await retry.waitFor({ state: 'hidden' });
  const recovered = {
    focusedRange: await isFocused(range),
    activeTag: await page.evaluate(() => document.activeElement.tagName),
  };
  assert.deepEqual({ during, afterFailure, recovered }, {
    during: { attached: true, focused: true, activeTag: 'BUTTON', disabled: 'true', outline: true },
    afterFailure: { attached: true, focused: true, activeTag: 'BUTTON', disabled: null, outline: true, sameButton: true },
    recovered: { focusedRange: true, activeTag: 'SELECT' },
  });

  const receiveRangeFailure = holdOverview();
  await range.press('ArrowDown');
  (await receiveRangeFailure())(false);
  await error.waitFor();
  assert.equal(await isFocused(range), true);
  await focusRetry();
  const receiveBackgroundSuccess = holdOverview();
  await page.keyboard.press('Enter');
  const sendBackgroundSuccess = await receiveBackgroundSuccess();
  await page.keyboard.press('Shift+Tab');
  await page.keyboard.press('Shift+Tab');
  assert.equal(await isFocused(openTrace), true);
  sendBackgroundSuccess(true);
  await trend.getByText('新建运行 4 条', { exact: true }).waitFor();
  await retry.waitFor({ state: 'hidden' });
  assert.equal(await isFocused(openTrace), true);
  assert.equal(requestCount(), 5);
}

(async () => {
  const dist = resolve(__dirname, '../dist');
  const { createUiServer } = await import(pathToFileURL(resolve(dist, 'server.js')));
  const manual = process.argv.includes('--serve');
  let manualAttempts = 0;
  const pendingRequests = new Map();
  const holdRequest = (name) => {
    assert.equal(pendingRequests.has(name), false);
    const { promise, resolve } = Promise.withResolvers();
    pendingRequests.set(name, resolve);
    return async () => {
      const deadline = setTimeout(() => resolve(null), 4000);
      try {
        const send = await promise;
        assert.ok(send, `Fixture did not receive the ${name} request`);
        return send;
      } finally {
        clearTimeout(deadline);
        pendingRequests.delete(name);
      }
    };
  };
  const holdOverview = () => holdRequest('overview');
  const upstream = createServer((request, response) => {
    response.setHeader('Content-Type', 'application/json');
    const url = new URL(request.url, 'http://fixture.invalid');
    if (url.pathname === '/v1/whoami') return response.end(JSON.stringify({
      id: '11111111-1111-4111-8111-111111111111',
      workspace_id: '22222222-2222-4222-8222-222222222222',
      role: 'admin',
    }));
    if (url.pathname === '/health/ready') return response.end('{"status":"ready"}');
    if (url.pathname === '/v1/runs/overview') {
      const sendOverview = (success) => {
        if (response.destroyed) return;
        if (!success) {
          response.statusCode = 503;
          response.end(JSON.stringify({ error: { message: overviewStatus } }));
          return;
        }
        const days = url.searchParams.get('window') === '7d';
        const buckets = days ? 7 : 24;
        const duration = days ? 86_400_000 : 3_600_000;
        const created = days ? 4 : 3;
        response.end(JSON.stringify({
          start_ms: 0, end_ms: buckets * duration, created_runs: created,
          series: Array.from({ length: buckets }, (_, index) => ({
            start_ms: index * duration, end_ms: (index + 1) * duration, created_runs: index === 0 ? created : 0,
          })),
        }));
      };
      if (manual) {
        const attempt = ++manualAttempts;
        setTimeout(() => sendOverview(attempt === 3 || attempt >= 5), 1200);
      } else if (pendingRequests.has('overview')) {
        const notify = pendingRequests.get('overview');
        pendingRequests.delete('overview');
        notify(sendOverview);
      } else {
        sendOverview(true);
      }
      return;
    }
    if (url.pathname === '/v1/assets' || url.pathname === '/v1/runs') {
      const sendPage = () => response.end('{"items":[],"next_after":null}');
      if (url.searchParams.get('kind') === 'knowledge' && pendingRequests.has('knowledge')) {
        const notify = pendingRequests.get('knowledge');
        pendingRequests.delete('knowledge');
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
    ui = createUiServer({ apiBaseUrl: await listen(upstream), accounts: await fixtureAccounts('22222222-2222-4222-8222-222222222222', ['synthetic-key']) });
    const origin = await listen(ui, manual ? Number(process.env.ORIGENCE_FIXTURE_PORT ?? 0) : 0);
    if (manual) {
      console.log(origin + '/#overview · account fixture-synthetic-key · password ' + password + ' · fixture responses: 503, 503, 200, 503, then 200');
      await Promise.race([once(process, 'SIGINT'), once(process, 'SIGTERM')]);
      return;
    }
    const { chromium } = require('playwright');
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    for (const width of [1024, 320]) {
      const page = await browser.newPage({ viewport: { width, height: 768 } });
      page.setDefaultTimeout(4000);
      let overviewRequests = 0;
      page.on('request', (request) => {
        if (new URL(request.url()).pathname === '/api/v1/runs/overview') overviewRequests++;
      });
      const receiveInitial = holdOverview();
      await connectOverview(page, origin);

      const nav = (name) => page.getByRole('navigation').getByRole('button', { name });
      const expectMain = async (title) => {
        await page.getByRole('heading', { name: title, exact: true }).waitFor();
        assert.equal(await page.evaluate(() => document.activeElement === document.querySelector('main')), true, title);
        assert.equal(await page.locator('main').getAttribute('tabindex'), '-1');
      };
      await expectMain('运行总览');
      await checkOverviewRetry(page, receiveInitial, holdOverview, () => overviewRequests);
      console.log('PASS ' + width + 'px: keyboard retry, focus during latency and repeated error, recovery with and without focus on Retry');
      const receiveKnowledge = holdRequest('knowledge');
      await nav(/知识管理/).press('Enter');
      await expectMain('知识管理');
      const query = page.getByLabel('查询', { exact: true });
      await query.fill('持续输入');
      const releaseKnowledge = await receiveKnowledge();
      const response = page.waitForResponse((r) => r.url().includes('kind=knowledge'));
      releaseKnowledge();
      await response;
      await page.getByText('没有匹配的知识', { exact: true }).waitFor();
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
    for (const width of [736, 375]) {
      const backgrounds = new Map();
      for (const colorScheme of ['light', 'dark']) {
        const page = await browser.newPage({ viewport: { width, height: 768 }, colorScheme });
        page.setDefaultTimeout(4000);
        await connectOverview(page, origin);
        backgrounds.set(colorScheme, await checkResponsiveNavigation(page, width));
        await page.close();
      }
      assert.notEqual(backgrounds.get('light'), backgrounds.get('dark'), `${width}px: light and dark rendered the same page background`);
      console.log(`PASS ${width}px light/dark: distinct backgrounds, responsive navigation, Tab order in both directions, route landing and visible focus`);
    }
  } finally {
    await browser?.close();
    ui?.closeAllConnections();
    ui?.close();
    upstream.closeAllConnections();
    upstream.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
