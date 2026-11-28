// Real Chromium and Origence, with disposable loopback workspaces and tokens.
const assert = require("node:assert/strict");
const { randomUUID, randomBytes } = require("node:crypto");
const { spawn } = require("node:child_process");
const { once } = require("node:events");
const { mkdtemp, rm } = require("node:fs/promises");
const { tmpdir } = require("node:os");
const { join, resolve } = require("node:path");
const { createInterface } = require("node:readline");
const { chromium } = require("playwright");
const { seedWorkspace } = require("./browser-fixture-workspaces.cjs");

const password = "console-key-fixture-password";

async function login(page, username) {
  await page.getByLabel("账号", { exact: true }).fill(username);
  await page.getByLabel("密码", { exact: true }).fill(password);
  await page.getByRole("button", { name: "登录", exact: true }).click();
}

async function run(binary) {
  const { createUiServer } = await import("../dist/server.js");
  const { hashConsolePassword } = await import("../dist/console-accounts.js");
  const directory = await mkdtemp(join(tmpdir(), "origence-key-browser-"));
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.toUpperCase().startsWith("OC_")));
  env.OC_DATA_DIR = directory;
  env.OC_ENABLE_MODELS = "false";
  const secrets = [];
  let host, ui, browser, stderr = "";
  try {
    const a = seedWorkspace(directory, "研发", undefined);
    const b = seedWorkspace(directory, "运营", a.workspace_id);
    const deploymentKey = "oc_admin_" + randomBytes(32).toString("hex");
    env.OC_DEPLOYMENT_ADMIN_KEY = deploymentKey;
    secrets.push(a.token, b.token, deploymentKey);
    host = spawn(binary, ["serve", "--local", "--bind", "127.0.0.1:0"], { cwd: directory, env });
    host.stderr.setEncoding("utf8").on("data", (chunk) => { stderr = (stderr + chunk).slice(-65536); });
    const lines = createInterface({ input: host.stdout });
    const deadline = setTimeout(() => host.kill(), 30_000);
    const [startup] = await Promise.race([
      once(lines, "line"),
      once(host, "exit").then(() => { throw new Error("Origence exited before reporting a listening address"); }),
    ]).finally(() => clearTimeout(deadline));
    const address = JSON.parse(startup).listening;
    assert.match(address, /^127\.0\.0\.1:\d+$/);
    const upstream = `http://${address}`;
    const whoami = (token, workspace) => fetch(`${upstream}/v1/whoami`, {
      headers: { Authorization: `Bearer ${token}`, ...(workspace ? { "X-Origence-Workspace": workspace } : {}) },
    });
    const call = (path, token, body) => fetch(`${upstream}${path}`, {
      method: body ? "POST" : "GET",
      headers: { Authorization: `Bearer ${token}`, ...(body && { "Content-Type": "application/json" }) },
      body: body && JSON.stringify(body),
    });
    const passwordHash = await hashConsolePassword(password);
    ui = createUiServer({ apiBaseUrl: upstream, accounts: [
      { username: "deployment", passwordHash, deploymentAuthorization: `Bearer ${deploymentKey}` },
    ] });
    ui.listen(0, "127.0.0.1");
    await once(ui, "listening");
    const base = `http://127.0.0.1:${ui.address().port}`;
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    const page = await browser.newPage({ viewport: { width: 375, height: 800 } });
    await page.goto(base);
    const open = (name) => page.getByRole("dialog", { name, exact: true });
    await page.getByRole("button", { name: "用 Key 登录", exact: true }).click();
    await page.getByLabel("访问 Key", { exact: true }).fill(a.token);
    await page.getByRole("button", { name: "使用 Key 登录", exact: true }).click();
    const nav = page.getByRole("navigation", { name: "主导航" });
    await nav.waitFor();
    assert.equal(await nav.getByRole("button", { name: "API Key" }).count(), 0);
    const denied = await page.evaluate(async (workspace) => {
      const { session } = await (await fetch("/api/console/session")).json();
      const response = await fetch(`/api/admin/deployment/workspaces/${workspace}/keys/rotate`, {
        method: "POST", headers: { "X-Origence-Session": session.binding },
      });
      return response.status;
    }, b.workspace_id);
    assert.equal(denied, 403);
    const runId = randomUUID();
    assert.equal((await call("/v1/runs", a.token, { run_id: runId, summary: "Private synthetic Run" })).status, 200);
    assert.equal((await call(`/v1/runs/${runId}/events`, a.token, {
      event_id: randomUUID(), reporter: "test", observed_at_ms: 0,
      event: { kind: "result_reported", outcome: "unknown", summary: "Synthetic result" },
    })).status, 200);
    await nav.getByRole("button", { name: "调用链路", exact: true }).click();
    await page.getByLabel("Run ID", { exact: true }).fill(runId);
    await page.getByRole("button", { name: "查看链路", exact: true }).click();
    await page.getByRole("button", { name: "选取依据并起草", exact: true }).click();
    await page.getByLabel("Skill 名称", { exact: true }).fill("verify-synthetic-output");
    await page.getByLabel("描述", { exact: true }).fill("Check an output");
    await page.getByLabel("方法步骤", { exact: true }).fill("Verify the reported output independently.");
    await page.getByLabel("选定依据（审阅者可见）", { exact: true }).fill("Browser fixture disclosed basis");
    await page.getByLabel("私有备注（可选）", { exact: true }).fill("Browser fixture private notes");
    await page.getByLabel("我确认可向管理员披露方法和选定依据").check();
    await page.getByRole("button", { name: "保存候选", exact: true }).click();
    await page.getByText("已保存修订 1。", { exact: true }).waitFor();
    const skillId = await page.getByLabel("目标 Skill ID", { exact: true }).inputValue();
    const packagePath = `/v1/skills/${skillId}/versions/1/package`;
    assert.equal(await page.getByLabel("审阅者凭据 ID", { exact: true }).count(), 0);
    await page.getByRole("button", { name: "提交审阅", exact: true }).click();
    await page.getByText("修订 1 已提交审阅。", { exact: true }).waitFor();
    await page.getByRole("button", { name: "退出", exact: true }).click();
    await login(page, "deployment");
    await page.getByRole("heading", { name: "管理员", exact: true }).waitFor();
    await page.getByRole("button", { name: "研发", exact: true }).waitFor();
    await page.getByRole("button", { name: "运营", exact: true }).waitFor();
    await page.getByRole("button", { name: "运营", exact: true }).click();
    await page.getByRole("button", { name: "Skill 审核", exact: true }).click();
    await page.getByText("暂无记录", { exact: true }).waitFor();
    await page.getByRole("button", { name: "研发", exact: true }).click();
    await page.getByRole("button", { name: "Skill 审核", exact: true }).click();
    await page.getByRole("button", { name: /修订 1 · 待审核/ }).click();
    await page.getByText("Browser fixture disclosed basis", { exact: true }).waitFor();
    assert.equal(await page.getByText("Browser fixture private notes", { exact: true }).count(), 0);
    await page.getByRole("button", { name: "批准", exact: true }).click();
    await page.getByText("已批准", { exact: true }).waitFor();
    assert.equal((await call(packagePath, a.token)).status, 404);
    await page.getByRole("button", { name: "发布 Skill", exact: true }).click();
    await page.getByText("已发布", { exact: true }).waitFor();
    assert.equal((await call(packagePath, a.token)).status, 200);
    assert.equal((await call(packagePath, b.token)).status, 404);
    await page.getByRole("button", { name: "新建工作区", exact: true }).click();
    await page.getByLabel("工作区名称", { exact: true }).fill("独立试点");
    await page.getByRole("button", { name: "创建工作区", exact: true }).click();
    const workspaceSecret = page.getByLabel("新工作区 Key", { exact: true });
    await workspaceSecret.waitFor();
    const firstKey = await workspaceSecret.inputValue();
    secrets.push(firstKey);
    const firstIdentity = await (await whoami(firstKey)).json();
    assert.equal(firstIdentity.role, "workspace");
    await page.getByRole("button", { name: "打开工作区", exact: true }).click();
    await page.getByRole("button", { name: "API Key", exact: true }).click();
    page.once("dialog", (dialog) => dialog.accept());
    await page.getByRole("button", { name: "轮换工作区 Key", exact: true }).click();
    await workspaceSecret.waitFor();
    const nextKey = await workspaceSecret.inputValue();
    secrets.push(nextKey);
    assert.equal((await (await whoami(nextKey)).json()).workspace_id, firstIdentity.workspace_id);
    assert.equal((await whoami(firstKey)).status, 401);
    assert.equal((await whoami(a.token)).status, 200);
    assert.equal((await whoami(b.token)).status, 200);
    await page.getByRole("button", { name: "知识", exact: true }).click();
    await workspaceSecret.waitFor({ state: "detached" });
    const browserData = await page.evaluate(() => JSON.stringify({ session: { ...sessionStorage }, local: { ...localStorage }, cookie: document.cookie }));
    assert.ok(secrets.every((value) => !browserData.includes(value)));
    await page.getByRole("button", { name: "新建知识", exact: true }).click();
    const createKnowledge = open("新建知识");
    await createKnowledge.getByLabel("文档标题").fill("浏览器测试知识");
    await createKnowledge.getByLabel("正文", { exact: true }).fill("合成正文第一版");
    await createKnowledge.getByRole("button", { name: "提交发布", exact: true }).click();
    await createKnowledge.getByRole("button", { name: "查看内容 →", exact: true }).click();
    await createKnowledge.waitFor({ state: "detached" });
    await page.getByText("合成正文第一版", { exact: true }).waitFor();
    await page.getByRole("button", { name: "更新内容", exact: true }).click();
    const updateKnowledge = open("更新知识");
    await updateKnowledge.getByLabel("正文", { exact: true }).fill("合成正文第二版");
    await updateKnowledge.getByRole("button", { name: "提交新版本", exact: true }).click();
    await updateKnowledge.getByRole("button", { name: "查看内容 →", exact: true }).click();
    await page.getByText("合成正文第二版", { exact: true }).waitFor();
    await page.getByLabel("读取固定历史版本", { exact: true }).fill("1");
    await page.getByRole("button", { name: "读取版本", exact: true }).click();
    await page.getByText("合成正文第一版", { exact: true }).waitFor();
    page.once("dialog", (dialog) => {
      assert.equal(dialog.type(), "prompt");
      void dialog.accept("恢复经核实的正文");
    });
    await page.getByRole("button", { name: "恢复此版本", exact: true }).click();
    await page.getByRole("button", { name: "查看内容 →", exact: true }).click();
    await page.getByText("v3", { exact: true }).waitFor();
    await page.getByText("合成正文第一版", { exact: true }).waitFor();
    await page.getByRole("tab", { name: "原始来源", exact: true }).click();
    await page.getByRole("tabpanel", { name: "原始来源", exact: true }).getByText(/合成正文第一版/).waitFor();
    page.once("dialog", (dialog) => {
      assert.match(dialog.message(), /独立试点/);
      void dialog.accept();
    });
    await page.getByRole("button", { name: "撤回来源", exact: true }).click();
    await page.getByRole("heading", { name: "内容已阻断", exact: true }).waitFor();
    await page.getByRole("button", { name: "返回列表", exact: true }).click();
    await page.getByText("暂无记录", { exact: true }).waitFor();
    await page.getByRole("button", { name: "全局知识审核", exact: true }).click();
    await page.getByRole("heading", { name: "全局知识审核", exact: true }).waitFor();
    assert.ok(secrets.every((value) => !stderr.includes(value)));
    console.log("PASS Chromium and Origence: workspace creation and key rotation, scoped Skill review and publication, knowledge create/update/restore and source withdrawal, global review");
  } finally {
    await browser?.close();
    if (ui) { ui.closeAllConnections(); ui.close(); }
    if (host && host.exitCode === null && host.signalCode === null) {
      const done = once(host, "exit");
      host.kill();
      await done;
    }
    await rm(directory, { recursive: true, force: true });
  }
}

run(resolve(process.argv[2])).catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
