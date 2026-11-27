import { api } from "./api.js";
import {
  badge,
  button,
  code,
  date,
  el,
  empty,
  field,
  form,
  heading,
  input,
  message,
  panel,
  short,
  submit,
  table,
  type ViewContext,
} from "./components.js";
import type { Page, Provenance, Run, RunEvent, Trace } from "./types.js";
import { runPdfPreview, sourcePreview, sourcePreviewTarget } from "./source.js";
import { runCreationTrend } from "./run-creation-trend.js";
import { readRunPrefix } from "./run-prefix.js";
import { runSkillDrafting } from "./run-skill.js";

type RunDirectory = Page<Run> & { scope: "workspace" | "current_logical_key" };

export function citation(
  ref: Provenance,
  ctx: ViewContext,
  { pdf, knowledgeQuote, label }: { pdf?: { path?: string }; knowledgeQuote?: string; label?: string } = {},
) {
  const source = el("div");
  const textSource = !pdf && ref.source_event_id && ctx.role !== "reader"
    ? sourcePreviewTarget()
    : null;
  const pdfPath = pdf?.path;
  const openPdf = pdfPath && ctx.role !== "reader"
    ? button("核验 PDF 页内引用", async () => {
        if (!openPdf) return;
        openPdf.disabled = true;
        try {
          await runPdfPreview(source, pdfPath, ctx);
        } finally {
          openPdf.disabled = false;
        }
      })
    : null;
  return el(
    "div",
    "citation",
    ref.asset_id && (!pdf || (typeof ref.asset_version === "number" &&
      Number.isSafeInteger(ref.asset_version) && ref.asset_version > 0))
      ? button(
          label ?? `${short(ref.asset_id)} · v${ref.asset_version ?? "?"}`,
          () => ctx.navigate("asset", ref.asset_id, ref.asset_version),
          "link",
        )
      : badge(ref.type),
    code(ref),
    pdf
      ? openPdf ?? el("p", "muted", "没有可用于核验的页内来源。")
      : textSource
        ? button("核验原始引用", () =>
            sourcePreview(textSource, ref.source_event_id!, ref.locator ?? {}, ctx, knowledgeQuote),
          )
        : null,
    pdf ? source : textSource?.root,
  );
}

export async function overview(root: HTMLElement, ctx: ViewContext, workspaceId: string) {
  root.append(
    heading(
      "运行总览",
      el(
        "span",
        "",
        "查看新建运行和调用记录。",
        el(
          "span",
          "overview-workspace-id",
          el("span", "", "工作区 ID"),
          el("code", "mono", workspaceId),
        ),
      ),
      button("查看调用链 →", () => ctx.navigate("runs"), "primary"),
    ),
  );
  const readiness = el("div", "overview-health");
  const recent = el("div");
  root.append(readiness, runCreationTrend(ctx.signal), recent);
  const results = await Promise.allSettled([
    api.get<{ status: string }>("/health/ready", ctx.signal),
    api.get<RunDirectory>("/v1/runs?limit=25", ctx.signal),
  ]);
  if (ctx.signal.aborted) return;
  const health = results[0],
    runs = results[1];
  const workspaceWide = runs.status === "fulfilled" && runs.value.scope === "workspace";
  const list = panel(workspaceWide ? "当前工作区的运行" : "当前 Key 的运行");
  recent.append(list);
  const ready = health.status === "fulfilled" && health.value.status === "ready";
  readiness.append(
    panel(
      "服务就绪状态",
      el(
        "div",
        ready ? "stat-health is-ready" : "stat-health",
        ready ? "就绪" : health.status === "fulfilled" ? health.value.status : "不可用",
      ),
      el("p", "muted", "检查 Worker 与本地存储。模型健康需单独核对。"),
    ),
  );
  if (runs.status === "fulfilled") {
    list.append(runTable(runs.value.items, workspaceWide, ctx));
    if (runs.value.next_after)
      list.append(el("p", "muted", "还有更多运行，请进入调用链路分页查看。"));
  } else message(list, runs.reason);
  root.append(
    panel(
      "可观测范围",
      el(
        "p",
        "muted",
        "Origence 展示服务端准备回执和客户端显式上报。没有上报的活动保持未知。Run 详情仅向所属 Key 开放。",
      ),
    ),
  );
}

