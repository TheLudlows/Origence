// Real Chromium and Origence, with disposable loopback workspaces and tokens.
const assert = require("node:assert/strict");
const { spawn } = require("node:child_process");
const { once } = require("node:events");
const { mkdtemp, rm } = require("node:fs/promises");
const { tmpdir } = require("node:os");
const { join, resolve } = require("node:path");
const { createInterface } = require("node:readline");
const { chromium } = require("playwright");
const { randomBytes } = require("node:crypto");
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
    const a = seedWorkspace(directory, "研发");
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
    const reader = await fetch(`${upstream}/admin/keys`, {
      method: "POST", headers: { Authorization: `Bearer ${a.token}`, "Content-Type": "application/json" },
      body: JSON.stringify({ workspace_id: a.workspace_id, role: "reader" }),
    });
    assert.equal(reader.status, 200);
    assert.equal(reader.headers.get("Cache-Control"), "no-store");
    const readerKey = await reader.json();
    secrets.push(readerKey.token);
    const passwordHash = await hashConsolePassword(password);
    const grant = (workspace, token) => ({ workspaceId: workspace.workspace_id, name: workspace.name, authorization: `Bearer ${token}` });
    ui = createUiServer({ apiBaseUrl: upstream, accounts: [
      { username: "admin", passwordHash, grants: [grant({ ...a, name: "研发" }, a.token), grant({ ...b, name: "运营" }, b.token)] },
      { username: "deployment", passwordHash, grants: [], deploymentAuthorization: `Bearer ${deploymentKey}` },
      { username: "reader", passwordHash, grants: [grant({ ...a, name: "研发" }, readerKey.token)] },
    ] });
    ui.listen(0, "127.0.0.1");
    await once(ui, "listening");
    const base = `http://127.0.0.1:${ui.address().port}`;
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    const page = await browser.newPage({ viewport: { width: 375, height: 800 } });
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: base });
    await page.goto(base + "/#keys");
    await login(page, "admin");
    const open = (name) => page.getByRole("dialog", { name, exact: true });
    const workspace = (name) => page.getByRole("list", { name: "工作区" }).getByRole("button", { name, exact: true });
    const row = (id) => page.getByRole("row").filter({ hasText: id });
    const nav = page.getByRole("navigation", { name: "主导航" });
    const secret = page.getByLabel("新 API Key", { exact: true });
    await page.getByText("当前控制台凭据", { exact: true }).waitFor();
    await page.getByRole("button", { name: "创建 API Key", exact: true }).click();
    const create = open("创建 API Key");
    assert.deepEqual(await create.getByLabel("权限").locator("option").evaluateAll(
      (options) => options.map((option) => option.value)
    ), ["reader", "writer", "admin"]);
    await create.getByLabel("权限").selectOption("writer");
    await create.getByRole("button", { name: "创建", exact: true }).click();
    await open("保存新 API Key").waitFor();
    const token = await secret.inputValue();
    secrets.push(token);
    const identity = await (await whoami(token)).json();
    assert.equal(identity.role, "writer");
    assert.equal(identity.workspace_id, a.workspace_id);
    await open("保存新 API Key").getByRole("button", { name: "复制 API Key" }).click();
    await open("保存新 API Key").getByText("已复制", { exact: true }).waitFor();
    assert.ok(await page.evaluate(async (expected) => await navigator.clipboard.readText() === expected, token));
    await page.evaluate(() => navigator.clipboard.writeText(""));
    await open("保存新 API Key").getByRole("button", { name: "关闭" }).click();
    await secret.waitFor({ state: "detached" });
    await row(identity.id).waitFor();
    await row(identity.id).getByRole("button", { name: "调整权限" }).click();
    const role = open("调整工作区授权");
    await role.getByLabel("权限").selectOption("reader");
    await role.getByRole("button", { name: "保存授权" }).click();
    await row(identity.id).getByRole("cell", { name: "reader", exact: true }).waitFor();
    assert.equal((await (await whoami(token)).json()).role, "reader");
    await row(identity.id).getByRole("button", { name: "移除本区授权" }).click();
    await open("移除工作区授权").getByRole("button", { name: "确认移除" }).click();
    await row(identity.id).getByRole("button", { name: "启用本区访问" }).click();
    assert.equal((await whoami(token, a.workspace_id)).status, 403);
    await role.getByRole("button", { name: "保存授权" }).click();
    await row(identity.id).getByRole("cell", { name: "reader", exact: true }).waitFor();
    await row(identity.id).getByRole("button", { name: "轮换" }).click();
    await open("轮换 API Key").getByRole("button", { name: "确认轮换" }).click();
    await open("保存新 API Key").waitFor();
    const replacement = await secret.inputValue();
    secrets.push(replacement);
    assert.ok(replacement !== token, "rotation must emit a different token");
    assert.equal((await whoami(token)).status, 401);
    const newIdentity = await (await whoami(replacement)).json();
    assert.equal(newIdentity.role, "reader");
    await page.evaluate(() => { location.hash = "overview"; });
    await secret.waitFor({ state: "detached" });
    const browserData = await page.evaluate(() => JSON.stringify({ session: { ...sessionStorage }, local: { ...localStorage }, cookie: document.cookie }));
    assert.ok(secrets.every((value) => !browserData.includes(value)));

    await workspace("运营").click();
    await nav.getByRole("button", { name: "API Key" }).click();
    await page.getByRole("button", { name: "添加工作区授权" }).click();
    const incoming = open("添加工作区授权");
    await incoming.getByLabel("逻辑 Key ID").fill(identity.id.toUpperCase());
    await incoming.getByLabel("权限").selectOption("reader");
    await incoming.getByRole("button", { name: "保存授权" }).click();
    await row(identity.id).getByRole("button", { name: "移除授权" }).waitFor();
    assert.equal((await (await whoami(replacement, b.workspace_id)).json()).role, "reader");
    await row(identity.id).getByRole("button", { name: "调整权限" }).click();
    await role.getByLabel("权限").selectOption("writer");
    await role.getByRole("button", { name: "保存授权" }).click();
    await row(identity.id).getByRole("cell", { name: "writer", exact: true }).waitFor();
    assert.equal((await (await whoami(replacement, b.workspace_id)).json()).role, "writer");
    await row(identity.id).getByRole("button", { name: "移除授权" }).click();
    await open("移除工作区授权").getByRole("button", { name: "确认移除" }).click();
    await row(identity.id).waitFor({ state: "detached" });
    assert.equal((await whoami(replacement, b.workspace_id)).status, 403);
    await workspace("研发").click();
    await nav.getByRole("button", { name: "API Key" }).click();
    assert.match(await row(newIdentity.id).innerText(), new RegExp(identity.id));
    await row(newIdentity.id).getByRole("button", { name: "撤销" }).click();
    await open("撤销 API Key").getByRole("button", { name: "确认撤销" }).click();
    await row(newIdentity.id).waitFor({ state: "detached" });
    assert.equal((await whoami(replacement)).status, 401);

    await page.getByRole("button", { name: "退出", exact: true }).click();
    await login(page, "reader");
    await page.getByText("只有工作区管理员可以管理 API Key。", { exact: true }).waitFor();
    assert.equal(await nav.getByRole("button", { name: "API Key" }).count(), 0);
    await page.getByRole("button", { name: "退出", exact: true }).click();
    await login(page, "deployment");
    await page.getByRole("heading", { name: "部署管理", exact: true }).waitFor();
    await page.getByRole("button", { name: "研发", exact: true }).waitFor();
    await page.getByRole("button", { name: "运营", exact: true }).waitFor();
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
    await page.getByRole("button", { name: "全局知识审核", exact: true }).click();
    await page.getByRole("heading", { name: "全局知识审核", exact: true }).waitFor();
    assert.ok(secrets.every((value) => !stderr.includes(value)));
    console.log("PASS Chromium and Origence: scoped key management, deployment account sign-in, peer workspace creation and rotation, global knowledge review");
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
