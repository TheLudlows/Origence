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

export function citation(
  ref: Provenance,
  ctx: ViewContext,
  { pdf, knowledgeQuote }: { pdf?: { path?: string }; knowledgeQuote?: string } = {},
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
          `${short(ref.asset_id)} · v${ref.asset_version ?? "?"}`,
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

export async function overview(root: HTMLElement, ctx: ViewContext) {
  root.append(
    heading(
      "运行总览",
      "追溯任务、上下文与每一条运行证据。",
      button("查看调用链 →", () => ctx.navigate("runs"), "primary"),
    ),
  );
  const cards = el("div", "stats");
  const list = panel("当前 key 的运行");
  root.append(cards, runCreationTrend(ctx.signal), list);
  const results = await Promise.allSettled([
    api.get<{ status: string }>("/health/ready", ctx.signal),
    api.get<Page<Run>>("/v1/runs?limit=25", ctx.signal),
  ]);
  if (ctx.signal.aborted) return;
  const health = results[0],
    runs = results[1];
  cards.append(
    panel(
      "服务就绪状态",
      el(
        "div",
        "stat-value",
        health.status === "fulfilled" ? health.value.status : "不可用",
      ),
      el("p", "muted", "Worker 与本地存储；不代表模型健康"),
    ),
  );
  cards.append(
    panel(
      "本页运行数",
      el(
        "div",
        "stat-value",
        runs.status === "fulfilled" ? runs.value.items.length : "—",
      ),
      el("p", "muted", "仅当前身份；不代表全部运行总数"),
    ),
  );
  cards.append(
    panel(
      "业务成功率 / 端到端 P95 / Agent Token / 成本",
      el("div", "stat-value", "未采集"),
      el("p", "muted", "单次上下文准备的查询向量用量不代表整轮计量。"),
    ),
  );
  if (runs.status === "fulfilled") {
    list.append(runTable(runs.value.items, ctx));
    if (runs.value.next_after)
      list.append(el("p", "muted", "还有更多运行，请进入调用链路分页查看。"));
  } else message(list, runs.reason);
  root.append(
    panel(
      "可观测范围",
      el(
        "p",
        "muted",
        "Origence 展示服务端准备回执和客户端显式上报。没有上报的模型、工具与客户端活动保持未知。共享资产不公开他人的私有会话或 Run。",
      ),
    ),
  );
}

function runTable(items: Run[], ctx: ViewContext) {
  if (!items.length)
    return empty(
      "还没有可读取的运行",
      "使用当前逻辑 key 上报的任务会显示在这里。",
    );
  return table(
    ["任务", "Run ID", "创建时间", "操作"],
    items.map((run) => [
      run.summary,
      el("span", "mono", short(run.run_id)),
      date(run.created_at_ms),
      button("查看链路 →", () => ctx.navigate("runs", run.run_id), "link"),
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
  const items: Run[] = [];
  const load = async () => {
    more.disabled = true;
    try {
      const page = await api.get<Page<Run>>(
        `/v1/runs?limit=25${after ? "&after=" + after : ""}`,
        ctx.signal,
      );
      if (ctx.signal.aborted) return;
      items.push(...page.items);
      after = page.next_after;
      list.replaceChildren(runTable(items, ctx));
      more.hidden = !after;
    } catch (e) {
      message(error, e);
    } finally {
      more.disabled = false;
    }
  };
  const more = button("加载下一页", load);
  root.append(
    heading("调用链路", "只展示当前逻辑 key 有权访问的 Run。"),
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

interface ArchivedPdfReference {
  evidence_ref_id: string;
  source_event_id: string;
  locator: Record<string, unknown>;
  reference: { chunk_id: string; asset_id: string; asset_version: number };
}
function pdfLocator(locator?: Record<string, unknown>) {
  return locator?.page != null ||
    (typeof locator?.parser === "string" && locator.parser.startsWith("pdf-"));
}
function archivedPdf(value: unknown): value is ArchivedPdfReference {
  const item = value as Partial<ArchivedPdfReference> | null;
  return typeof item?.evidence_ref_id === "string" &&
    typeof item.source_event_id === "string" &&
    typeof item.reference?.chunk_id === "string" &&
    typeof item.reference.asset_id === "string" &&
    Number.isSafeInteger(item.reference.asset_version) && item.reference.asset_version > 0 && pdfLocator(item.locator);
}

async function trace(root: HTMLElement, id: string, ctx: ViewContext) {
  const events: RunEvent[] = [];
  const assessments = el("div");
  const declarations = el("div", "", el("p", "muted", "正在读取采集声明…"));
  const coverage = panel("采集声明", declarations);
  let after = 0;
  const timeline = el("div", "timeline"),
    details = el("div"),
    summary = el("p", "muted"),
    error = el("div");
  const selectEvent = (event: RunEvent) => {
    const report = event.report,
      kind = report.kind ?? String(report.event?.kind ?? "reported");
    const eventBody = report.event ?? report;
    const embedding = contextQueryEmbedding(event);
    details.replaceChildren(
      el("h2", "", kind),
      badge(eventOrigin(event)[1]),
      el(
        "p",
        "mono muted",
        `sequence ${event.sequence} · ${date(event.recorded_at_ms)}`,
      ),
      ...(embedding ? [embedding] : []),
      code(eventBody),
    );
    const refs = Array.isArray(report.citations)
      ? (report.citations as Provenance[])
      : [];
    const pdfRefs = event.references.filter(archivedPdf);
    const shown = new Set<string>();
    const pdfPath = (ref: ArchivedPdfReference) => {
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
      if (archivedPdf(ref) && !shown.has(ref.evidence_ref_id)) {
        details.append(citation({
          type: "asset_chunk",
          asset_id: ref.reference.asset_id,
          asset_version: ref.reference.asset_version,
          source_event_id: ref.source_event_id,
          locator: ref.locator,
        }, ctx, { pdf: { path: pdfPath(ref) } }));
      }
      details.append(code(ref));
    }
    details.append(
      button("获取此处的固定前缀摘要", async () => {
        try {
          const prefix = await api.get(
            `/v1/runs/${id}/prefix?through_sequence=${event.sequence}`,
            ctx.signal,
          );
          details.append(code(prefix));
        } catch (e) {
          message(error, e);
        }
      }),
    );
    timeline
      .querySelectorAll("button")
      .forEach((b) =>
        b.setAttribute(
          "aria-pressed",
          String(b.dataset.sequence === String(event.sequence)),
        ),
      );
  };
  const load = async () => {
    more.disabled = true;
    try {
      const value = await api.get<Trace>(
        `/v1/runs/${encodeURIComponent(id)}?after=${after}&limit=50`,
        ctx.signal,
      );
      if (ctx.signal.aborted) return;
      summary.textContent = value.run.summary;
      events.push(...value.events);
      after = value.next_after ?? after;
      error.replaceChildren();
      showCoverage(declarations, events, value.next_after !== null);
      coverage.hidden = false;
      timeline.replaceChildren(
        ...events.map((event) => {
          const kind =
            event.report.kind ?? String(event.report.event?.kind ?? "reported");
          const row = button("", () => selectEvent(event), "event");
          row.dataset.sequence = String(event.sequence);
          row.append(
            el("span", "event-index", event.sequence),
            el(
              "div",
              "",
              el("strong", "", kind),
              el("small", "muted", date(event.recorded_at_ms)),
            ),
            badge(eventOrigin(event)[0]),
          );
          return row;
        }),
      );
      if (!events.length)
        timeline.append(empty("暂无已记录事件", "未上报的活动未知。"));
      if (events[0]) selectEvent(events[0]);
      more.hidden = value.next_after === null;
    } catch (e) {
      if (ctx.signal.aborted) return;
      events.length = 0;
      after = 0;
      timeline.replaceChildren();
      details.replaceChildren();
      assessments.replaceChildren();
      declarations.replaceChildren();
      coverage.hidden = true;
      summary.textContent = "";
      message(error, e);
    } finally {
      more.disabled = false;
    }
  };
  const more = button("加载后续事件", load);
  root.append(
    heading(
      "Run 详情",
      id,
      button("返回运行列表", () => ctx.navigate("runs")),
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
    coverage,
    panel("外部核验记录", assessments),
    error,
  );
  await load();
  if (events.length && !ctx.signal.aborted) await verificationPanel(assessments, id, ctx);
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