function runTable(items: Run[], workspaceWide: boolean, ctx: ViewContext) {
  if (!items.length)
    return empty(
      "还没有可读取的运行",
      workspaceWide ? "当前工作区上报的运行会显示在这里。" : "使用当前 Key 上报的运行会显示在这里。",
    );
  const statuses = { reported_success: "上报成功", reported_failure: "上报失败", unknown: "未知" };
  return table(
    ["任务", "Run ID", ...(workspaceWide ? ["所属 Key"] : []), "创建时间", "状态", "操作"],
    items.map((run) => [
      run.summary ?? "私有任务",
      el("span", "mono", short(run.run_id)),
      ...(workspaceWide ? [run.owner_key_id ? el("span", "mono", run.owner_key_id) : "—"] : []),
      date(run.created_at_ms),
      badge(statuses[run.status] ?? statuses.unknown,
        run.status === "reported_success" ? "good" : run.status === "reported_failure" ? "danger" : ""),
      run.is_owner === false
        ? "仅所有者可查看链路"
        : button("查看链路 →", () => ctx.navigate("runs", run.run_id), "link"),
    ]),
  );
}

export async function runsPage(
  root: HTMLElement,
  ctx: ViewContext,
  id?: string,
) {
  if (id) {
    await trace(root, id, ctx);
    return;
  }
  const list = el("div"),
    error = el("div"),
    direct = input("Run UUID");
  let after: string | null = null;
  let scope: RunDirectory["scope"] | undefined;
  const items: Run[] = [];
  const load = async () => {
    more.disabled = true;
    try {
      const path = "/v1/runs?limit=25";
      let page = await api.get<RunDirectory>(
        `${path}${after ? "&after=" + after : ""}`,
        ctx.signal,
      );
      if (ctx.signal.aborted) return;
      if (scope !== undefined && page.scope !== scope) {
        items.length = 0;
        after = null;
        list.replaceChildren();
        page = await api.get<RunDirectory>(path, ctx.signal);
        if (ctx.signal.aborted) return;
      }
      scope = page.scope;
      items.push(...page.items);
      after = page.next_after;
      list.replaceChildren(runTable(items, page.scope === "workspace", ctx));
      more.hidden = !after;
    } catch (e) {
      message(error, e);
    } finally {
      more.disabled = false;
    }
  };
  const more = button("加载下一页", load);
  root.append(
    heading("调用链路", "查看运行状态与有权读取的链路。"),
    panel("运行记录", list, more),
    panel(
      "打开指定 Run",
      form(
        () => ctx.navigate("runs", direct.value.trim()),
        field("Run ID", direct),
        submit("查看链路"),
      ),
    ),
    error,
  );
  await load();
}

function eventOrigin(event: RunEvent): [string, string] {
  switch (event.evidence_origin) {
    case "server_prepared":
      return ["服务端准备成功", "服务端准备成功；未确认收到或使用"];
    case "server_observed":
      return event.report.kind === "context_preparation_failed"
        ? ["服务端准备失败", "服务端观察到准备失败；未准备上下文"]
        : ["服务端观察", "服务端观察；不代表准备、收到或使用"];
    case "reported":
      return [
        event.report.reporter === "client" ? "客户端上报" : "显式上报",
        event.report.reporter === "client"
          ? "客户端上报；未经独立验证"
          : `显式上报 · ${event.report.reporter ?? "未知上报者"}；未经独立验证`,
      ];
    default:
      return ["来源未知", "证据来源未知；不推断为服务端观察或客户端上报"];
  }
}

const codexCommandStatuses = new Map([
  ["completed", "已完成"], ["failed", "失败"], ["declined", "已拒绝"],
]);
const claudeInterruptStages = new Map([
  ["requested", "Claude Code · 已发起取消"],
  ["receipt", "Claude Code · 取消回执"],
  ["result", "Claude Code · 取消结果"],
]);
const claudeInterruptStatuses = new Map([
  ["acknowledged", "已收到 SDK 回执"],
  ["prompt_still_queued", "目标提示仍在队列"],
  ["acknowledged_without_details", "SDK 已回应，回执详情未知"],
  ["unavailable", "SDK 回执不可用"],
  ["interrupted", "确认中断"],
  ["completed", "轮次已完成"],
  ["other_terminal", "轮次因其他原因结束"],
]);

