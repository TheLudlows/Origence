import { api } from "./api.js";
import { badge, button, cursor, el, field, heading, message, paged, panel, select, short, type ViewContext } from "./components.js";
import type { Page } from "./types.js";

type State = "pending" | "approved" | "rejected" | "withdrawn";
type Decision = "approve" | "reject" | "withdraw";
export type GlobalKnowledge = {
  publication_id: string;
  title: string;
  version: number;
  state: State;
  workspace_id?: string;
  content?: string;
};

export const publicationStatus: Record<State, string> = {
  pending: "待审核",
  approved: "已发布",
  rejected: "已驳回",
  withdrawn: "已下架",
};

const publicPath = "/v1/global-knowledge";
const reviewPath = "/admin/deployment/global-knowledge";

function catalog(
  target: HTMLElement,
  path: string,
  signal: AbortSignal,
  open: (id: string) => void,
) {
  target.replaceChildren();
  paged<GlobalKnowledge, string>(target, signal,
    (after) => api.get<Page<GlobalKnowledge>>(cursor(path, after), signal),
    (item) => panel(
      item.title,
      el("p", "muted", `v${item.version} · ${publicationStatus[item.state]}${item.workspace_id ? ` · 工作空间 ${short(item.workspace_id)}` : ""}`),
      button(`查看 ${item.title}`, () => open(item.publication_id), "link"),
    ),
  );
}

export async function globalKnowledgePage(root: HTMLElement, ctx: ViewContext, id?: string) {
  if (!id) {
    const list = el("div");
    root.append(heading("全局知识", "当前部署已批准的知识。"), list);
    catalog(list, publicPath, ctx.signal, (publication) => ctx.navigate("global-knowledge", publication));
    return;
  }
  const item = await api.get<GlobalKnowledge>(`${publicPath}/${encodeURIComponent(id)}`, ctx.signal);
  if (ctx.signal.aborted) return;
  root.replaceChildren(
    heading(item.title, `全局知识 · v${item.version}`, button("返回全局知识", () => ctx.navigate("global-knowledge"))),
    panel("正文", el("p", "plain-text", item.content ?? "")),
  );
}

export function globalReviewPage(root: HTMLElement, signal: AbortSignal) {
  const view = el("div");
  const states = select(Object.entries(publicationStatus));
  let current: AbortController | undefined;
  function start() {
    current?.abort();
    current = new AbortController();
    return AbortSignal.any([signal, current.signal]);
  }
  function list() {
    catalog(view, `${reviewPath}?state=${encodeURIComponent(states.value)}`, start(), (id) => void detail(id));
  }
  async function detail(id: string) {
    const readSignal = start();
    const feedback = el("div");
    view.replaceChildren(button("返回审核列表", list), feedback);
    try {
      const path = `${reviewPath}/${encodeURIComponent(id)}`;
      const item = await api.get<GlobalKnowledge>(path, readSignal);
      if (readSignal.aborted) return;
      const actions = el("div", "actions");
      const decide = (decision: Decision, label: string, style?: string) =>
        button(label, async () => {
          const buttons = actions.querySelectorAll("button");
          for (const control of buttons) control.disabled = true;
          feedback.replaceChildren();
          try {
            const updated = await api.post<GlobalKnowledge>(`${path}/review`, { decision }, readSignal);
            if (!readSignal.aborted) {
              states.value = updated.state;
              list();
            }
          } catch (error) {
            if (!readSignal.aborted) message(feedback, error);
          } finally {
            for (const control of buttons) control.disabled = false;
          }
        }, style);
      if (item.state === "pending")
        actions.append(decide("approve", "批准发布", "primary"), decide("reject", "驳回"));
      if (item.state === "approved") actions.append(decide("withdraw", "下架", "danger"));
      view.append(panel(item.title,
        badge(publicationStatus[item.state]), el("p", "muted", `工作空间 ${item.workspace_id} · v${item.version}`),
        el("p", "plain-text", item.content ?? ""), actions,
      ));
    } catch (error) {
      if (!readSignal.aborted) message(feedback, error);
    }
  }
  root.replaceChildren(heading("全局知识审核", "查看并处理各工作空间提交的知识版本。"), field("审核状态", states), view);
  states.addEventListener("change", list);
  list();
}
