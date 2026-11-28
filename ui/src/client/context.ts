import { api } from "./api.js";
import {
  badge,
  button,
  code,
  el,
  empty,
  field,
  form,
  heading,
  input,
  message,
  panel,
  select,
  submit,
  table,
  type ViewContext,
} from "./components.js";
import { citation } from "./runs.js";
import { sourcePreview, sourcePreviewTarget } from "./source.js";
import type {
  Bundle,
  LongTermMemoryCitation,
  LongTermMemoryProvenance,
  LongTermMemoryState,
} from "./types.js";

function businessDate(ms: number) {
  const date = new Date(ms);
  return Number.isNaN(date.valueOf()) ? String(ms) : date.toISOString().split("T")[0];
}

function memoryState(state: LongTermMemoryState) {
  if (state.status === "current") return state.value;
  return state.status === "conflicting" ? "来源冲突，无法确定单一值" : "该日期缺少适用证据";
}

function memoryCitation(
  entry: LongTermMemoryCitation,
  ctx: ViewContext,
) {
  const preview = sourcePreviewTarget();
  const { byte_start, byte_end, quote } = entry.statement;
  const open = button("打开原文", () => sourcePreview(
    preview, entry.source_event_id, { byte_start, byte_end, byte_basis: "source_utf8" }, ctx, quote,
  ));
  const date = entry.effective_at_ms === null ? "无日期基线" : businessDate(entry.effective_at_ms);
  return el(
    "div", "citation",
    el("p", "muted", `${date} · ${entry.source_event_id} · 字节 ${byte_start}–${byte_end}`),
    open,
    preview.root,
  );
}

function datedMemory(ref: LongTermMemoryProvenance, unverified: boolean, ctx: ViewContext) {
  return el(
    "div", "",
    el("p", "", `${businessDate(ref.as_of_ms)} 的结论 · ${memoryState(ref.state)}`),
    el("p", "muted", `记忆版本 ${ref.version}${unverified ? " · 适用条件待确认" : ""}`),
    table(["生效日期（UTC）", "结论"], ref.timeline.map(({ effective_from_ms, state }) => [
      effective_from_ms === null ? "无日期基线" : businessDate(effective_from_ms),
      memoryState(state),
    ])),
    ...ref.citations.map((entry) => memoryCitation(entry, ctx)),
  );
}

export function contextPage(root: HTMLElement, ctx: ViewContext) {
  const query = input("描述本次任务，例如“Atlas 的生产发布要求”");
  query.required = true;
  const mode = select([
    ["keyword", "关键词"],
    ["hybrid", "混合检索"],
    ["vector", "向量检索"],
  ]);
  const budget = input("预算", "4000", "number");
  budget.min = "0";
  budget.max = "32000";
  budget.required = true;
  const asOf = input("", "", "date");
  const session = el("input");
  session.type = "checkbox";
  const result = el("div");
  const run = submit("预览上下文");
  let currentRequest: AbortController | undefined;
  const request = form(
    async () => {
      currentRequest?.abort();
      const activeRequest = new AbortController();
      currentRequest = activeRequest;
      run.disabled = true;
      result.replaceChildren(el("p", "muted", "正在准备上下文…"));
      try {
        const routes = [
          "memory",
          "knowledge",
          "learning",
          ...(session.checked ? ["session"] : []),
        ].map((route) => ({
          route,
          limit: 20,
          max_bytes: Number(budget.value),
        }));
        const bundle = await api.post<Bundle>(
          "/v1/context",
          {
            query: query.value,
            mode: mode.value,
            budget: Number(budget.value),
            routes,
            ...(asOf.value ? { as_of_ms: asOf.valueAsNumber } : {}),
          },
          AbortSignal.any([ctx.signal, activeRequest.signal]),
        );
        if (ctx.signal.aborted) return;
        const used = el("progress");
        used.max = Math.max(1, bundle.budget);
        used.value = bundle.used_bytes;
        used.setAttribute("aria-label", "UTF-8 字节预算使用");
        result.replaceChildren(
          panel(
            "上下文预算",
            el(
              "div",
              "actions",
              badge(
                `${bundle.used_bytes.toLocaleString()} / ${bundle.budget.toLocaleString()} UTF-8 字节`,
              ),
              badge(`实际检索：${bundle.effective_mode}`),
            ),
            used,
            table(
              ["来源", "保留项", "预算省略", "已用字节"],
              bundle.routes.map((r) => [
                r.route,
                r.retained,
                r.omitted_for_budget,
                r.used_bytes,
              ]),
            ),
          ),
        );
        for (const warning of bundle.warnings)
          result.append(el("p", "notice", warning));
        const items = panel("本次上下文来源");
        if (!bundle.items.length)
          items.append(
            empty(
              "没有可保留的上下文",
              "可能未命中或预算不足；请结合警告与各来源统计判断。",
            ),
          );
        for (const item of bundle.items) {
          const detail = el("details", "source");
          const provenance = item.provenance;
          detail.append(
            el(
              "summary",
              "",
              item.title || item.evidence_id,
              badge(item.route),
              provenance.type === "long_term_memory" && item.conditions?.length
                ? badge(item.conditions.join(" · "))
                : null,
              badge(item.semantic_status ?? item.normalization_status),
            ),
          );
          if (provenance.type === "long_term_memory") {
            detail.append(datedMemory(provenance, item.applicability === "unverified", ctx));
          } else {
            detail.append(code(item.content), citation(provenance, ctx, {
              knowledgeQuote: item.route === "knowledge" ? item.content : undefined,
              label: item.normalization_status === "unnormalized" ? "查看证据" : undefined,
            }));
          }
          items.append(detail);
        }
        result.append(
          items,
          panel("实际渲染内容", code(bundle.rendered_context)),
          el(
            "p",
            "muted",
            "这是新查询的预览，不是历史 Run 快照，也不证明客户端收到或模型使用。",
          ),
        );
      } catch (e) {
        if (currentRequest !== activeRequest || ctx.signal.aborted) return;
        if (activeRequest.signal.aborted)
          result.replaceChildren(
            empty("预览已取消", "可以调整查询后重新预览。"),
          );
        else message(result, e);
      } finally {
        if (currentRequest === activeRequest) run.disabled = false;
      }
    },
    field("任务与查询", query),
    el(
      "div",
      "form-grid",
      field("检索方式", mode),
      field("总预算（UTF-8 字节）", budget),
      field("记忆业务日期（UTC）", asOf),
    ),
    field("包含当前 key 的私有会话", session),
    el(
      "div",
      "actions",
      run,
      button("取消预览", () => currentRequest?.abort()),
    ),
  );
  root.append(
    heading(
      "上下文工作台",
      "组合记忆、知识、经验与获授权会话，查看实际准备的内容。",
    ),
    panel("准备上下文", request),
    result,
  );
}