function interruptObservation(report: Record<string, unknown>) {
  return report.observation as { stage?: unknown; status?: unknown } | undefined;
}

function runEventLabel(event: RunEvent) {
  const report = event.report.event;
  if (event.evidence_origin === "reported" && report?.kind === "codex_command_reported")
    return "Codex · 命令结果";
  if (event.evidence_origin === "reported" && report?.kind === "claude_interrupt_reported")
    return claudeInterruptStages.get(String(interruptObservation(report)?.stage)) ?? "Claude Code · 取消观察";
  return event.report.kind ?? String(report?.kind ?? "reported");
}

function managedObservation(event: RunEvent): HTMLElement | null {
  const report = event.report.event;
  if (event.evidence_origin !== "reported" || !report) return null;
  const number = (value: unknown, unit = "") =>
    typeof value === "number" && Number.isSafeInteger(value) ? `${value}${unit}` : "未知";
  if (report.kind === "codex_command_reported") {
    return el("div", "notice",
      el("p", "", "命令状态　", codexCommandStatuses.get(String(report.status)) ?? "未知"),
      el("p", "", "退出码　", number(report.exit_code)),
      el("p", "", "命令耗时　", number(report.duration_ms, " ms")),
    );
  }
  if (report.kind === "claude_interrupt_reported") {
    const observation = interruptObservation(report);
    return el("p", "notice", observation?.stage === "requested" ? "Origence 已通过 SDK 发起取消。"
      : claudeInterruptStatuses.get(String(observation?.status)) ?? "未知");
  }
  return null;
}

function contextQueryEmbedding(event: RunEvent) {
  if (!(event.evidence_origin === "server_prepared" && event.report.kind === "context_prepared") &&
      !(event.evidence_origin === "server_observed" && event.report.kind === "context_preparation_failed")) return null;
  const retrieval = event.report.retrieval as {
    context_query_embedding?: {
      model_config_sha256?: unknown;
      provider_reported_prompt_tokens?: unknown;
    };
  } | null | undefined;
  const fingerprint = retrieval?.context_query_embedding?.model_config_sha256;
  const tokens = retrieval?.context_query_embedding?.provider_reported_prompt_tokens;
  const config = typeof fingerprint === "string" && /^[0-9a-f]{64}$/.test(fingerprint) ? fingerprint : null;
  const known = config !== null && typeof tokens === "number" &&
    Number.isInteger(tokens) && tokens >= 0 && tokens <= 0xffffffff;
  return el("div", "notice",
    el("strong", "", "上下文查询向量化 · 输入 Token"),
    el("p", "", known ? `${tokens.toLocaleString()} Token（模型服务上报）` : "未知（没有可用的模型服务上报）"),
    config ? el("p", "muted", "查询向量配置 SHA-256：", el("span", "mono", config)) : null,
    el("p", "muted", "仅此事件的 Origence 查询向量请求，不是 Agent 或整轮用量，不计算成本。"),
  );
}

const coverageStatus = new Map([
  ["reported", "已上报"],
  ["partial", "部分采集"],
  ["not_collected", "未采集"],
  ["unknown", "未知"],
]);

function showCoverage(target: HTMLElement, events: RunEvent[], hasMore: boolean) {
  const rows = events.flatMap((event) => {
    const declaration = event.report.event ?? event.report;
    if (declaration.kind !== "coverage") return [];
    const status = typeof declaration.status === "string"
      ? coverageStatus.get(declaration.status) : undefined;
    return [[
      event.sequence,
      typeof declaration.component === "string" ? declaration.component : "未知",
      status ?? "未知",
      eventOrigin(event)[1],
    ]];
  });
  target.replaceChildren(
    el("p", "muted", "采集声明来自上报方，未经独立核验。整轮覆盖仍未知。",
      hasMore ? " 还有后续事件，此处仅显示已加载事件中的声明。" : null),
    rows.length
      ? table(["事件序号", "组件", "声明状态", "来源"], rows)
      : el("p", "muted", "已加载事件中未见采集声明，覆盖未知"),
  );
}

