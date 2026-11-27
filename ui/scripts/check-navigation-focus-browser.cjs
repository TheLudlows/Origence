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
  const labels = ['运行总览', '调用链路', '记忆管理', '知识管理', '上下文工作台', '私有归档', 'API Key', 'Agent 连接'];
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
  await step('Shift+Tab', buttons.at(-2));
  await step('Shift+Tab', buttons.at(-3));
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
  await trend.getByRole('row', { name: '端到端耗时 P95 2,345 ms 2' }).waitFor();
  await trend.getByRole('row', { name: 'Agent Token 总量 0 1' }).waitFor();
  await trend.getByRole('row', { name: '费用估算 USD 0.000075 1' }).waitFor();
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
  assert.equal(await trend.getByText('USD 0.000075', { exact: true }).count(), 0);
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
  await trend.getByRole('row', { name: '端到端耗时 P95 未知 0' }).waitFor();
  await retry.waitFor({ state: 'hidden' });
  assert.equal(await isFocused(openTrace), true);
  assert.equal(requestCount(), 5);
}

async function checkCreationChart(page, width, hasTouch) {
  const chart = page.getByRole('figure', { name: '新建 Run 数' });
  const range = page.getByRole('combobox', { name: '统计范围' });
  const bars = chart.getByRole('button');
  const details = page.locator('details.run-creation-buckets');
  const summary = details.locator('summary');
  const readout = chart.locator('.run-chart-readout');
  const exactStart = /1970\/01\/01 GMT\+8 08:00:00\.123（含）/;
  const focusBucket = async (key, index) => {
    await page.keyboard.press(key);
    const bar = bars.nth(index);
    assert.equal(await isFocused(bar), true, `${key} should focus bucket ${index}`);
    assert.equal(await readout.innerText(), await bar.getAttribute('aria-label'));
  };

  assert.equal(await bars.count(), 7, '7-day view');
  await range.selectOption('24h');
  await page.getByText('新建运行 3 条', { exact: true }).waitFor();
  assert.equal(await bars.count(), 24);
  assert.match(await chart.locator('.run-chart-ticks').innerText(), /^08:00/);
  assert.match(await chart.locator('.run-chart-time').innerText(), /Asia\/Shanghai/);
  assert.match(await bars.first().getAttribute('aria-label'), exactStart);
  await details.locator('table').waitFor({ state: 'hidden' });
  const geometry = await chart.evaluate((node) => {
    const marks = [...node.querySelectorAll('.run-chart-mark')].slice(0, 3)
      .map((mark) => mark.getBoundingClientRect());
    const baseline = node.querySelector('.run-chart-baseline').getBoundingClientRect();
    const ticks = [...node.querySelectorAll('.run-chart-ticks span')]
      .map((tick) => tick.getBoundingClientRect());
    const chart = node.getBoundingClientRect();
    const metering = node.closest('.run-creation-result')
      .querySelector(':scope > .panel').getBoundingClientRect();
    return {
      heights: marks.map((mark) => mark.height),
      fromBaseline: marks.every((mark) => Math.abs(mark.bottom - baseline.bottom) <= 1),
      distinctTicks: ticks.slice(1).every((tick, index) => ticks[index].right < tick.left),
      chartFits: chart.right <= innerWidth &&
        document.documentElement.scrollWidth === innerWidth,
      sideBySide: Math.abs(chart.top - metering.top) < 1 && chart.right < metering.left,
      stacked: chart.bottom < metering.top,
    };
  });
  assert.ok(geometry.heights[0] > 0);
  assert.equal(geometry.heights[1], 0);
  assert.equal(geometry.heights[2], 2 * geometry.heights[0]);
  assert.ok(geometry.fromBaseline && geometry.distinctTicks && geometry.chartFits, JSON.stringify(geometry));
  assert.equal(width === 1440 ? geometry.sideBySide : geometry.stacked, true, JSON.stringify(geometry));
  await range.focus();
  await focusBucket('Tab', 0);
  assert.match(await readout.innerText(), exactStart);
  assert.ok(await bars.first().evaluate((bar) =>
    bar.matches(':focus-visible') && getComputedStyle(bar).outlineStyle === 'solid'));
  await focusBucket('ArrowRight', 1);
  assert.match(await readout.innerText(), /新建 0 个 Run$/);
  await focusBucket('End', 23);
  await focusBucket('Home', 0);
  await focusBucket('ArrowRight', 1);
  await focusBucket('ArrowLeft', 0);
  await page.keyboard.press('Tab');
  assert.equal(await isFocused(summary), true);
  await page.keyboard.press('Enter');
  await details.locator('table').waitFor();
  assert.equal(await details.getByRole('row').count(), 25);
  assert.match(await details.getByRole('cell').first().innerText(), /GMT\+8 08:00:00\.123/);
  assert.equal(await details.getByRole('row').nth(1).getByRole('cell').last().innerText(), '1');
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth === innerWidth), true);
  if (hasTouch) await bars.nth(2).tap();
  else await bars.nth(2).hover();
  assert.match(await readout.innerText(), /新建 2 个 Run$/);
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
          scope: 'workspace',
          start_ms: 123, end_ms: 123 + buckets * duration, created_runs: created,
          metering: days
            ? { duration_p95_ms: null, duration_runs: 0, agent_tokens: null, token_runs: 0, costs: [] }
            : {
                duration_p95_ms: 2345, duration_runs: 2, agent_tokens: '0', token_runs: 1,
                costs: [{ basis: 'estimated', currency: 'USD', micros: '75', runs: 1 }],
              },
          series: Array.from({ length: buckets }, (_, index) => ({
            start_ms: 123 + index * duration, end_ms: 123 + (index + 1) * duration,
            created_runs: index === 0 ? created - 2 : index === 2 ? 2 : 0,
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
    for (const width of [1440, 1024, 320]) {
      const hasTouch = width === 320;
      const page = await browser.newPage({
        viewport: { width, height: 768 },
        timezoneId: 'Asia/Shanghai',
        colorScheme: hasTouch ? 'dark' : 'light',
        hasTouch,
      });
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
      await checkCreationChart(page, width, hasTouch);
      console.log(`PASS ${width}px: creation chart and metering layout, counts and baseline, local times, keyboard, ${hasTouch ? 'emulated tap' : 'hover'}, and collapsed details`);
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
