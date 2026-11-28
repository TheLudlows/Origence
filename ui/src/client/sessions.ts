import { api } from "./api.js";
import { button, el, empty, heading, message, panel, type ViewContext } from "./components.js";

interface SessionTurnsPage {
  turns: { turn_id: string; question: string; answer: string }[];
  next_after: number | null;
  through_sequence: string;
}

export async function sessionsPage(root: HTMLElement, ctx: ViewContext) {
  const canDelete = ctx.role !== "workspace";
  root.append(heading("会话", canDelete ? "查看当前 Key 在此工作空间保存的问答。" : "查看本工作空间保存的问答。"));
  if (ctx.role === "reader") {
    root.append(empty("需要写入权限", "读取会话记录需要使用具备写入权限的所属 Key。"));
    return;
  }
  const { session_id } = await api.get<{ session_id: string }>("/v1/session", ctx.signal);
  const path = "/v1/sessions/" + encodeURIComponent(session_id);
  const list = el("div", "");
  const status = el("div", "");
  status.setAttribute("role", "status");
  root.append(status, list);
  let cutoff = "0";
  let after: number | null = 0;
  let busy = false;

  async function remove(target: string, bulk = false) {
    list.inert = true;
    status.replaceChildren(el("p", "muted", "正在删除…"));
    try {
      do {
        const result = await api.request<{ next_after?: string | null }>(target, {
          method: "DELETE", signal: ctx.signal,
        });
        if (!bulk || result.next_after == null) break;
        const query = new URLSearchParams(target.slice(target.indexOf("?") + 1));
        query.set("after", result.next_after);
        target = path + "?" + query;
      } while (!ctx.signal.aborted);
    } catch (error) {
      if (!ctx.signal.aborted) {
        message(status, error);
        status.prepend(el("p", "", "删除未确认"));
        const retry = button("重试删除", () => remove(target, bulk));
        status.append(retry, button("返回列表", refresh));
        retry.focus();
      }
      return;
    }
    if (ctx.signal.aborted) return;
    await refresh();
    if (!ctx.signal.aborted) status.prepend(el("p", "", "删除完成"));
  }

  async function load() {
    if (busy || after === null) return;
    busy = true;
    more.disabled = true;
    try {
      const page = await api.get<SessionTurnsPage>(path + "/turns?after=" + after, ctx.signal);
      if (after === 0) cutoff = page.through_sequence;
      after = page.next_after;
      for (const turn of page.turns) {
        const target = path + "/turns/" + encodeURIComponent(turn.turn_id);
        turns.append(panel(turn.question, el("p", "plain-text", turn.answer),
          canDelete ? button("删除这一轮", () => {
            if (!busy && window.confirm("删除这一轮问答？")) return remove(target);
          }, "link") : null));
      }
      more.hidden = after === null;
      clear.disabled = cutoff === "0";
      if (!turns.childElementCount) turns.append(empty("暂无会话记录", "客户端保存的问答会显示在这里。"));
    } catch (error) {
      message(status, error);
      status.append(button("重试读取", refresh));
    } finally {
      busy = false;
      more.disabled = false;
    }
  }

  async function refresh() {
    status.replaceChildren();
    turns.replaceChildren();
    list.inert = false;
    after = 0;
    await load();
  }

  const clear = button("清空会话记录", async () => {
    if (busy) return;
    clear.disabled = true;
    list.inert = true;
    try {
      const current = await api.get<SessionTurnsPage>(path + "/turns?limit=1", ctx.signal);
      if (ctx.signal.aborted) return;
      const target = path + "?through_sequence=" + encodeURIComponent(current.through_sequence);
      if (window.confirm("清空当前会话记录？此后新提交的问答会保留。")) {
        await remove(target, true);
        return;
      }
    } catch (error) {
      if (!ctx.signal.aborted) message(status, error);
    }
    list.inert = false;
    clear.disabled = cutoff === "0";
  });
  clear.disabled = true;
  const turns = el("div", "");
  const more = button("加载更多", load);
  more.hidden = true;
  if (canDelete) list.append(clear);
  list.append(turns, more);
  await load();
}