interface ArchivedAssetReference {
  evidence_ref_id: string;
  source_event_id: string;
  locator: Record<string, unknown>;
  reference: { chunk_id: string; asset_id: string; asset_version: number };
}
function pdfLocator(locator?: Record<string, unknown>) {
  return locator?.page != null ||
    (typeof locator?.parser === "string" && locator.parser.startsWith("pdf-"));
}
function archivedAsset(value: unknown): value is ArchivedAssetReference {
  const item = value as Partial<ArchivedAssetReference> | null;
  return typeof item?.evidence_ref_id === "string" &&
    typeof item.source_event_id === "string" &&
    typeof item.reference?.chunk_id === "string" &&
    typeof item.reference.asset_id === "string" &&
    Number.isInteger(item.reference.asset_version) &&
    item.reference.asset_version > 0 && item.reference.asset_version <= 2147483647;
}

function fixedPrefix(runId: string, sequence: number, signal: AbortSignal) {
  const notice = el("div", "muted");
  notice.setAttribute("role", "status");
  notice.setAttribute("aria-atomic", "true");
  const result = el("div");
  let pending = false;
  const load = button("获取此处的固定前缀摘要", async () => {
    if (pending || signal.aborted) return;
    pending = true;
    // Remain focusable while the event's own request is in flight.
    load.setAttribute("aria-disabled", "true");
    result.replaceChildren();
    notice.className = "muted";
    notice.textContent = "正在读取固定前缀摘要…";
    try {
      const prefix = await readRunPrefix(runId, sequence, signal);
      if (signal.aborted) return;
      result.replaceChildren(code(prefix));
      notice.textContent = `已读取截至事件 ${sequence} 的固定前缀摘要。`;
    } catch (e) {
      if (signal.aborted) return;
      notice.className = "error";
      notice.textContent = e instanceof Error ? e.message : "固定前缀摘要读取失败，请重试";
    } finally {
      pending = false;
      load.removeAttribute("aria-disabled");
    }
  });
  return el("div", "", load, notice, result);
}

