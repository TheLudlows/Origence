import { api } from "./api.js";
import { button, code, el, empty, heading, message, panel, short } from "./components.js";
import type { Asset, Page, Run, RunEvent } from "./types.js";
import { globalReviewPage } from "./global-knowledge.js";

const prefix = "/admin/deployment/workspaces";
type Workspace = { workspace_id: string; name: string };
type Turn = { question: string; answer: string };

function cursor(path: string, after: string | number | null) {
  if (after === null) return path;
  return `${path}${path.includes("?") ? "&" : "?"}after=${encodeURIComponent(after)}`;
}

function paged<T, C extends string | number>(
  root: HTMLElement,
  signal: AbortSignal,
  read: (after: C | null) => Promise<{ items: T[]; next_after: C | null }>,
  row: (item: T) => HTMLElement,
) {
  const items = el("div"), feedback = el("div");
  let after: C | null = null;
  const more = button("加载更多", load);
  async function load() {
    if (signal.aborted) return;
    more.disabled = true;
    feedback.replaceChildren();
    try {
      const page = await read(after);
      if (signal.aborted) return;
      after = page.next_after;
      items.append(...page.items.map(row));
      more.textContent = "加载更多";
      more.hidden = after === null;
      if (!items.childElementCount && more.hidden)
        items.append(empty("暂无记录", "此范围内暂无可查看的记录。"));
    } catch (error) {
      if (signal.aborted) return;
      message(feedback, error);
      more.textContent = "重试";
    } finally {
      more.disabled = false;
    }
  }
  root.append(items, feedback, more);
  void load();
}

export function deploymentConsole(root: HTMLElement, signal: AbortSignal, logout: () => Promise<void>) {
  const workspaceList = el("div"), content = el("main", "content");
  const title = "部署管理";
  root.replaceChildren(el("div", "shell", el("aside", "sidebar",
    button("全局知识审核", reviews, "workspace-item"),
    el("h2", "", "工作空间"), workspaceList, button("退出", logout, "sidebar-exit"),
  ), el("div", "workspace-main", content)));
  content.append(heading(title, "选择工作空间，查看其会话、运行和已发布内容。"));
  let selected: AbortController | undefined;

  function reviews() {
    selected?.abort();
    selected = new AbortController();
    globalReviewPage(content, AbortSignal.any([signal, selected.signal]));
  }

  function select(workspace: Workspace) {
    selected?.abort();
    selected = new AbortController();
    const scope = AbortSignal.any([signal, selected.signal]);
    const base = `${prefix}/${encodeURIComponent(workspace.workspace_id)}`;
    const view = el("div");
    let viewLifetime: AbortController | undefined;
    const show = (headingText: string, render: (target: HTMLElement, viewSignal: AbortSignal) => void) => {
      viewLifetime?.abort();
      viewLifetime = new AbortController();
      const viewSignal = AbortSignal.any([scope, viewLifetime.signal]);
      const section = panel(headingText);
      view.replaceChildren(section);
      render(section, viewSignal);
    };
    const openArchive = (session: string) => show(`会话 ${short(session)}`, (target, viewSignal) => {
      paged<Turn, number>(target, viewSignal, async (after) => {
        const page = await api.get<{ turns: Turn[]; next_after: number | null }>(cursor(`${base}/sessions/${encodeURIComponent(session)}/turns`, after), viewSignal);
        return { items: page.turns, next_after: page.next_after };
      }, (turn) => panel(turn.question, el("p", "archive-answer", turn.answer)));
    });
    const archives = () => show("归档会话", (target, viewSignal) =>
      paged<string, string>(target, viewSignal, (after) => api.get<Page<string>>(cursor(`${base}/sessions`, after), viewSignal),
        (session) => button(`会话 ${short(session)}`, () => openArchive(session), "link")));
    const openRun = (run: Run) => show(`Run ${short(run.run_id)}`, (target, viewSignal) => {
      const summary = el("p");
      target.append(summary);
      paged<RunEvent, number>(target, viewSignal, async (after) => {
        const page = await api.get<{ run: { summary: string }; events: RunEvent[]; next_after: number | null }>(cursor(`${base}/runs/${encodeURIComponent(run.run_id)}`, after), viewSignal);
        if (!viewSignal.aborted) summary.textContent = page.run.summary;
        return { items: page.events, next_after: page.next_after };
      }, (event) => panel(`事件 ${event.sequence}`, code(event.report)));
    });
    const runs = () => show("运行记录", (target, viewSignal) =>
      paged<Run, string>(target, viewSignal, (after) => api.get<Page<Run>>(cursor(`${base}/runs`, after), viewSignal),
        (run) => button(`${short(run.run_id)} · ${run.status}`, () => openRun(run), "link")));
    const openAsset = (asset: Asset) => show(asset.title, (target, viewSignal) => {
      const path = `${base}/assets/${encodeURIComponent(asset.asset_id)}?version=${asset.version}`;
      void api.get<Asset>(path, viewSignal).then((value) => {
        if (!viewSignal.aborted) target.append(el("p", "archive-answer", value.content ?? ""));
      }).catch((error: unknown) => {
        if (!viewSignal.aborted) message(target, error);
      });
    });
    const assets = (kind: "memory" | "knowledge", label: string) => show(label, (target, viewSignal) =>
      paged<Asset, string>(target, viewSignal, (after) => api.get<Page<Asset>>(cursor(`${base}/assets?kind=${kind}`, after), viewSignal),
        (asset) => button(asset.title, () => openAsset(asset), "link")));
    content.replaceChildren(heading(workspace.name, workspace.workspace_id), el("nav", "actions",
      button("会话", archives), button("运行", runs),
      button("记忆", () => assets("memory", "记忆")), button("知识", () => assets("knowledge", "知识"))), view);
    archives();
  }

  paged<Workspace, string>(workspaceList, signal, (after) => api.get<Page<Workspace>>(cursor(prefix, after), signal),
    (workspace) => button(workspace.name, () => select(workspace), "workspace-item"));
}
