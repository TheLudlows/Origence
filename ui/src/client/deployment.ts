import { api } from "./api.js";
import { button, code, cursor, el, field, form, heading, input, message, paged, panel, short, submit, type ViewContext } from "./components.js";
import type { Asset, Page, Run, RunEvent } from "./types.js";
import { globalReviewPage } from "./global-knowledge.js";
import { editor } from "./asset-editor.js";
import { assetDetail } from "./assets.js";
import { skillReviewPage } from "./skill-review.js";

const prefix = "/admin/deployment/workspaces";
type Workspace = { workspace_id: string; name: string };
type Turn = { question: string; answer: string };
type IssuedKey = { key_id: string; token: string };

export function deploymentConsole(root: HTMLElement, signal: AbortSignal, logout: () => Promise<void>) {
  const workspaceList = el("div"), content = el("main", "content");
  const title = "管理员";
  root.replaceChildren(el("div", "shell", el("aside", "sidebar",
    button("全局知识审核", reviews, "workspace-item"),
    button("新建工作区", createWorkspace, "workspace-item"),
    el("h2", "", "工作空间"), workspaceList, button("退出", logout, "sidebar-exit"),
  ), el("div", "workspace-main", content)));
  content.append(heading(title, "选择工作空间，查看会话和运行记录，管理记忆、知识及 Key。"));
  let selected: AbortController | undefined;
  let listRequest: AbortController | undefined;

  function revealKey(target: HTMLElement, key: IssuedKey) {
    const token = input("", key.token);
    token.readOnly = true;
    token.autocomplete = "off";
    token.spellcheck = false;
    target.replaceChildren(
      el("p", "muted", "Key 明文仅显示一次，请保存到凭据管理工具。"),
      field("新工作区 Key", token),
    );
    token.focus();
    token.select();
  }

  function createWorkspace() {
    selected?.abort();
    selected = new AbortController();
    const requestSignal = AbortSignal.any([signal, selected.signal]);
    const name = input("输入工作区名称");
    name.required = true;
    name.maxLength = 256;
    const create = submit("创建工作区");
    const result = el("div");
    let attempt: { id: string; name: string } | undefined;
    const request = form(async () => {
      if (create.disabled) return;
      create.disabled = true;
      result.replaceChildren();
      try {
        const value = name.value.trim();
        if (!value || new TextEncoder().encode(value).length > 256)
          throw new Error("请输入工作区名称，最多 256 字节");
        if (attempt?.name !== value) attempt = { id: crypto.randomUUID(), name: value };
        const { key, ...workspace } = await api.request<Workspace & { key?: IssuedKey }>(`${prefix}/${attempt.id}`, {
          method: "PUT", data: { name: value }, signal: requestSignal,
        });
        if (requestSignal.aborted) return;
        request.remove();
        refreshWorkspaces();
        const info = el("div");
        result.replaceChildren(
          el("p", "", `已创建「${workspace.name}」。`), info,
          button("打开工作区", () => select(workspace)),
        );
        if (key) revealKey(info, key);
        else info.append(el("p", "muted", "如未收到首次签发的 Key，请打开工作区轮换。"));
      } catch (error) {
        if (!requestSignal.aborted) message(result, error);
      } finally {
        create.disabled = false;
      }
    }, field("工作区名称", name), create);
    content.replaceChildren(heading("新建工作区", "创建独立工作区并签发它的 Key。"), panel("工作区", request, result));
    name.focus();
  }

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
    const show = (headingText: string | null, render: (target: HTMLElement, viewSignal: AbortSignal) => void) => {
      viewLifetime?.abort();
      viewLifetime = new AbortController();
      const viewSignal = AbortSignal.any([scope, viewLifetime.signal]);
      const section = headingText ? panel(headingText) : el("div");
      view.replaceChildren(section);
      render(section, viewSignal);
    };
    const openSession = (session: string) => show(`会话 ${short(session)}`, (target, viewSignal) => {
      paged<Turn, number>(target, viewSignal, async (after) => {
        const page = await api.get<{ turns: Turn[]; next_after: number | null }>(cursor(`${base}/sessions/${encodeURIComponent(session)}/turns`, after), viewSignal);
        return { items: page.turns, next_after: page.next_after };
      }, (turn) => panel(turn.question, el("p", "plain-text", turn.answer)));
    });
    const sessions = () => show("会话", (target, viewSignal) =>
      paged<string, string>(target, viewSignal, (after) => api.get<Page<string>>(cursor(`${base}/sessions`, after), viewSignal),
        (session) => button(`会话 ${short(session)}`, () => openSession(session), "link")));
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
    const label = (kind: Asset["kind"]) => kind === "memory" ? "记忆" : "知识";
    const editContext = (kind: Asset["kind"], signal: AbortSignal): ViewContext => ({
      signal,
      navigate: (page, id, version) => {
        if (page === "asset" && id) openAsset(kind, id, version);
        else if (page === kind) assets(kind);
      },
    });
    const openAsset = (kind: Asset["kind"], assetId: string, version?: number) => show(null, (target, viewSignal) => {
      void assetDetail(target, assetId, version, editContext(kind, viewSignal), { base, workspaceName: workspace.name })
        .catch((error: unknown) => {
          if (!viewSignal.aborted) {
            const feedback = el("div");
            message(feedback, error);
            target.replaceChildren(button("返回列表", () => assets(kind)), feedback);
          }
        });
    });
    const assets = (kind: Asset["kind"]) => show(label(kind), (target, viewSignal) => {
      target.append(button(`新建${label(kind)}`, () => editor(kind, editContext(kind, viewSignal), undefined, base)));
      paged<Asset, string>(target, viewSignal, (after) => api.get<Page<Asset>>(cursor(`${base}/assets?kind=${kind}`, after), viewSignal),
        (asset) => button(asset.title, () => openAsset(kind, asset.asset_id, asset.version), "link"));
    });
    const keys = () => show("API Key", (target, viewSignal) => {
      const result = el("div");
      target.append(result);
      void api.get<{ key: { key_id: string } | null }>(`${base}/keys`, viewSignal).then((page) => {
        if (viewSignal.aborted) return;
        const state = el("p", "muted", page.key?.key_id ?? "暂无有效 Key");
        const rotate = button("轮换工作区 Key", async () => {
          if (rotate.disabled || !window.confirm("轮换后，旧 Key 将立即失效。继续？")) return;
          rotate.disabled = true;
          result.replaceChildren();
          try {
            const key = await api.request<IssuedKey>(`${base}/keys/rotate`, { method: "POST", signal: viewSignal });
            if (!viewSignal.aborted) {
              state.textContent = key.key_id;
              revealKey(result, key);
            }
          } catch (error) {
            if (!viewSignal.aborted) message(result, error);
          } finally {
            rotate.disabled = false;
          }
        });
        target.insertBefore(state, result);
        target.insertBefore(rotate, result);
      }).catch((error: unknown) => {
        if (!viewSignal.aborted) message(result, error);
      });
    });
    content.replaceChildren(heading(workspace.name, workspace.workspace_id), el("nav", "actions",
      button("会话", sessions), button("运行", runs),
      button("记忆", () => assets("memory")), button("知识", () => assets("knowledge")),
      button("Skill 审核", () => show("Skill 审核", (target, viewSignal) => skillReviewPage(target, `${base}/skill-reviews`, viewSignal))),
      button("API Key", keys)), view);
    sessions();
  }

  function refreshWorkspaces() {
    listRequest?.abort();
    listRequest = new AbortController();
    const listSignal = AbortSignal.any([signal, listRequest.signal]);
    workspaceList.replaceChildren();
    paged<Workspace, string>(workspaceList, listSignal, (after) => api.get<Page<Workspace>>(cursor(prefix, after), listSignal),
      (workspace) => button(workspace.name, () => select(workspace), "workspace-item"));
  }

  refreshWorkspaces();
}