async function trace(root: HTMLElement, id: string, ctx: ViewContext) {
  const events: RunEvent[] = [];
  const drafting = ["writer", "admin"].includes(ctx.role) ? runSkillDrafting(id, ctx) : null;
  const assessments = el("div");
  const declarations = el("div", "", el("p", "muted", "正在读取采集声明…"));
  const coverage = panel("采集声明", declarations);
  let after = 0;
  const timeline = el("div", "timeline"),
    details = el("div"),
    summary = el("p", "muted"),
    error = el("div");
  let selected: AbortController | undefined;
  let selectedSequence: number | undefined;
  let verification: AbortController | undefined;
  let loading = false;
  const selectEvent = (event: RunEvent) => {
    if (selectedSequence === event.sequence) return;
    selectedSequence = event.sequence;
    selected?.abort();
    selected = new AbortController();
    const selectionSignal = AbortSignal.any([ctx.signal, selected.signal]);
    const report = event.report;
    const eventBody = report.event ?? report;
    const embedding = contextQueryEmbedding(event);
    details.replaceChildren(
      el("h2", "", runEventLabel(event)),
      badge(eventOrigin(event)[1]),
      el(
        "p",
        "mono muted",
        `sequence ${event.sequence} · ${date(event.recorded_at_ms)}`,
      ),
      ...(embedding ? [embedding] : []),
      typeof report.private_sources_unavailable === "number"
        ? el("p", "notice", "归档来源已删除或不可用。")
        : managedObservation(event) ?? code(eventBody),
    );
    const refs = Array.isArray(report.citations)
      ? (report.citations as Provenance[])
      : [];
    const pdfRefs = event.references.filter(
      (ref): ref is ArchivedAssetReference => archivedAsset(ref) && pdfLocator(ref.locator),
    );
    const shown = new Set<string>();
    const pdfPath = (ref: ArchivedAssetReference) => {
      shown.add(ref.evidence_ref_id);
      const query = new URLSearchParams({
        sequence: String(event.sequence),
        evidence_ref_id: ref.evidence_ref_id,
      });
      return `/v1/runs/${encodeURIComponent(id)}/pdf-source?${query}`;
    };
    for (const ref of refs) {
      if (!pdfLocator(ref.locator)) {
        details.append(citation(ref, ctx));
        continue;
      }
      const archived = pdfRefs.find((r) =>
        r.reference.chunk_id === ref.chunk_id &&
        r.reference.asset_id === ref.asset_id &&
        r.reference.asset_version === ref.asset_version &&
        r.source_event_id === ref.source_event_id,
      );
      details.append(citation(ref, ctx, { pdf: { path: archived && pdfPath(archived) } }));
    }
    for (const ref of event.references) {
      if (archivedAsset(ref)) {
        const isPdf = pdfLocator(ref.locator);
        if (isPdf && !shown.has(ref.evidence_ref_id)) {
          details.append(citation({
            type: "asset_chunk",
            asset_id: ref.reference.asset_id,
            asset_version: ref.reference.asset_version,
            source_event_id: ref.source_event_id,
            locator: ref.locator,
          }, ctx, { pdf: { path: pdfPath(ref) } }));
        } else if (!isPdf && event.evidence_origin === "reported" &&
          report.event?.kind === "context_reported") {
          // Only an archived reference can link a client report to an asset.
          // Opening it still rechecks access to that exact historical version.
          const { asset_id, asset_version } = ref.reference;
          details.append(button(
            `${short(asset_id)} · v${asset_version}`,
            () => ctx.navigate("asset", asset_id, asset_version),
            "link",
          ));
        }
      }
      details.append(code(ref));
    }
    details.append(fixedPrefix(id, event.sequence, selectionSignal));
    if (drafting) details.append(drafting.action(event.sequence));
    timeline
      .querySelectorAll("button")
      .forEach((b) =>
        b.setAttribute(
          "aria-pressed",
          String(b.dataset.sequence === String(event.sequence)),
        ),
      );
  };
  const eventRow = (event: RunEvent) => {
    const row = button("", () => selectEvent(event), "event");
    row.dataset.sequence = String(event.sequence);
    row.setAttribute("aria-pressed", "false");
    row.append(
      el("span", "event-index", event.sequence),
      el("div", "", el("strong", "", runEventLabel(event)), el("small", "muted", date(event.recorded_at_ms))),
      badge(eventOrigin(event)[0]),
    );
    return row;
  };
  const load = async () => {
    if (loading || ctx.signal.aborted) return;
    loading = true;
    more.setAttribute("aria-disabled", "true");
    try {
      const value = await api.get<Trace>(
        `/v1/runs/${encodeURIComponent(id)}?after=${after}&limit=50`,
        ctx.signal,
      );
      if (ctx.signal.aborted) return;
      summary.textContent = value.run.summary;
      const firstPage = events.length === 0;
      events.push(...value.events);
      after = value.next_after ?? after;
      error.replaceChildren();
      showCoverage(declarations, events, value.next_after !== null);
      coverage.hidden = false;
      const rows = value.events.map(eventRow);
      if (firstPage) timeline.replaceChildren(...rows);
      else timeline.append(...rows);
      if (!events.length)
        timeline.append(empty("暂无已记录事件", "未上报的活动未知。"));
      if (drafting) drafting.root.hidden = !events.length;
      if (selectedSequence === undefined && events[0]) selectEvent(events[0]);
      if (events.length && !verification) {
        verification = new AbortController();
        const signal = AbortSignal.any([ctx.signal, verification.signal]);
        void verificationPanel(assessments, id, { ...ctx, signal });
      }
      const moveFocus = value.next_after === null && document.activeElement === more;
      more.hidden = value.next_after === null;
      if (moveFocus) {
        (rows[0] ?? timeline.querySelector<HTMLButtonElement>('button[aria-pressed="true"]')
          ?? back).focus();
      }
    } catch (e) {
      if (ctx.signal.aborted) return;
      const focused = document.activeElement;
      const restoreFocus = [timeline, details, assessments, drafting?.root].some((section) => section?.contains(focused));
      events.length = 0;
      after = 0;
      selected?.abort();
      selected = undefined;
      selectedSequence = undefined;
      verification?.abort();
      verification = undefined;
      if (drafting) {
        drafting.reset();
        drafting.root.hidden = true;
      }
      timeline.replaceChildren();
      details.replaceChildren();
      assessments.replaceChildren();
      declarations.replaceChildren();
      coverage.hidden = true;
      summary.textContent = "";
      message(error, e);
      if (restoreFocus) more.focus();
    } finally {
      loading = false;
      more.removeAttribute("aria-disabled");
    }
  };
  const more = button("加载后续事件", load);
  const back = button("返回运行列表", () => ctx.navigate("runs"));
  root.append(
    heading(
      "Run 详情",
      id,
      back,
    ),
    summary,
    el(
      "div",
      "notice",
      "这是事件时间线。当前事件不含可靠的操作起止时间，未推断瀑布耗时；上报成功也不等于独立验证通过。",
    ),
    el(
      "div",
      "columns",
      panel("事件顺序", timeline, more),
      panel("节点与上下文证据", details),
    ),
    ...(drafting ? [drafting.root] : []),
    coverage,
    panel("外部核验记录", assessments),
    error,
  );
  await load();
}

