const assert = require("node:assert/strict");
const { randomUUID } = require("node:crypto");
const { once } = require("node:events");
const { readFileSync } = require("node:fs");
const { chromium } = require("playwright");
const { createFixtureUiServer, fixtureSignIn } = require("./browser-fixture-accounts.cjs");

const fixture = JSON.parse(readFileSync(0, "utf8"));
const { run, keys } = fixture;
const owner = keys.writer.token, reviewer = keys.reviewer.token, reader = keys.reader.token;
const runPath = `/v1/runs/${run}`;
const raw = "PRIVATE_SYNTHETIC_RESULT";
const later = "PRIVATE_SYNTHETIC_LATER_EVENT";
const notes = "PRIVATE_SYNTHETIC_NOTES";
const firstBasis = "合成核对：固定版本给出了金额上限。";
const secondBasis = "合成复核：已对照金额与固定版本。";
const method = "逐条核对规则与请求金额。";

async function call(token, path, { method = "GET", data, status = 200 } = {}) {
  const response = await fetch(fixture.api + path, {
    method,
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json",
      "Idempotency-Key": randomUUID() },
    body: data === undefined ? undefined : JSON.stringify(data),
  });
  const value = await response.json();
  assert.equal(response.status, status, `${method} ${path}: ${JSON.stringify(value)}`);
  return value;
}

const post = (token, path, data, status = 200) => call(token, path, { method: "POST", data, status });
const report = (summary) => post(owner, `${runPath}/events`, {
  event_id: randomUUID(), reporter: "test", observed_at_ms: 1,
  event: { kind: "result_reported", outcome: "success", summary },
});

async function seed() {
  await post(owner, "/v1/runs", { run_id: run, summary: "PRIVATE_SYNTHETIC_RUN" });
  const prepared = await post(owner, `${runPath}/context`, {
    event_id: randomUUID(), context: { query: fixture.query, mode: "keyword" },
  });
  assert.ok(prepared.bundle.items.some(item => item.provenance.source_event_id === fixture.source));
  await report(raw);
  return call(owner, `${runPath}/prefix?through_sequence=2`);
}

const byLabel = (page, name) => page.getByLabel(name, { exact: true });
const byButton = (page, name) => page.getByRole("button", { name, exact: true });
const byText = (page, text) => page.getByText(text, { exact: true });
const editor = (page) => page.locator("section").filter({
  has: page.getByRole("heading", { name: "Skill 候选", exact: true }),
});
const touched = (page, method, path) => page.waitForResponse(response =>
  response.request().method() === method && new URL(response.url()).pathname === `/api${path}`);

async function submitReview(page, candidate, expectedRevision, basis) {
  const receipt = page.waitForResponse(response => response.request().method() === "POST" &&
    new URL(response.url()).pathname.startsWith(`/api${candidate}/reviews/`));
  await byButton(page, "提交审阅").click();
  const response = await receipt;
  const packet = await response.json();
  assert.equal(response.status(), 200, JSON.stringify(packet));
  await byText(page, `修订 ${expectedRevision} 已提交审阅。`).waitFor();
  assert.equal(await byLabel(page, "审阅 ID").inputValue(), packet.review_id);
  assert.equal(packet.basis, basis);
  assert.equal(packet.revision, expectedRevision);
  const visible = JSON.stringify(packet);
  for (const hidden of [notes, raw, later, run, "PRIVATE_SYNTHETIC_RUN", fixture.source]) {
    assert.ok(!visible.includes(hidden), "Review included unselected source material");
  }
  return packet;
}

