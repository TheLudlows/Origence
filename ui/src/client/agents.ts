import { api } from "./api.js";
import { badge, button, date, el, empty, heading, message, panel, table, type ViewContext } from "./components.js";

const labels = {
  codex: "Codex",
  claude_code: "Claude Code",
  other: "其他 MCP 客户端",
};

interface AgentsResponse {
  workspace_id: string;
  window_seconds: number;
  capacity_reached: boolean;
  items: {
    role: string;
    observation: {
      credential_id: string;
      client_kinds: (keyof typeof labels)[];
      last_seen_at_ms: number;
      active_requests: number;
    };
  }[];
}

export function agentsPage(root: HTMLElement, ctx: ViewContext, workspaceId: string) {
  const description = el("p", "muted");
  const status = el("p", "muted", "正在读取 Agent 连接…");
  status.setAttribute("role", "status");
  const result = el("div");
  const refresh = button("刷新", () => load());
  let timer: number | undefined;
  let pending = false;
  async function load() {
    if (pending || ctx.signal.aborted) return;
    window.clearTimeout(timer);
    pending = true;
    refresh.setAttribute("aria-disabled", "true");
    try {
      const data = await api.get<AgentsResponse>("/admin/agents", ctx.signal);
      if (ctx.signal.aborted) return;
      if (data.workspace_id !== workspaceId) throw new Error("返回的工作区不匹配");
      description.textContent = `涵盖正在处理的请求，以及过去 ${data.window_seconds / 60} 分钟内的活动。共用同一凭据的 Agent 合并显示，客户端类型来自握手。`;
      const summary = `${data.items.length} 个有活动的凭据`;
      if (status.textContent !== summary) status.textContent = summary;
      result.replaceChildren();
      if (data.capacity_reached) result.append(el("p", "muted", "服务已达观测容量，列表可能不全。"));
      if (data.items.length === 0) {
        result.append(empty("暂无 Agent 活动", "使用当前工作区的凭据发起 MCP 请求后会显示在这里。"));
      } else {
        result.append(table(["客户端", "凭据 ID", "权限", "状态", "最近活动"], data.items.map(({ role, observation }) => [
          observation.client_kinds.map(kind => labels[kind]).join("、") || "未识别",
          el("span", "mono", observation.credential_id),
          role,
          badge(observation.active_requests ? `正在处理（${observation.active_requests}）` : "近期活动"),
          date(observation.last_seen_at_ms),
        ])));
      }
    } catch (error) {
      if (ctx.signal.aborted) return;
      description.textContent = "";
      status.textContent = "Agent 连接暂不可用";
      result.replaceChildren();
      message(result, error);
    } finally {
      pending = false;
      refresh.removeAttribute("aria-disabled");
      if (!ctx.signal.aborted) timer = window.setTimeout(load, 15_000);
    }
  }
  ctx.signal.addEventListener("abort", () => window.clearTimeout(timer), { once: true });
  root.append(heading("Agent 连接", "查看当前工作区的 MCP 请求活动。", refresh), panel("当前工作区", description, status, result));
  void load();
}
