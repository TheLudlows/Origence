import { api } from "./api.js";
import { button, el, empty, heading, message, panel, type ViewContext } from "./components.js";

interface SessionTurnsPage {
  turns: { question: string; answer: string }[];
  next_after: number | null;
}

export async function sessionsPage(root: HTMLElement, ctx: ViewContext) {
  root.append(heading("会话", "查看本工作空间保存的问答。"));
  const { session_id } = await api.get<{ session_id: string }>("/v1/session", ctx.signal);
  const path = "/v1/sessions/" + encodeURIComponent(session_id) + "/turns?after=";
  const status = el("div", "");
  status.setAttribute("role", "status");
  const turns = el("div", "");
  let after: number | null = 0;
  let busy = false;
  const more = button("加载更多", load);
  more.hidden = true;
  root.append(status, turns, more);

  async function load() {
    if (busy || after === null) return;
    busy = true;
    more.disabled = true;
    status.replaceChildren();
    try {
      const page = await api.get<SessionTurnsPage>(path + after, ctx.signal);
      after = page.next_after;
      for (const turn of page.turns) {
        turns.append(panel(turn.question, el("p", "plain-text", turn.answer)));
      }
      more.hidden = after === null;
      if (!turns.childElementCount) turns.append(empty("暂无会话记录", "客户端保存的问答会显示在这里。"));
    } catch (error) {
      if (ctx.signal.aborted) return;
      message(status, error);
      status.append(button("重试读取", load));
    } finally {
      busy = false;
      more.disabled = false;
    }
  }

  await load();
}
