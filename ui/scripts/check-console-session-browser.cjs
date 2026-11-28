const assert = require("node:assert/strict");
const { once } = require("node:events");
const { createServer } = require("node:http");
const { chromium } = require("playwright");

const alpha = "11111111-1111-4111-8111-111111111111";
const beta = "22222222-2222-4222-8222-222222222222";
const excluded = "33333333-3333-4333-8333-333333333333";
const existingChild = "44444444-4444-4444-8444-444444444444";
const singleWorkspace = "55555555-5555-4555-8555-555555555555";
const workspaceKey = "oc_" + "a".repeat(64);
const deploymentKey = "oc_admin_" + "b".repeat(64);
const publication = "66666666-6666-4666-8666-666666666666";
const reviewCursor = "77777777-7777-4777-8777-777777777777";
const password = "console-fixture-密码-01";
const grants = [
  { workspaceId: alpha, name: "研发", authorization: "Bearer synthetic-alpha" },
  { workspaceId: beta, name: "运营", authorization: "Bearer synthetic-beta" },
];
const otherGrant = { workspaceId: excluded, name: "另一个账号的工作区", authorization: "Bearer synthetic-excluded" };

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return `http://127.0.0.1:${server.address().port}`;
}

async function run() {
  const { createUiServer } = await import("../dist/server.js");
  const { hashConsolePassword } = await import("../dist/console-accounts.js");
  const children = new Map([[existingChild, { workspace_id: existingChild, name: "已有子工作区" }]]);
  const creationAttempts = [];
  const knowledge = { asset_id: alpha, title: "共享运维手册", version: 1, kind: "knowledge",
    content: "发布版运维知识正文", source_event_id: excluded, content_hash: "a".repeat(64),
    identity: null, normalization_status: "not_applicable", restored_from: null };
  let submission;
  let reviewRetry = false;
  const upstream = createServer(async (req, res) => {
    const url = new URL(req.url, "http://fixture.invalid");
    const { pathname: path } = url;
    const readJson = async () => {
      const parts = [];
      for await (const part of req) parts.push(part);
      return JSON.parse(Buffer.concat(parts).toString());
    };
    const send = (value, status = 200) => {
      res.writeHead(status, { "Content-Type": "application/json" });
      res.end(JSON.stringify(value));
    };
    if (path === "/health/ready") return send({ status: "ready" });
    if (req.headers.authorization === `Bearer ${deploymentKey}`) {
      const base = "/admin/deployment/workspaces";
      const review = "/admin/deployment/global-knowledge";
      if (path === "/admin/deployment/me") return send({ role: "deployment_admin" });
      if (path === review) {
        if (submission?.state === "pending" && url.searchParams.get("state") === "pending") {
          if (url.searchParams.get("after") !== reviewCursor)
            return send({ items: [{ ...submission, publication_id: reviewCursor, title: "另一条待审核知识" }], next_after: reviewCursor });
          if (!reviewRetry) {
            reviewRetry = true;
            return send({ error: { message: "模拟审核分页失败" } }, 502);
          }
        }
        return send({ items: submission?.state === url.searchParams.get("state") ? [submission] : [], next_after: null });
      }
      if (path === `${review}/${publication}`) return send(submission);
      if (path === `${review}/${publication}/review` && req.method === "POST") {
        const { decision } = await readJson();
        submission.state = decision === "approve" ? "approved" : decision === "withdraw" ? "withdrawn" : "rejected";
        return send(submission);
      }
      if (path === base) return send({ items: [{ workspace_id: singleWorkspace, name: "Alpha space" }, { workspace_id: beta, name: "Beta space" }], next_after: null });
      if (path === `${base}/${singleWorkspace}/sessions`)
        return send({ items: [singleWorkspace], next_after: null });
      if (path === `${base}/${singleWorkspace}/sessions/${singleWorkspace}/turns`)
        return send({ turns: [{ question: "管理员可见问题", answer: "管理员查看原始会话" }], next_after: null });
      if (path === `${base}/${singleWorkspace}/runs`)
        return send({ items: [{ run_id: alpha, status: "unknown" }], next_after: null });
      if (path === `${base}/${singleWorkspace}/runs/${alpha}`)
        return send({ run: { summary: "Trace sample" }, events: [{ sequence: 1, report: { kind: "synthetic report" } }], next_after: null });
      if (path === `${base}/${singleWorkspace}/assets`)
        return send({ items: [{ asset_id: alpha, title: "Memory sample", version: 1 }], next_after: null });
      if (path === `${base}/${singleWorkspace}/assets/${alpha}`)
        return send({ content: "Recorded memory body" });
      return send({}, 404);
    }
    if (req.headers.authorization === `Bearer ${workspaceKey}`) {
      if (path === "/v1/whoami") return send({ id: singleWorkspace, workspace_id: singleWorkspace, role: "workspace" });
      if (path === "/admin/agents") return send({ workspace_id: singleWorkspace, window_seconds: 300, capacity_reached: false,
        items: [{ role: "workspace", observation: { credential_id: singleWorkspace, client_kinds: ["codex"],
          active_requests: 1, last_seen_at_ms: 1770000000000 } }] });
      if (path === `/v1/assets/${alpha}`) return send(knowledge);
      if (path === `/v1/assets/${alpha}/runs`) return send({ items: [], next_after: null });
      if (path === "/v1/assets") return send({ items: [knowledge], next_after: null });
      if (path === "/v1/search" && submission?.state === "approved")
        return send({ hits: [{ ...knowledge, asset_id: publication, scope: "global", locator: { byte_start: 0 } }] });
      if (path === "/v1/global-knowledge" && req.method === "POST") {
        assert.deepEqual(await readJson(), { asset_id: alpha, version: 1 });
        submission ??= { publication_id: publication, workspace_id: singleWorkspace, title: knowledge.title,
          version: knowledge.version, content: knowledge.content, state: "pending" };
        return send(submission);
      }
      if (path === "/v1/global-knowledge") return send({ items: submission?.state === "approved" ? [submission] : [], next_after: null });
      if (path === `/v1/global-knowledge/${publication}` && submission?.state === "approved") return send(submission);
      if (path === "/v1/session") return send({ session_id: singleWorkspace });
      if (path === `/v1/sessions/${singleWorkspace}/turns`)
        return send({ turns: [{ turn_id: singleWorkspace, question: "归档问题", answer: "归档原文" }], next_after: null, through_sequence: "1" });
      return send({}, 404);
    }
    const grant = [...grants, otherGrant].find((item) => item.authorization === req.headers.authorization);
    if (!grant) return send({}, 401);
    if (path === "/v1/whoami")
      return send({ id: grant.workspaceId, workspace_id: grant.workspaceId, role: grant.workspaceId === alpha ? "admin" : "reader" });
    if (path === "/v1/workspaces/owned")
      return grant.workspaceId === alpha ? send({ items: [...children.values()].map((child) => ({ ...child, role: "writer" })), next_after: null }) : send({}, 403);
    if (req.method === "PUT" && path.startsWith("/v1/workspaces/owned/")) {
      if (grant.workspaceId !== alpha || req.headers["x-origence-workspace"]) return send({}, 403);
      const id = path.split("/").at(-1);
      const parts = [];
      for await (const part of req) parts.push(part);
      const { name } = JSON.parse(Buffer.concat(parts).toString());
      creationAttempts.push(id);
      children.set(id, { workspace_id: id, name });
      if (creationAttempts.length === 1) return send({ error: { message: "模拟提交回执丢失" } }, 502);
      return send({ workspace_id: id });
    }
    if (path === "/v1/assets") {
      const scoped = req.headers["x-origence-workspace"];
      const child = scoped ? children.get(scoped) : undefined;
      if (scoped && (!child || grant.workspaceId !== alpha)) return send({}, 403);
      return send({ next_after: null, items: [{
        asset_id: child?.workspace_id ?? grant.workspaceId, kind: "knowledge",
        title: `${child?.name ?? grant.name}知识夹具`, version: 1, source_event_id: excluded,
      }] });
    }
    if (path === "/v1/runs") return send({ items: [], next_after: null });
    return send({}, 404);
  });
  let ui, keyUi, browser;
  try {
    const passwordHash = await hashConsolePassword(password);
    const apiBaseUrl = await listen(upstream);
    ui = createUiServer({
      apiBaseUrl,
      accounts: [
        { username: "fixture", passwordHash, grants },
        { username: "other", passwordHash, grants: [otherGrant] },
      ],
    });
    const base = await listen(ui);
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    const context = await browser.newContext({ viewport: { width: 375, height: 800 }, hasTouch: true });
    const page = await context.newPage();
    page.setDefaultTimeout(5000);
    await page.goto(base + "/#knowledge");
    const username = page.getByLabel("账号", { exact: true });
    const passwordField = page.getByLabel("密码", { exact: true });
    const accountLogin = page.getByRole("button", { name: "登录", exact: true });
    const switchToKey = page.getByRole("button", { name: "用 Key 登录", exact: true });
    const alternateKeyField = page.getByLabel("访问 Key", { exact: true });
    const focused = (control) => control.evaluate((node) => node === document.activeElement);
    await username.fill("fixture");
    assert.equal(await alternateKeyField.count(), 0);
    assert.equal(await page.locator(".login form").count(), 1);
    await passwordField.fill(password);
    await page.keyboard.press("Tab");
    assert.equal(await focused(accountLogin), true);
    await page.keyboard.press("Tab");
    assert.equal(await focused(switchToKey), true);
    await page.keyboard.press("Enter");
    assert.equal(await focused(alternateKeyField), true);
    assert.equal(await username.count(), 0);
    assert.equal(await page.locator(".login form").count(), 1);
    await alternateKeyField.fill(workspaceKey);
    await page.getByRole("button", { name: "用账号密码登录", exact: true }).tap();
    assert.equal(await focused(username), true);
    assert.equal(await username.inputValue(), "fixture");
    assert.equal(await passwordField.inputValue(), "");
    assert.equal(await alternateKeyField.count(), 0);
    await passwordField.fill(password);
    await accountLogin.click();
    await page.getByText("研发知识夹具", { exact: true }).waitFor();
    await page.getByRole("heading", { name: "已发布知识", exact: true }).waitFor();
    assert.doesNotMatch(await page.locator("main").innerText(), /资产/);
    const workspaces = page.getByRole("list", { name: "工作区", exact: true });
    const workspace = (name) => workspaces.getByRole("button", { name, exact: true });
    await workspaces.waitFor();
    assert.deepEqual(await workspaces.getByRole("button").allTextContents(), ["研发", "已有子工作区", "运营"]);
    for (const name of ["研发", "已有子工作区", "运营"]) assert.equal(await workspace(name).isVisible(), true);
    assert.equal(await workspace("研发").getAttribute("aria-current"), "true");
    assert.equal(await workspace("另一个账号的工作区").count(), 0);
    await workspace("运营").press("Enter");
    await page.getByRole("heading", { name: "运行总览", exact: true }).waitFor();
    await page.getByRole("button", { name: /知识管理/ }).click();
    await page.getByText("运营知识夹具", { exact: true }).waitFor();
    assert.equal(await page.getByText("研发知识夹具", { exact: true }).count(), 0);
    await page.reload();
    await page.getByText("运营知识夹具", { exact: true }).waitFor();
    await page.getByRole("heading", { name: "已发布知识", exact: true }).waitFor();
    assert.doesNotMatch(await page.locator("main").innerText(), /资产/);
    assert.equal(await workspace("运营").getAttribute("aria-current"), "true");
    assert.deepEqual(await page.evaluate(() => ({
      storage: { ...sessionStorage },
      localStorageSize: localStorage.length,
      readableCookies: document.cookie,
    })), {
      storage: { "origence.selectedWorkspace": beta },
      localStorageSize: 0,
      readableCookies: "",
    });
    assert.equal((await context.cookies()).find((cookie) => cookie.name.startsWith("origence-console-")).httpOnly, true);
    const newWorkspace = page.getByRole("button", { name: "新建工作区", exact: true });
    assert.equal(await newWorkspace.count(), 0);
    await workspace("研发").tap();
    await newWorkspace.click();
    const createDialog = page.getByRole("dialog", { name: "新建工作区", exact: true });
    await createDialog.getByLabel("工作区名称", { exact: true }).fill("试点");
    const create = createDialog.getByRole("button", { name: "创建工作区", exact: true });
    await create.click();
    await createDialog.getByText("模拟提交回执丢失", { exact: true }).waitFor();
    await create.click();
    await createDialog.waitFor({ state: "hidden" });
    await page.getByRole("button", { name: /知识管理/ }).click();
    await page.getByText("试点知识夹具", { exact: true }).waitFor();
    assert.deepEqual(creationAttempts, [creationAttempts[0], creationAttempts[0]]);
    assert.equal(children.size, 2);
    assert.deepEqual(await workspaces.getByRole("button").allTextContents(), ["研发", "已有子工作区", "试点", "运营"]);
    assert.equal(await newWorkspace.count(), 0);
    await page.reload();
    await page.getByText("试点知识夹具", { exact: true }).waitFor();
    assert.equal(await workspace("试点").getAttribute("aria-current"), "true");
    await page.getByRole("button", { name: "退出", exact: true }).click();
    await page.getByLabel("账号", { exact: true }).waitFor();
    await page.reload();
    await page.getByLabel("账号", { exact: true }).waitFor();
    assert.equal(await page.getByText("运营知识夹具", { exact: true }).count(), 0);
    await page.getByLabel("账号", { exact: true }).fill("other");
    await page.getByLabel("密码", { exact: true }).fill(password);
    await page.getByRole("button", { name: "登录", exact: true }).click();
    await workspaces.waitFor();
    assert.deepEqual(await workspaces.getByRole("button").allTextContents(), ["另一个账号的工作区"]);
    assert.equal(await workspace("另一个账号的工作区").getAttribute("aria-current"), "true");
    await page.getByRole("button", { name: "退出", exact: true }).click();
    await page.goto(base + "/#sessions");
    await switchToKey.tap();
    assert.equal(await alternateKeyField.inputValue(), "");
    await alternateKeyField.fill(workspaceKey);
    await page.getByRole("button", { name: "使用 Key 登录", exact: true }).click();
    await page.getByText("归档原文", { exact: true }).waitFor();
    await page.getByRole("heading", { name: "会话", exact: true }).waitFor();
    console.log("PASS single-form account/Key switch, keyboard focus, credential reset, and Key sign-in with accounts configured (synthetic HTTP, Chromium)");

    keyUi = createUiServer({ apiBaseUrl });
    const keyBase = await listen(keyUi);
    const keyPage = await context.newPage();
    keyPage.setDefaultTimeout(5000);
    await keyPage.goto(keyBase + "/#archive");
    const keyField = keyPage.getByLabel("访问 Key", { exact: true });
    await keyField.fill(workspaceKey);
    assert.equal(await keyPage.getByLabel("账号", { exact: true }).count(), 0);
    assert.equal(await keyPage.getByRole("button", { name: "用账号密码登录", exact: true }).count(), 0);
    await keyPage.getByRole("button", { name: "使用 Key 登录", exact: true }).click();
    await keyPage.waitForURL("**/#sessions");
    await keyPage.getByRole("heading", { name: "会话", exact: true }).waitFor();
    await keyPage.getByText("归档原文", { exact: true }).waitFor();
    assert.deepEqual(await keyPage.getByRole("list", { name: "工作区", exact: true }).getByRole("button").allTextContents(), [singleWorkspace]);
    assert.equal(await keyPage.getByRole("button", { name: /删除这一轮|清空会话记录|API Key/ }).count(), 0);
    await keyPage.reload();
    await keyPage.getByText("归档原文", { exact: true }).waitFor();
    assert.equal(await keyPage.evaluate(() => localStorage.length), 0);
    await keyPage.getByRole("button", { name: "Agent 连接", exact: true }).click();
    await keyPage.getByRole("cell", { name: "Codex", exact: true }).waitFor();
    console.log("PASS workspace Key sign-in, legacy bookmark redirect, and read-only session controls (synthetic HTTP, Chromium)");
    await keyPage.goto(`${keyBase}/#asset/${alpha}?version=1`);
    await keyPage.getByRole("button", { name: "提交到全局知识审核" }).click();
    await keyPage.getByText("全局知识 v1 · 待审核", { exact: true }).waitFor();
    await keyPage.getByRole("button", { name: "全局知识", exact: true }).click();
    await keyPage.getByRole("heading", { name: "暂无记录", exact: true }).waitFor();
    await keyPage.getByRole("button", { name: "退出", exact: true }).click();
    await keyField.fill(deploymentKey);
    await keyPage.getByRole("button", { name: "使用 Key 登录", exact: true }).click();
    await keyPage.getByRole("heading", { name: "部署管理", exact: true }).waitFor();
    await keyPage.getByRole("button", { name: "Beta space", exact: true }).waitFor();
    await keyPage.getByRole("button", { name: "Alpha space", exact: true }).click();
    await keyPage.getByRole("heading", { name: "会话", exact: true }).waitFor();
    await keyPage.getByRole("button", { name: /^会话 55555555/ }).click();
    await keyPage.getByText("管理员查看原始会话", { exact: true }).waitFor();
    assert.equal(await keyPage.getByRole("button", { name: /删除|清空|新建/ }).count(), 0);
    await keyPage.getByRole("button", { name: "运行", exact: true }).click();
    await keyPage.getByRole("button", { name: /11111111.*unknown/ }).click();
    await keyPage.getByText("Trace sample", { exact: true }).waitFor();
    await keyPage.getByRole("button", { name: "记忆", exact: true }).click();
    await keyPage.getByRole("button", { name: "Memory sample", exact: true }).click();
    await keyPage.getByText("Recorded memory body", { exact: true }).waitFor();
    await keyPage.getByRole("button", { name: "全局知识审核", exact: true }).click();
    await keyPage.getByText("另一条待审核知识", { exact: true }).waitFor();
    await keyPage.getByRole("button", { name: "加载更多", exact: true }).click();
    await keyPage.getByText("模拟审核分页失败", { exact: true }).waitFor();
    await keyPage.getByRole("button", { name: "重试", exact: true }).click();
    await keyPage.getByRole("button", { name: "查看 共享运维手册", exact: true }).click();
    await keyPage.getByText(knowledge.content, { exact: true }).waitFor();
    await keyPage.getByRole("button", { name: "批准发布", exact: true }).click();
    await keyPage.getByText(/v1 · 已发布/).waitFor();
    await keyPage.getByRole("button", { name: "退出", exact: true }).click();
    await keyField.fill(workspaceKey);
    await keyPage.getByRole("button", { name: "使用 Key 登录", exact: true }).click();
    await keyPage.getByRole("button", { name: "知识管理", exact: true }).click();
    await keyPage.getByLabel("查询", { exact: true }).fill("运维");
    await keyPage.getByRole("button", { name: "检索预览", exact: true }).click();
    await keyPage.getByRole("button", { name: "查看命中版本 v1", exact: true }).click();
    await keyPage.waitForURL(`**/#global-knowledge/${publication}`);
    await keyPage.getByText(knowledge.content, { exact: true }).waitFor();
    await keyPage.getByRole("button", { name: "全局知识", exact: true }).click();
    await keyPage.getByRole("button", { name: "查看 共享运维手册", exact: true }).click();
    await keyPage.getByText(knowledge.content, { exact: true }).waitFor();
    await keyPage.getByRole("button", { name: "退出", exact: true }).click();
    await keyField.fill(deploymentKey);
    await keyPage.getByRole("button", { name: "使用 Key 登录", exact: true }).click();
    await keyPage.getByRole("button", { name: "全局知识审核", exact: true }).click();
    await keyPage.getByLabel("审核状态").selectOption("approved");
    await keyPage.getByRole("button", { name: "查看 共享运维手册", exact: true }).click();
    await keyPage.getByRole("button", { name: "下架", exact: true }).click();
    await keyPage.getByText(/v1 · 已下架/).waitFor();
    console.log("PASS global knowledge submission, deployment review, browsing, and withdrawal (synthetic HTTP, Chromium)");
    console.log("PASS deployment administrator sign-in, workspace browsing, and read-only session, Run and memory views (synthetic HTTP, Chromium)");
    console.log("PASS console login, visible workspace lists with account isolation, scoped creation and retry, keyboard and mobile switching, reload, and sign-out (synthetic HTTP, Chromium)");
  } finally {
    await browser?.close();
    for (const server of [ui, keyUi, upstream]) {
      server?.closeAllConnections();
      server?.close();
    }
  }
}

run().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
