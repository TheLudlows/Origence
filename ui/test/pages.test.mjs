import { test } from "node:test";
import assert from "node:assert/strict";
import { Window } from "happy-dom";

test("five-page DOM flows preserve API contracts, retry identity, version links and logout", async (t) => {
  const window = new Window({ url: "http://localhost:3000" });
  const original = new Map();
  for (const key of [
    "window",
    "document",
    "Node",
    "HTMLElement",
    "location",
    "Event",
    "SubmitEvent",
    "matchMedia",
    "confirm",
    "prompt",
  ]) {
    original.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, {
      configurable: true,
      writable: true,
      value:
        key === "confirm"
          ? () => true
          : key === "prompt"
            ? () => "test restore"
            : key === "matchMedia"
              ? window.matchMedia.bind(window)
              : window[key],
    });
  }
  document.body.innerHTML = '<div id="app"></div>';
  if (!window.HTMLDialogElement.prototype.showModal)
    window.HTMLDialogElement.prototype.showModal = function () {
      this.open = true;
    };
  if (!window.HTMLDialogElement.prototype.close)
    window.HTMLDialogElement.prototype.close = function () {
      this.open = false;
      this.dispatchEvent(new window.Event("close"));
    };
  const ids = {
    asset: "11111111-1111-4111-8111-111111111111",
    run: "22222222-2222-4222-8222-222222222222",
    source: "33333333-3333-4333-8333-333333333333",
    job: "44444444-4444-4444-8444-444444444444",
  };
  let verificationDenied = false;
  const asset = {
    asset_id: ids.asset,
    title: "<img src=x>",
    kind: "memory",
    version: 1,
    content: "中文 🦀 é",
    identity: {
      subject: { kind: "service", stable_id: "Atlas" },
      predicate: "release.rule",
      context: { env: "production" },
    },
    normalization_status: "explicit_identity",
    source_event_id: ids.source,
    content_hash: "a".repeat(64),
    restored_from: null,
  };
  const writes = [],
    keys = [];
  const searches = new Map();
  let failMemory = true,
    unauthorized = false;
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (url, options = {}) => {
    const path = new URL(url, "http://localhost:3000"),
      body =
        typeof options.body === "string"
          ? JSON.parse(options.body)
          : options.body;
    const response = (value, status = 200) =>
      new Response(JSON.stringify(value), {
        status,
        headers: { "content-type": "application/json" },
      });
    if (unauthorized)
      return response({ error: { message: "authentication required" } }, 401);
    assert.equal(
      options.headers.get("Authorization"),
      "Bearer disposable-fixture-key",
    );
    if (path.pathname === "/api/v1/whoami")
      return response({
        id: ids.source,
        workspace_id: ids.asset,
        role: "admin",
      });
    if (path.pathname === "/api/health/ready")
      return response({ status: "ready" });
    if (path.pathname === "/api/v1/assets")
      return response({
        items: [asset].filter((a) => a.kind === path.searchParams.get("kind")),
        next_after: null,
      });
    if (path.pathname === "/api/v1/runs")
      return response({
        items: [
          { run_id: ids.run, summary: "fixture task", created_at_ms: 1000 },
        ],
        next_after: null,
      });
    if (path.pathname === `/api/v1/runs/${ids.run}/verifications` && verificationDenied)
      return response({ error: { message: "source unavailable" } }, 404);
    if (path.pathname === `/api/v1/runs/${ids.run}/verifications`)
      return response({ items: [{
        assessment: { verification_id: ids.source, through_sequence: 2, prefix_digest: "d".repeat(64),
          verifier_label: "<img src=x> external tester", assessed_at_ms: 1000, conclusion: "failed",
          basis_summary: "synthetic missing output", basis_sequences: [1, 2], reported_skill_version: null },
        recorder_principal_id: ids.run, recorded_at_ms: 2000,
        withdrawal: { reason: "superseded evidence", recorded_at_ms: 3000 },
      }], next_after: null });
    if (path.pathname === `/api/v1/runs/${ids.run}`)
      return response({
        run: { run_id: ids.run, summary: "fixture task" },
        events: [
          {
            sequence: 1,
            recorded_at_ms: 1000,
            evidence_origin: "server_prepared",
            report: {
              kind: "context_prepared",
              reporter: "server",
              citations: [
                {
                  type: "asset_chunk",
                  asset_id: ids.asset,
                  asset_version: 1,
                  source_event_id: ids.source,
                  locator: {},
                },
              ],
            },
            references: [],
          },
          ...[
            ["server_observed", "server", "context_preparation_failed", "not_prepared"],
            ["reported", "client", "result_reported", undefined],
            ["future_origin", "client", "future_event", undefined],
          ].map(([evidence_origin, reporter, kind, delivery_status], index) => ({
            sequence: index + 2,
            recorded_at_ms: 1001 + index,
            evidence_origin,
            report: { reporter, kind, delivery_status },
            references: [],
          })),
        ],
        next_after: null,
      });
    if (path.pathname === `/api/v1/assets/${ids.asset}`) return response(asset);
    if (path.pathname === "/api/v1/memories/identified") {
      writes.push(body);
      keys.push(options.headers.get("Idempotency-Key"));
      if (failMemory) {
        failMemory = false;
        return response({ error: { message: "dependency unavailable" } }, 503);
      }
      assert.equal(body.expected_version, 0);
      assert.equal(body.identity.context.env, "production");
      return response({
        job_id: ids.job,
        asset_id: ids.asset,
        state: "accepted",
      });
    }
    if (path.pathname === `/api/v1/events/${ids.source}`)
      return response({ content: "source text is not verified PDF page text", file_id: ids.source });
    if (path.pathname === `/api/v1/assets/${ids.asset}/runs`) {
      assert.equal(path.searchParams.get("version"), "1");
      return response({items:[{run_id:ids.run,created_at_ms:1}],next_after:null,scope:"current_logical_key",coverage:"recorded_references"});
    }
    if (path.pathname === "/api/v1/files") {
      assert.equal(
        options.headers.get("Content-Type"),
        "application/octet-stream",
      );
      assert.equal(await body.text(), "# 中文 🦀 手册");
      assert.equal(path.searchParams.get("format"), "markdown");
      return response({ file_id: ids.source });
    }
    if (path.pathname === "/api/v1/knowledge") {
      assert.equal(body.asset_id, null);
      assert.equal(body.expected_version, null);
      writes.push(body);
      return response({ job_id: ids.job, asset_id: ids.asset });
    }
    if (path.pathname === "/api/v1/search") {
      const pending = Promise.withResolvers();
      searches.set(body.query, pending);
      return pending.promise;
    }
    if (path.pathname === `/api/v1/jobs/${ids.job}`)
      return response({ state: "completed", outcome: "published" });
    if (path.pathname === "/api/v1/context") {
      assert.equal(body.budget, 4000);
      return response({
        budget: 4000,
        used_bytes: 32,
        routes: [
          {
            route: "memory",
            retained: 1,
            used_bytes: 32,
            omitted_for_budget: 0,
          },
        ],
        items: [
          {
            title: asset.title,
            route: "memory",
            content: asset.content,
            provenance: {
              type: "asset_chunk",
              asset_id: ids.asset,
              asset_version: 1,
            },
            normalization_status: "explicit_identity",
          },
        ],
        warnings: [],
        effective_mode: "keyword",
        requested_mode: "keyword",
        rendered_context: asset.content,
      });
    }
    throw new Error("Unexpected fixture request: " + path.pathname);
  };
  t.after(async () => {
    globalThis.fetch = originalFetch;
    for (const [key, descriptor] of original) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
    await window.happyDOM.close();
  });
  const wait = async (condition) => {
    for (let n = 0; n < 100; n++) {
      if (condition()) return;
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    assert.fail("DOM state did not settle");
  };
  const click = (text, scope = document) => {
    const node = [...scope.querySelectorAll("button")].find(
      (b) => b.textContent.trim() === text,
    );
    assert.ok(node, `Missing button ${text}`);
    node.click();
  };
  const control = (text, scope = document) => {
    const label = [...scope.querySelectorAll("label")].find(
      (l) => l.querySelector("span")?.textContent === text,
    );
    assert.ok(label, `Missing field ${text}`);
    return label.querySelector("input,select,textarea");
  };
  await import("../dist/client/app.js");
  await t.test("login and real-scope overview", async () => {
    control("API key").value = "disposable-fixture-key";
    document.querySelector("form").requestSubmit();
    await wait(
      () =>
        document.querySelector("nav") &&
        document.body.textContent.includes("fixture task"),
    );
    assert.equal(document.querySelector("h1").textContent, "运行总览");
    assert.equal(window.localStorage.length, 0);
    assert.equal(window.sessionStorage.length, 0);
  });
  await t.test(
    "memory form retries the same operation without silent version overwrite",
    async () => {
      click("◇记忆管理");
      await wait(
        () => document.querySelector("h1")?.textContent === "记忆管理",
      );
      click("＋ 新建记忆");
      const modal = document.querySelector("dialog");
      control("主体稳定 ID", modal).value = "Atlas";
      control("属性", modal).value = "release.rule";
      control("适用条件（字符串键值）", modal).value = '{"env":"production"}';
      control("事实正文 / 原始引用", modal).value = "中文 🦀 é";
      modal.querySelector("form").requestSubmit();
      await wait(() => modal.textContent.includes("dependency unavailable"));
      modal.querySelector("form").requestSubmit();
      await wait(() => modal.textContent.includes("completed"));
      assert.equal(keys.length, 2);
      assert.equal(keys[0], keys[1]);
      assert.deepEqual(writes[0], writes[1]);
      assert.equal(modal.querySelector("form").hidden, true);
      click("关闭", modal);
    },
  );
  await t.test("new knowledge sends null version preconditions", async () => {
    click("▤知识管理");
    await wait(() => document.querySelector("h1")?.textContent === "知识管理");
    click("＋ 新建 / 上传");
    const modal = document.querySelector("dialog");
    control("文档标题", modal).value = "发布手册";
    control("正文", modal).value = "Atlas 中文操作说明";
    modal.querySelector("form").requestSubmit();
    await wait(() => modal.textContent.includes("completed"));
    assert.equal(writes.at(-1).title, "发布手册");
    click("关闭", modal);
    click("＋ 新建 / 上传");
    const upload = document.querySelector("dialog");
    control("文档标题", upload).value = "上传手册";
    const file = control("上传文件（选填，替代正文）", upload);
    Object.defineProperty(file, "files", {
      configurable: true,
      value: [new File(["# 中文 🦀 手册"], "manual.md")],
    });
    file.dispatchEvent(new Event("change"));
    upload.querySelector("form").requestSubmit();
    await wait(() => upload.textContent.includes("completed"));
    assert.equal(writes.at(-1).file_id, ids.source);
    assert.equal(writes.at(-1).content, undefined);
    click("关闭", upload);
  });
  await t.test(
    "a superseded search cannot overwrite the newer query",
    async () => {
      click("◇记忆管理");
      await wait(
        () => document.querySelector("h1")?.textContent === "记忆管理",
      );
      const query = control("查询");
      const search = query.closest("form");
      query.value = "older";
      search.requestSubmit();
      await wait(() => searches.has("older"));
      query.value = "newer";
      search.requestSubmit();
      await wait(() => searches.has("newer"));
      const response = (title) =>
        new Response(
          JSON.stringify({
            hits: [{ ...asset, title }],
            effective_mode: "keyword",
          }),
        );
      searches.get("newer").resolve(response("newer result"));
      await wait(() => document.body.textContent.includes("newer result"));
      searches.get("older").resolve(response("older result"));
      await new Promise((resolve) => setTimeout(resolve, 10));
      assert.ok(document.body.textContent.includes("newer result"));
      assert.ok(!document.body.textContent.includes("older result"));
    },
  );
  await t.test(
    "context renders byte accounting and untrusted content as text",
    async () => {
      click("⊞上下文工作台");
      await wait(
        () => document.querySelector("h1")?.textContent === "上下文工作台",
      );
      control("任务与查询").value = "Atlas";
      document.querySelector("form").requestSubmit();
      click("取消预览");
      await wait(() => document.body.textContent.includes("预览已取消"));
      assert.ok(!document.body.textContent.includes("正在准备上下文"));
      document.querySelector("form").requestSubmit();
      await wait(() =>
        document.body.textContent.includes("32 / 4,000 UTF-8 字节"),
      );
      assert.equal(document.querySelector("img"), null);
      assert.ok(document.body.textContent.includes("不是历史 Run 快照"));
    },
  );
  await t.test("PDF source distinguishes page locator from unavailable page text", async () => {
    const { sourcePreview } = await import("../dist/client/source.js");
    const target = document.createElement("div");
    await sourcePreview(target, ids.source, { page: 2, byte_start: 0, byte_end: 6 }, {
      signal: new AbortController().signal, role: "admin", navigate() {},
    });
    assert.ok(target.textContent.includes("PDF 第 2 页"));
    assert.ok(target.textContent.includes("未提供可核验的页内文本"));
    assert.ok(target.textContent.includes("下载原始文件"));
    assert.ok(!target.textContent.includes("source text is not verified"));
    assert.equal(target.querySelector("mark"), null);
  });
  await t.test("Run preparation links to the exact asset version", async () => {
    click("⑂调用链路");
    await wait(() => document.body.textContent.includes("fixture task"));
    click("查看链路 →");
    await wait(
      () =>
        document.querySelector("h1")?.textContent === "Run 详情" &&
        document.body.textContent.includes("context_prepared"),
    );
    await wait(() => document.body.textContent.includes("外部结论：失败"));
    assert.ok(document.body.textContent.includes("已撤销的外部核验"));
    assert.ok(document.body.textContent.includes("实际使用的 Skill 版本：未知"));
    assert.ok(document.body.textContent.includes("平台未核验其结论"));
    assert.ok(document.body.textContent.includes("<img src=x> external tester"));
    assert.equal(document.querySelectorAll("img").length, 0);
    assert.ok(document.body.textContent.includes("未确认收到或使用"));
    const events = [...document.querySelectorAll(".timeline button.event")];
    assert.deepEqual(events.map((event) => event.querySelector(".badge").textContent), [
      "服务端准备成功", "服务端准备失败", "客户端上报", "来源未知",
    ]);
    events[1].click();
    assert.ok(document.body.textContent.includes("服务端观察到准备失败；未准备上下文"));
    assert.ok(document.body.textContent.includes("not_prepared"));
    events[2].click();
    assert.ok(document.body.textContent.includes("客户端上报；未经独立验证"));
    events[3].click();
    assert.ok(document.body.textContent.includes("证据来源未知；不推断为服务端观察或客户端上报"));
    verificationDenied = true;
    click("刷新核验记录");
    await wait(() => document.body.textContent.includes("source unavailable"));
    assert.ok(!document.body.textContent.includes("synthetic missing output"));
    assert.ok(!document.body.textContent.includes("<img src=x> external tester"));
    verificationDenied = false;
    click("刷新核验记录");
    await wait(() => document.body.textContent.includes("synthetic missing output"));
    events[0].click();
    click("11111111 · v1");
    await wait(
      () =>
        location.hash.includes("?version=1") &&
        document.querySelector("h1")?.textContent === asset.title,
    );
  });
  await t.test("asset association uses the displayed exact version and returns to Run", async () => {
    click("读取已记录引用");
    await wait(() => document.body.textContent.includes(`打开 Run ${ids.run}`));
    assert.ok(document.body.textContent.includes("仅当前逻辑 key 的已记录引用"));
    click(`打开 Run ${ids.run}`);
    await wait(() => location.hash === `#runs/${ids.run}` && document.querySelector("h1")?.textContent === "Run 详情");
  });
  await t.test(
    "revoked credentials clear private UI and return to login",
    async () => {
      unauthorized = true;
      click("◇记忆管理");
      await wait(() => document.querySelector("input[type=password]"));
      assert.equal(document.querySelector("nav"), null);
      assert.equal(document.querySelector("dialog"), null);
    },
  );
});
