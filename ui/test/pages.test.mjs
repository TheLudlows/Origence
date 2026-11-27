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
    "sessionStorage",
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
    coverageRun: "55555555-5555-4555-8555-555555555555",
    managedRun: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
    reportedAsset: "66666666-6666-4666-8666-666666666666",
    reportedChunk: "77777777-7777-4777-8777-777777777777",
    reportedRef: "88888888-8888-4888-8888-888888888888",
    otherRun: "99999999-9999-4999-8999-999999999999",
    otherKey: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  };
  const reportedReference = {
    kind: "chunk", id: ids.reportedChunk, chunk_id: ids.reportedChunk,
    asset_id: ids.reportedAsset, asset_version: 1,
  };
  const reportedReads = [];
  let reportedAssetDenied = false;
  let verificationDenied = false;
  let coveragePageDenied = false;
  let holdTrace = false, releaseTrace;
  const coverageCursors = [];
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
  const catalogQueries = [];
  let overviewUnavailable = false;
  let overviewEmpty = false;
  let agentState = "active";
  let signedIn = false;
  let jobRetries = 0;
  let failMemory = true,
    unauthorized = false,
    conflictKnowledge = false,
    jobState = "completed";
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
    if (path.pathname === "/api/console/session") {
      if (options.method === "POST") {
        assert.deepEqual(body, { username: "fixture-user", password: "disposable-fixture-password" });
        signedIn = true;
      }
      if (options.method === "DELETE") signedIn = false;
      return response({ mode: "account", session: signedIn && !unauthorized ? {
        account: "fixture-user",
        binding: "fixture-session-binding",
        workspaces: [{ id: ids.source, workspace_id: ids.asset, role: "admin", name: ids.asset }],
      } : null });
    }
    if (unauthorized)
      return response({ error: { message: "authentication required" } }, 401);
    assert.equal(options.headers.get("Authorization"), null);
    assert.equal(options.headers.get("X-Origence-Workspace"), ids.asset);
    assert.equal(options.headers.get("X-Origence-Session"), "fixture-session-binding");
    if (path.pathname === "/api/health/ready")
      return response({ status: "ready" });
    if (path.pathname === "/api/admin/agents") {
      if (agentState === "unavailable") return response({ error: { message: "连接列表不可读取" } }, 503);
      return response({ workspace_id: ids.asset, window_seconds: 300, capacity_reached: false,
        items: agentState === "active" ? [{ role: "writer", observation: {
          credential_id: ids.otherKey, client_kinds: ["codex", "claude_code"],
          last_seen_at_ms: 1000, active_requests: 2,
        } }] : [] });
    }
    if (path.pathname === "/api/v1/runs/overview") {
      if (overviewUnavailable)
        return response({ error: { message: "overview unavailable" } }, 503);
      const window = path.searchParams.get("window");
      assert.ok(window === "24h" || window === "7d");
      const size = window === "24h" ? 24 : 7;
      const width = window === "24h" ? 3_600_000 : 86_400_000;
      const created = overviewEmpty ? 0 : window === "24h" ? 31 : 34;
      return response({
        scope: "workspace",
        start_ms: 0,
        end_ms: size * width,
        created_runs: created,
        metering: window === "24h" ? {
          duration_p95_ms: 1500, duration_runs: 2,
          agent_tokens: "0", token_runs: 1,
          costs: [
            { basis: "estimated", currency: "USD", micros: "12345", runs: 1 },
            { basis: "billed", currency: "CNY", micros: "9007199254740993", runs: 2 },
          ],
        } : { duration_p95_ms: null, duration_runs: 0, agent_tokens: null, token_runs: 0, costs: [] },
        series: Array.from({ length: size }, (_, index) => ({
          start_ms: index * width,
          end_ms: (index + 1) * width,
          created_runs: index === 0 ? created : 0,
        })),
      });
    }
    if (path.pathname === "/api/v1/assets") {
      catalogQueries.push(path.searchParams);
      if (path.searchParams.get("memory_filter")?.includes("staging"))
        return response({items: path.searchParams.has("after") ? [asset] : [], next_after: path.searchParams.has("after") ? null : ids.asset});
      return response({
        items: [asset].filter((a) => a.kind === path.searchParams.get("kind")),
        next_after: null,
      });
    }
    if (path.pathname === "/api/v1/runs")
      return response({
        scope: "workspace",
        items: [
          { run_id: ids.run, owner_key_id: ids.source, is_owner: true, summary: "fixture task", created_at_ms: 1000, status: "unknown" },
          { run_id: ids.otherRun, owner_key_id: ids.otherKey, is_owner: false, summary: null, created_at_ms: 1100, status: "reported_failure" },
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
    if (path.pathname === `/api/v1/runs/${ids.coverageRun}/verifications`)
      return response({ items: [], next_after: null });
    if (path.pathname === `/api/v1/runs/${ids.managedRun}/verifications`)
      return response({ items: [], next_after: null });
    if (path.pathname === `/api/v1/runs/${ids.managedRun}`) {
      return response({
        run: { run_id: ids.managedRun, summary: "managed fixture" },
        events: [
          { kind: "codex_command_reported", status: "failed", exit_code: 0 },
          { kind: "claude_interrupt_reported", observation: { stage: "receipt", status: "prompt_still_queued" } },
          { kind: "claude_interrupt_reported", observation: { stage: "result", status: "unknown" } },
          { kind: "claude_interrupt_reported", observation: { stage: "result", status: "interrupted" } },
        ].map((event, index) => ({
          sequence: index + 1,
          evidence_origin: "reported",
          recorded_at_ms: 1000,
          report: { reporter: "client", event },
          references: [],
        })),
        next_after: null,
      });
    }
    if (path.pathname === `/api/v1/runs/${ids.coverageRun}`) {
      const after = path.searchParams.get("after");
      coverageCursors.push(after);
      if (after === "1" && coveragePageDenied)
        return response({ error: { message: "synthetic trace denied" } }, 403);
      const claims = after === "0"
        ? [[1, "codex_hook", "not_collected", "client"]]
        : [[2, "codex_hook", "reported", "test"], [3, "<img src=x>", "partial", "client"], [4, "other", "unknown", "user"]];
      return response({
        run: { run_id: ids.coverageRun, summary: "synthetic coverage Run" },
        events: claims.map(([sequence, component, status, reporter]) => ({
          sequence,
          recorded_at_ms: 1000 + sequence,
          evidence_origin: "reported",
          report: { reporter, event: { kind: "coverage", component, status, summary: "synthetic declaration" } },
          references: [],
        })),
        next_after: after === "0" ? 1 : null,
      });
    }
    if (path.pathname === `/api/v1/runs/${ids.run}`) {
      const embeddingUsage = (tokens) => ({
        context_query_embedding: {
          model_config_sha256: "a".repeat(64),
          provider_reported_prompt_tokens: tokens,
        },
      });
      const trace = response({
        run: { run_id: ids.run, summary: "fixture task" },
        events: [
          {
            sequence: 1,
            recorded_at_ms: 1000,
            evidence_origin: "server_prepared",
            report: {
              kind: "context_prepared",
              reporter: "server",
              retrieval: embeddingUsage(0),
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
            ["server_observed", "server", "context_preparation_failed", "not_prepared", embeddingUsage(null)],
            ["reported", "client", "result_reported", undefined, embeddingUsage(42)],
            ["future_origin", "client", "future_event", undefined],
            ["server_prepared", "server", "context_prepared", undefined, {requests: [], omitted_requests: 0}],
            ["reported", "client", "context_prepared", undefined, embeddingUsage(42)],
          ].map(([evidence_origin, reporter, kind, delivery_status, retrieval], index) => ({
            sequence: index + 2,
            recorded_at_ms: 1001 + index,
            evidence_origin,
            report: { reporter, kind, delivery_status, retrieval },
            references: [],
          })),
          {
            sequence: 7,
            recorded_at_ms: 1006,
            evidence_origin: "reported",
            report: {
              reporter: "client",
              event: { kind: "context_reported", references: [reportedReference] },
            },
            references: [{
              evidence_ref_id: ids.reportedRef,
              reference: reportedReference,
              source_event_id: ids.source, source_version: 1, version_source_event_id: ids.source,
              locator: { parser: "source-ranges-v1", byte_start: 0, byte_end: 6 },
            }],
          },
          {
            sequence: 8,
            recorded_at_ms: 1007,
            evidence_origin: "reported",
            report: {
              reporter: "client",
              event: { kind: "context_reported", references: [reportedReference] },
            },
            references: [],
          },
        ],
        next_after: null,
      });
      if (!holdTrace) return trace;
      const pending = Promise.withResolvers();
      releaseTrace = () => pending.resolve(trace);
      return pending.promise;
    }
    if (path.pathname === `/api/v1/assets/${ids.asset}`) return response(asset);
    if (path.pathname === `/api/v1/assets/${ids.reportedAsset}`) {
      const version = path.searchParams.get("version");
      reportedReads.push(version);
      if (reportedAssetDenied && version === "1")
        return response({ error: { message: "fixture historical source withdrawn" } }, 404);
      return response({
        ...asset, asset_id: ids.reportedAsset, kind: "knowledge", identity: null,
        version: version === "1" ? 1 : 2,
        title: version === "1" ? "历史手册" : "当前手册",
        content: version === "1" ? "仅 v1 原文" : "仅 v2 原文",
      });
    }
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
      if (conflictKnowledge) return response({error:{message:"asset/version mismatch"}},409);
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
    if (path.pathname === `/api/v1/jobs/${ids.job}/retry` && options.method === "POST") {
      jobRetries++;
      jobState = "pending";
      return response({ id: ids.job, state: jobState });
    }
    if (path.pathname === `/api/v1/jobs/${ids.job}`)
      return response({ state: jobState, outcome: jobState === "completed" ? "published" : null });
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
  const wait = async (condition, attempts = 100) => {
    for (let n = 0; n < attempts; n++) {
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
  const coveragePanel = () => [...document.querySelectorAll("section")].find(
    (section) => section.querySelector("h2")?.textContent === "采集声明",
  );
  await import("../dist/client/app.js");
  await t.test("login and real-scope overview", async () => {
    await wait(() => document.querySelector("input[type=password]"));
    control("账号").value = "fixture-user";
    control("密码").value = "disposable-fixture-password";
    document.querySelector("form").requestSubmit();
    await wait(
      () =>
        document.querySelector("nav") &&
        document.body.textContent.includes("fixture task"),
    );
    assert.equal(document.querySelector("h1").textContent, "运行总览");
    const otherRun = [...document.querySelectorAll("tbody tr")].find((row) => row.textContent.includes(ids.otherKey));
    assert.ok(otherRun.textContent.includes("私有任务"));
    assert.ok(otherRun.textContent.includes("上报失败"));
    assert.equal(otherRun.querySelector("button"), null);
    const trend = [...document.querySelectorAll("section")].find(
      (section) => section.querySelector("h2")?.textContent === "运行创建趋势",
    );
    await wait(() => trend.textContent.includes("新建运行 31 条"));
    const rows = (scope) => [...scope.querySelectorAll("tbody tr")].map(
      (row) => [...row.querySelectorAll("td")].map((cell) => cell.textContent),
    );
    const details = () => trend.querySelector("details.run-creation-buckets");
    const bars = () => [...trend.querySelectorAll(".run-chart-bar")];
    const metering = () => [...trend.querySelectorAll("section")].find(
      (section) => section.querySelector("h2")?.textContent === "运行计量",
    );
    assert.equal(bars().length, 24);
    assert.equal(bars().filter((bar) => bar.tabIndex === 0).length, 1);
    assert.match(bars()[0].getAttribute("aria-label"), /新建 31 个 Run$/);
    assert.match(bars()[1].getAttribute("aria-label"), /新建 0 个 Run$/);
    assert.equal(details().open, false);
    assert.equal(rows(details()).length, 24);
    assert.equal(rows(details())[0][2], "31");
    assert.deepEqual(rows(metering()), [
      ["端到端耗时 P95", "1,500 ms", "2"],
      ["Agent Token 总量", "0", "1"],
      ["费用估算", "USD 0.012345", "1"],
      ["账单金额", "CNY 9,007,199,254.740993", "2"],
    ]);
    assert.ok(document.body.textContent.includes("fixture task"));
    const range = control("统计范围", trend);
    range.value = "7d";
    range.dispatchEvent(new Event("change"));
    await wait(() => trend.textContent.includes("新建运行 34 条"));
    assert.equal(bars().length, 7);
    assert.equal(rows(details()).length, 7);
    assert.deepEqual(rows(metering()), [
      ["端到端耗时 P95", "未知", "0"],
      ["Agent Token 总量", "未知", "0"],
      ["费用", "未知", "0"],
    ]);
    overviewUnavailable = true;
    range.value = "24h";
    range.dispatchEvent(new Event("change"));
    await wait(() => trend.textContent.includes("创建量暂不可用"));
    assert.equal(trend.querySelectorAll("tbody tr").length, 0);
    assert.equal(bars().length, 0);
    assert.equal(metering(), undefined);
    assert.ok(document.body.textContent.includes("fixture task"));
    overviewUnavailable = false;
    click("重试读取创建量", trend);
    await wait(() => trend.textContent.includes("新建运行 31 条"));
    overviewEmpty = true;
    range.value = "7d";
    range.dispatchEvent(new Event("change"));
    await wait(() => trend.textContent.includes("新建运行 0 条"));
    assert.equal(bars().length, 7);
    assert.equal(trend.querySelector(".run-chart-baseline").style.bottom, "0%");
    assert.equal(trend.querySelectorAll(".run-chart-mark.is-zero").length, 7);
    overviewEmpty = false;
    assert.equal(window.localStorage.length, 0);
    assert.equal(window.sessionStorage.length, 1);
    assert.equal(window.sessionStorage.getItem("origence.selectedWorkspace"), ids.asset);
  });
  await t.test("administrator Agent list distinguishes active requests, failures, and empty results", async () => {
    click("Agent 连接");
    await wait(() => document.body.textContent.includes("Codex、Claude Code"));
    assert.equal(document.querySelector("h1").textContent, "Agent 连接");
    assert.ok(document.body.textContent.includes("正在处理（2）"));
    assert.ok(document.body.textContent.includes(ids.otherKey));
    agentState = "unavailable";
    click("刷新");
    await wait(() => document.body.textContent.includes("连接列表不可读取"));
    assert.ok(!document.body.textContent.includes(ids.otherKey));
    agentState = "empty";
    click("刷新");
    await wait(() => document.body.textContent.includes("暂无 Agent 活动"));
  });
  await t.test(
    "memory form retries the same operation without silent version overwrite",
    async () => {
      click("记忆管理");
      await wait(
        () => document.querySelector("h1")?.textContent === "记忆管理",
      );
      const main = document.querySelector("main");
      assert.equal(main.tabIndex, -1);
      assert.equal(document.activeElement, main);
      click("＋ 新建记忆");
      const modal = document.querySelector("dialog");
      const label = modal.querySelector("h2");
      assert.ok(label.id);
      assert.equal(
        document.getElementById(modal.getAttribute("aria-labelledby")),
        label,
      );
      assert.equal(label.textContent, "新建记忆");
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
    click("知识管理");
    await wait(() => document.querySelector("h1")?.textContent === "知识管理");
    assert.ok(document.body.textContent.includes("已发布知识"));
    assert.ok(control("内容 ID"));
    click("＋ 新建 / 上传");
    const modal = document.querySelector("dialog");
    control("文档标题", modal).value = "发布手册";
    control("正文", modal).value = "Atlas 中文操作说明";
    modal.querySelector("form").requestSubmit();
    await wait(() => modal.textContent.includes("completed"));
    assert.equal(writes.at(-1).title, "发布手册");
    assert.ok(modal.textContent.includes("查看内容"));
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
  await t.test("memory filters are server predicates and preserve empty-page continuation", async () => {
    click("记忆管理");
    await wait(() => document.querySelector("h1")?.textContent === "记忆管理");
    assert.ok(document.body.textContent.includes("已发布记忆"));
    control("主体类型").value = "service";
    control("主体 ID").value = "Atlas";
    control("属性").value = "release.rule";
    control("完整适用条件").value = '{"env":"staging"}';
    click("应用目录筛选");
    await wait(() => document.body.textContent.includes("本批暂无匹配的记忆"));
    const first = catalogQueries.at(-1);
    assert.equal(first.has("after"), false);
    assert.deepEqual(JSON.parse(first.get("memory_filter")), {subject:{kind:"service",stable_id:"Atlas"},predicate:"release.rule",context:{env:"staging"}});
    click("加载下一页");
    await wait(() => document.querySelector("table")?.textContent.includes("Atlas"));
    assert.equal(catalogQueries.at(-1).get("after"), ids.asset);
    assert.equal(catalogQueries.at(-1).get("memory_filter"), first.get("memory_filter"));
    control("完整适用条件").value = '{"env":"production"}';
    click("应用目录筛选");
    await wait(() => catalogQueries.at(-1).get("memory_filter")?.includes("production"));
    assert.equal(catalogQueries.at(-1).has("after"), false);
  });
  await t.test("failed jobs and version conflicts reread authority without replacing drafts", async () => {
    const { editor } = await import("../dist/client/asset-editor.js");
    const { trackJob } = await import("../dist/client/jobs.js");
    const lifetime = new AbortController();
    const ctx = { signal: lifetime.signal, role: "writer", navigate() {} };
    const target = document.createElement("div");
    document.body.append(target);
    for (const state of ["failed", "superseded"]) {
      jobState = state;
      await trackJob({job_id: ids.job, asset_id: ids.asset}, target, ctx);
      click("读取当前版本", target);
      await wait(() => target.textContent.includes("当前已发布 v1"));
      assert.equal(target.textContent.includes("重试任务"), state === "failed");
      assert.equal(target.textContent.includes("取消任务"), false);
    }
    jobState = "completed";
    conflictKnowledge = true;
    editor("knowledge", ctx, {...asset, kind: "knowledge"});
    const modal = document.querySelector("dialog");
    control("正文", modal).value = "retained draft";
    modal.querySelector("form").requestSubmit();
    await wait(() => modal.textContent.includes("版本冲突"));
    asset.version = 2;
    click("读取当前版本", modal);
    await wait(() => modal.textContent.includes("当前已发布 v2"));
    assert.equal(control("正文", modal).value, "retained draft");
    assert(modal.textContent.includes("基于 v1 提交"));
    assert.equal(modal.querySelector("form").hidden, false);
    click("关闭", modal);
    asset.version = 1;
    conflictKnowledge = false;
    target.remove();
    lifetime.abort();
  });
  await t.test("retry resumes job polling without another submission", async () => {
    const { trackJob } = await import("../dist/client/jobs.js");
    const lifetime = new AbortController();
    const target = document.createElement("div");
    const ctx = { signal: lifetime.signal, role: "writer", navigate() {} };
    document.body.append(target);
    try {
      jobState = "failed";
      await trackJob({ job_id: ids.job, asset_id: ids.asset }, target, ctx);
      click("重试任务", target);
      click("重试任务", target);
      await wait(() => target.textContent.includes("pending"));
      jobState = "completed";
      await wait(() => target.textContent.includes("completed"), 600);
      assert.equal(jobRetries, 1);
      assert.equal(target.textContent.includes("取消任务"), false);
    } finally {
      lifetime.abort();
      target.remove();
      jobState = "completed";
    }
  });
  await t.test(
    "a superseded search cannot overwrite the newer query",
    async () => {
      click("记忆管理");
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
      click("上下文工作台");
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
    const { sourcePreview, sourcePreviewTarget } = await import("../dist/client/source.js");
    const target = sourcePreviewTarget();
    document.body.append(target.root);
    try {
      await sourcePreview(target, ids.source, { page: 2, byte_start: 0, byte_end: 6 }, {
        signal: new AbortController().signal, role: "admin", navigate() {},
      });
      assert.ok(target.root.textContent.includes("PDF 第 2 页"));
      assert.ok(target.root.textContent.includes("未提供可核验的页内文本"));
      assert.ok(target.root.textContent.includes("下载原始文件"));
      assert.ok(!target.root.textContent.includes("source text is not verified"));
      assert.equal(target.root.querySelector("mark"), null);
    } finally {
      target.root.remove();
    }
  });
  await t.test("Run preparation links to the exact asset version", async () => {
    click("调用链路");
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
      "服务端准备成功", "服务端准备失败", "客户端上报", "来源未知", "服务端准备成功", "客户端上报",
      "客户端上报", "客户端上报",
    ]);
    assert.ok(document.body.textContent.includes("0 Token（模型服务上报）"));
    assert.ok(document.body.textContent.includes("查询向量配置 SHA-256：" + "a".repeat(64)));
    assert.ok(document.body.textContent.includes("不是 Agent 或整轮用量，不计算成本"));
    events[1].click();
    assert.ok(document.body.textContent.includes("未知（没有可用的模型服务上报）"));
    assert.ok(!document.body.textContent.includes("0 Token（模型服务上报）"));
    assert.ok(document.body.textContent.includes("服务端观察到准备失败；未准备上下文"));
    assert.ok(document.body.textContent.includes("not_prepared"));
    events[2].click();
    assert.ok(document.body.textContent.includes("客户端上报；未经独立验证"));
    assert.ok(!document.body.textContent.includes("上下文查询向量化 · 输入 Token"));
    events[3].click();
    assert.ok(document.body.textContent.includes("证据来源未知；不推断为服务端观察或客户端上报"));
    events[4].click();
    assert.ok(document.body.textContent.includes("未知（没有可用的模型服务上报）"));
    assert.ok(!document.body.textContent.includes("查询向量配置 SHA-256："));
    events[5].click();
    assert.ok(!document.body.textContent.includes("上下文查询向量化 · 输入 Token"));
    assert.ok(!document.body.textContent.includes("42 Token（模型服务上报）"));
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
    assert.ok(document.body.textContent.includes("删除记忆"));
  });
  await t.test("asset association uses the displayed exact version and returns to Run", async () => {
    click("读取已记录引用");
    await wait(() => document.body.textContent.includes(`打开 Run ${ids.run}`));
    assert.ok(document.body.textContent.includes("仅当前逻辑 key 的已记录引用"));
    click(`打开 Run ${ids.run}`);
    await wait(() => location.hash === `#runs/${ids.run}` && document.querySelector("h1")?.textContent === "Run 详情");
  });
  await t.test("client-reported non-PDF references open only their authorized archived version", async () => {
    const select = async (sequence) => {
      const event = () => document.querySelector(`.timeline button[data-sequence="${sequence}"]`);
      await wait(event);
      event().click();
    };
    const action = () => [...document.querySelectorAll("button")].filter((b) => b.textContent === "66666666 · v1");
    await select(8);
    assert.equal(action().length, 0, "unarchived report payload is not an action");
    await select(7);
    assert.equal(action().length, 1);
    assert.ok(document.body.textContent.includes("客户端上报；未经独立验证"));
    assert.ok(!document.body.textContent.includes("服务端准备成功；未确认收到或使用"));
    assert.ok(!document.body.textContent.includes("核验原始引用"));
    action()[0].click();
    await wait(() => document.querySelector("h1")?.textContent === "历史手册");
    assert.equal(location.hash, `#asset/${ids.reportedAsset}?version=1`);
    assert.ok(document.body.textContent.includes("删除知识"));
    assert.ok(document.body.textContent.includes("仅 v1 原文"));
    assert.ok(!document.body.textContent.includes("仅 v2 原文"));

    try {
      reportedAssetDenied = true;
      location.hash = `#runs/${ids.run}`;
      await select(7);
      action()[0].click();
      await wait(() => document.body.textContent.includes("fixture historical source withdrawn"));
      assert.ok(!document.body.textContent.includes("仅 v1 原文"));
      assert.ok(!document.body.textContent.includes("仅 v2 原文"));
      assert.deepEqual(reportedReads, ["1", "1"], "never falls back to latest");
    } finally {
      reportedAssetDenied = false;
      location.hash = `#runs/${ids.run}`;
    }
  });
  await t.test("managed Run events show command fields and separate cancellation receipts from results", async () => {
    location.hash = `#runs/${ids.managedRun}`;
    const event = sequence => document.querySelector(`button.event[data-sequence="${sequence}"]`);
    await wait(() => event(1)?.textContent.includes("Codex · 命令结果"));
    event(1).click();
    assert.match(document.querySelector("main").textContent, /退出码　0/);
    assert.match(document.querySelector("main").textContent, /命令耗时　未知/);
    event(2).click();
    assert.ok(document.querySelector("main").textContent.includes("目标提示仍在队列"));
    assert.ok(!document.querySelector("main").textContent.includes("确认中断"));
    event(3).click();
    assert.match(document.querySelector("main").textContent, /取消结果[\s\S]*未知/);
    event(4).click();
    assert.ok(document.querySelector("main").textContent.includes("确认中断"));
  });
  await t.test("Run coverage keeps loaded declarations ordered and clears denied or replaced evidence", async () => {
    const unknown = "已加载事件中未见采集声明，覆盖未知";
    await wait(() => coveragePanel()?.textContent.includes(unknown));
    location.hash = `#runs/${ids.coverageRun}`;
    await wait(() => coveragePanel()?.textContent.includes("codex_hook"));
    const panel = coveragePanel();
    const rows = () => [...panel.querySelectorAll("tbody tr")].map(
      (row) => [...row.cells].map((cell) => cell.textContent),
    );
    const first = ["1", "codex_hook", "未采集", "客户端上报；未经独立验证"];
    assert.deepEqual(rows(), [first]);
    assert.match(panel.textContent, /整轮覆盖仍未知.*还有后续事件/u);
    const hasVerification = () => document.body.textContent.includes("暂无外部核验记录");
    await wait(hasVerification);

    coveragePageDenied = true;
    click("加载后续事件");
    await wait(() => document.body.textContent.includes("synthetic trace denied"));
    assert.equal(panel.hidden, true);
    assert.deepEqual(rows(), []);
    assert.ok(!panel.textContent.includes(unknown));
    assert.equal(hasVerification(), false);

    coveragePageDenied = false;
    click("加载后续事件");
    await wait(() => !panel.hidden && rows().length === 1);
    assert.ok(!document.body.textContent.includes("synthetic trace denied"));
    await wait(hasVerification);
    const selected = document.querySelector('.timeline [aria-pressed="true"]');
    assert.equal(selected.dataset.sequence, "1");
    click("加载后续事件");
    await wait(() => rows().length === 4);
    assert.deepEqual(coverageCursors, ["0", "1", "0", "1"]);
    assert.deepEqual(rows(), [first,
      ["2", "codex_hook", "已上报", "显式上报 · test；未经独立验证"],
      ["3", "<img src=x>", "部分采集", "客户端上报；未经独立验证"],
      ["4", "other", "未知", "显式上报 · user；未经独立验证"],
    ]);
    assert.equal(panel.querySelector("img"), null);
    assert.ok(!panel.textContent.includes("还有后续事件"));
    assert.equal(document.querySelectorAll(".timeline button.event").length, 4);
    assert.equal(document.querySelector('.timeline [aria-pressed="true"]'), selected);
    assert.equal(document.querySelectorAll('.timeline [aria-pressed="false"]').length, 3);

    holdTrace = true;
    location.hash = `#runs/${ids.run}`;
    await wait(() => releaseTrace && coveragePanel()?.textContent.includes("正在读取采集声明"));
    assert.equal(panel.isConnected, false);
    assert.ok(!document.body.textContent.includes("codex_hook"));
    assert.ok(!coveragePanel().textContent.includes(unknown));
    holdTrace = false;
    releaseTrace();
    await wait(() => coveragePanel()?.textContent.includes(unknown));
  });
  await t.test(
    "an invalid account session clears private UI and returns to login",
    async () => {
      location.hash = `#runs/${ids.coverageRun}`;
      await wait(() => coveragePanel()?.textContent.includes("codex_hook"));
      unauthorized = true;
      click("加载后续事件");
      await wait(() => document.querySelector("input[type=password]"));
      assert.equal(document.querySelector("nav"), null);
      assert.equal(document.querySelector("dialog"), null);
      assert.equal(coveragePanel(), undefined);
      assert.ok(!document.body.textContent.includes("codex_hook"));
    },
  );
});