async function checkConsole(page, prefix) {
  await byButton(page, "调用链路").click();
  await page.getByRole("row").filter({ hasText: "PRIVATE_SYNTHETIC_RUN" })
    .getByRole("button", { name: "查看链路 →", exact: true }).click();
  await page.locator('.timeline button[data-sequence="2"]').click();
  const read = page.waitForResponse(response => response.url().endsWith(`${runPath}/prefix?through_sequence=2`));
  await byButton(page, "选取依据并起草").click();
  assert.deepEqual(await (await read).json(), prefix);
  await editor(page).getByText(prefix.digest, { exact: false }).waitFor();
  await page.setViewportSize({ width: 320, height: 820 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth), 320);
  assert.equal(await byLabel(page, "选定依据（审阅者可见）").inputValue(), "");
  assert.equal(await byLabel(page, "方法步骤").inputValue(), "");
  assert.ok(!(await editor(page).innerText()).includes(raw));
  const id = await byLabel(page, "候选 ID").inputValue();
  const candidate = `/v1/skill-candidates/${id}`;
  await call(owner, candidate, { status: 404 });
  await report(later);
  await page.locator('.timeline button[data-sequence="1"]').click();
  assert.ok((await editor(page).innerText()).includes("固定 Run 前缀截至事件 2"));
  for (const [label, value] of [["Skill 名称", "synthetic-review"], ["描述", "核对合成报销结果"],
    ["方法步骤", method], ["选定依据（审阅者可见）", firstBasis], ["私有备注（可选）", notes]]) {
    await byLabel(page, label).fill(value);
  }
  const confirmation = byLabel(page, "我确认可向指定审阅者披露方法和选定依据");
  await byButton(page, "保存候选").click();
  await call(owner, candidate, { status: 404 });
  await confirmation.check();
  let committed;
  await page.route(`**/api${candidate}`, async (route) => {
    committed = await (await route.fetch()).json();
    await route.abort("failed");
  }, { times: 1 });
  await byButton(page, "保存候选").click();
  await editor(page).getByRole("alert").waitFor();
  assert.deepEqual(committed, { candidate_id: id, revision: 1 });
  const put = touched(page, "PUT", candidate);
  await byButton(page, "保存候选").click();
  const saved = await put;
  assert.deepEqual(await saved.json(), { candidate_id: id, revision: 1 });
  const selection = saved.request().postDataJSON().candidate;
  assert.deepEqual(selection.source, { kind: "run", ...prefix });
  for (const hidden of [raw, later, "PRIVATE_SYNTHETIC_RUN", fixture.source]) {
    assert.ok(!JSON.stringify(selection).includes(hidden));
  }
  await call(owner, `/v1/skill-candidates/${randomUUID()}`, {
    method: "PUT", data: { expected_revision: 0, candidate: { ...selection, disclosure_confirmed: false } }, status: 422,
  });
  await call(reader, `/v1/skill-candidates/${randomUUID()}`, {
    method: "PUT", data: { expected_revision: 0, candidate: selection }, status: 403,
  });
  await call(owner, `/v1/skill-candidates/${randomUUID()}`, {
    method: "PUT", data: { expected_revision: 0, candidate: { ...selection, source: { ...selection.source,
      through_sequence: 3 } } }, status: 404,
  });
  await call(reviewer, candidate, { status: 404 });
  await call(reviewer, runPath, { status: 404 });
  await call(fixture.admin, candidate, { status: 404 });

  await byLabel(page, "审阅者凭据 ID").fill(keys.reviewer.key_id);
  const skill = await byLabel(page, "目标 Skill ID").inputValue();
  const first = await submitReview(page, candidate, 1, firstBasis);
  const firstReview = `/v1/skill-reviews/${first.review_id}`;
  await post(owner, `${candidate}/reviews/${randomUUID()}`, {
    reviewer_id: keys.reviewer.key_id, revision: first.revision, target: first.target, basis: secondBasis,
  }, 409);
  await call(reader, firstReview, { status: 404 });
  assert.equal((await call(reviewer, firstReview)).decision, "pending");
  await post(reviewer, `${firstReview}/decision`, { digest: "0".repeat(64), approve: true }, 409);
  await post(reviewer, `${firstReview}/decision`, { digest: first.digest, approve: true });

  await page.reload();
  await byLabel(page, "已有候选 ID").fill(id);
  await byButton(page, "打开候选").click();
  await byText(page, "已打开修订 1。").waitFor();
  assert.equal(await byLabel(page, "私有备注（可选）").inputValue(), notes);
  assert.equal(await byLabel(page, "选定依据（审阅者可见）").inputValue(), firstBasis);
  assert.ok(await byButton(page, "保存候选").isDisabled());
  await byLabel(page, "选定依据（审阅者可见）").fill(secondBasis);
  assert.ok(await byButton(page, "提交审阅").isDisabled());
  assert.equal(await confirmation.isChecked(), false);
  await confirmation.check();
  await byButton(page, "保存候选").click();
  await byText(page, "已保存修订 2，旧批准失效。").waitFor();
  await post(reviewer, `${firstReview}/publish`, { digest: first.digest }, 409);

  await byLabel(page, "审阅者凭据 ID").fill(keys.reviewer.key_id);
  await byLabel(page, "目标 Skill ID").fill(skill);
  const second = await submitReview(page, candidate, 2, secondBasis);
  assert.ok(!JSON.stringify(second).includes(firstBasis));
  const secondReview = `/v1/skill-reviews/${second.review_id}`;
  const latest = await call(owner, candidate);
  assert.deepEqual(latest.input.sources, [{ kind: "run", ...prefix }]);
  await post(reviewer, `${secondReview}/decision`, { digest: second.digest, approve: true });
  await post(reader, `${secondReview}/publish`, { digest: second.digest }, 403);
  const packagePath = `/v1/skills/${skill}/versions/1/package`;
  await call(reader, packagePath, { status: 404 });
  const published = await post(owner, `${secondReview}/publish`, { digest: second.digest });
  assert.deepEqual(await post(reviewer, `${secondReview}/publish`, { digest: second.digest }), published);
  assert.deepEqual(await call(reader, packagePath), published);
  assert.ok(published.files["SKILL.md"].includes(method));
  for (const hidden of [firstBasis, secondBasis, notes, raw, later, run, fixture.source]) {
    assert.ok(!JSON.stringify(published).includes(hidden), "Package included private review/source material");
  }
  await call(fixture.outsider, packagePath, { status: 404 });

  await call(fixture.admin, `/v1/events/${fixture.source}`, { method: "DELETE" });
  await call(reader, packagePath, { status: 404 });
  await post(reviewer, `${secondReview}/publish`, { digest: second.digest }, 404);
  await page.reload();
  await page.getByRole("alert").waitFor();
  assert.equal(await byButton(page, "选取依据并起草").count(), 0);
  assert.equal(await byLabel(page, "私有备注（可选）").count(), 0);
}

async function main() {
  const prefix = await seed();
  const { createUiServer } = await import("../dist/server.js");
  const ui = createFixtureUiServer(createUiServer, { apiBaseUrl: fixture.api, accounts: [
    { username: "fixture-owner", workspaceId: fixture.workspace, key: owner },
  ] });
  let browser;
  try {
    ui.listen(0, "127.0.0.1");
    await once(ui, "listening");
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
    const page = await browser.newPage();
    page.setDefaultTimeout(10_000);
    await page.goto(`http://127.0.0.1:${ui.address().port}`);
    await fixtureSignIn(page, "owner");
    await checkConsole(page, prefix);
    console.log("PASS Chromium → authenticated console → Origence: exact prefix, manual disclosure, reopen and edit, stale approval, publish and download isolation, source withdrawal.");
  } finally {
    await browser?.close();
    ui.closeAllConnections();
    ui.close();
  }
}

main().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