// Independent DTO: external claims never create an authenticated event role.
interface VerificationRecord {
  assessment: {
    verification_id: string;
    through_sequence: number;
    prefix_digest: string;
    verifier_label: string;
    assessed_at_ms: number;
    conclusion: "passed" | "failed" | "inconclusive";
    basis_summary: string;
    basis_sequences: number[];
    reported_skill_version: { skill_id: string; version: number; digest: string } | null;
  };
  recorder_principal_id: string;
  recorded_at_ms: number;
  withdrawal: { reason: string; recorded_at_ms: number } | null;
}
async function verificationPanel(root: HTMLElement, id: string, ctx: ViewContext) {
  const body = el("div"), error = el("div");
  let after: string | null = null;
  const records: VerificationRecord[] = [];
  let generation = 0;
  const load = async () => {
    const request = ++generation;
    more.disabled = true;
    error.replaceChildren();
    try {
      const page = await api.get<Page<VerificationRecord>>(
        `/v1/runs/${encodeURIComponent(id)}/verifications?limit=25${after ? "&after=" + after : ""}`, ctx.signal,
      );
      if (ctx.signal.aborted || request !== generation) return;
      records.push(...page.items);
      after = page.next_after;
      body.replaceChildren(...records.map((r) => {
        const a = r.assessment;
        return panel(
          r.withdrawal ? "已撤销的外部核验" : "Owner 转录的外部核验",
          badge(({ passed: "外部结论：通过", failed: "外部结论：失败", inconclusive: "外部结论：无法确定" })[a.conclusion]),
          el("p", "muted", `核验者（未经认证的声明）：${a.verifier_label}`),
          el("p", "muted", `声称核验时间：${date(a.assessed_at_ms)} · 平台记录时间：${date(r.recorded_at_ms)}`),
          el("p", "mono", `记录凭据：${r.recorder_principal_id}`),
          el("p", "mono", `固定前缀：1–${a.through_sequence} · ${a.prefix_digest}`),
          el("p", "", a.basis_summary),
          el("p", "muted", `依据事件：${a.basis_sequences.join(", ")}`),
          a.reported_skill_version ? code(a.reported_skill_version) : el("p", "muted", "实际使用的 Skill 版本：未知"),
          a.reported_skill_version ? el("p", "muted", "上述版本由 owner 声明，不证明加载或使用。") : null,
          r.withdrawal ? el("p", "", `撤销：${r.withdrawal.reason} · ${date(r.withdrawal.recorded_at_ms)}`) : null,
        );
      }));
      if (!records.length) body.append(empty("暂无外部核验记录", "自报成功不代表核验通过；未记录的核验保持未知。"));
      more.hidden = !after;
    } catch (e) {
      if (ctx.signal.aborted || request !== generation) return;
      records.length = 0;
      after = null;
      body.replaceChildren();
      more.hidden = false;
      message(error, e);
    } finally { if (request === generation) more.disabled = false; }
  };
  const more = button("加载核验记录 / 重试", load);
  const refresh = button("刷新核验记录", async () => {
    after = null;
    records.length = 0;
    body.replaceChildren();
    refresh.disabled = true;
    try { await load(); } finally { refresh.disabled = false; }
  });
  root.replaceChildren(el("p", "muted", "仅 owner 转录外部证据；平台未核验其结论，核验者标签不授予 Run 正文访问权。"), body, more, refresh, error);
  await load();
}
