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
import type { Bundle } from "./types.js";

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
  const session = el("input");
  session.type = "checkbox";
  const result = el("div");
  const run = submit("预览上下文");
  let currentRequest: AbortController | undefined;
  const request = form(
    async () => {
      currentRequest?.abort();
      currentRequest = new AbortController();
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
          },
          AbortSignal.any([ctx.signal, currentRequest.signal]),
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
          detail.append(
            el(
              "summary",
              "",
              item.title || item.evidence_id,
              badge(item.route),
              badge(item.semantic_status ?? item.normalization_status),
            ),
            code(item.content),
            citation(item.provenance, ctx),
          );
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
        message(result, e);
      } finally {
        run.disabled = false;
      }
    },
    field("任务与查询", query),
    el(
      "div",
      "form-grid",
      field("检索方式", mode),
      field("总预算（UTF-8 字节）", budget),
    ),
    ctx.role !== "reader" ? field("包含当前 key 的私有会话", session) : null,
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
