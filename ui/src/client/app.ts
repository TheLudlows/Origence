import { api, ApiError } from "./api.js";
import { assetDetail, assetsPage } from "./assets.js";
import {
  button,
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
  type ViewContext,
} from "./components.js";
import { contextPage } from "./context.js";
import { overview, runsPage } from "./runs.js";
import type { ConsoleSession, ConsoleSessionResponse, ConsoleWorkspace } from "./types.js";

const mount = document.getElementById("app")!;
const pages = [
  ["overview", "◫", "运行总览"],
  ["runs", "⑂", "调用链路"],
  ["memory", "◇", "记忆管理"],
  ["knowledge", "▤", "知识管理"],
  ["contexts", "⊞", "上下文工作台"],
] as const;
const sessionPath = "/console/session";
const workspaceStorageKey = "origence.selectedWorkspace";
let session: ConsoleSession | null = null;
let user: ConsoleWorkspace | undefined;
let route = new AbortController();

function focusRoute(target: HTMLElement) {
  target.tabIndex = -1;
  target.focus({ preventScroll: true });
}

function navigate(page: string, id?: string, version?: number) {
  const hash =
    "#" +
    [page, id && encodeURIComponent(id)].filter(Boolean).join("/") +
    (version ? "?version=" + version : "");
  if (location.hash === hash) void render();
  else location.hash = hash;
}

function clearView() {
  api.setWorkspace();
  session = null;
  user = undefined;
  route.abort();
  document.querySelectorAll("dialog").forEach((dialog) => dialog.close());
  mount.replaceChildren();
}

async function logout() {
  clearView();
  try {
    const result = await api.request<ConsoleSessionResponse>(sessionPath, { method: "DELETE" });
    sessionStorage.removeItem(workspaceStorageKey);
    login(result.setupRequired);
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") return;
    const detail = el("div");
    mount.replaceChildren(panel("暂时无法退出", detail, button("重试退出", logout)));
    message(detail, error);
  }
}

function chooseWorkspace(workspace: ConsoleWorkspace, changing = false) {
  if (!session) return;
  api.setWorkspace(workspace.workspace_id, session.binding);
  sessionStorage.setItem(workspaceStorageKey, workspace.workspace_id);
  user = workspace;
  if (changing) {
    route.abort();
    mount.replaceChildren();
    navigate("overview");
  } else void render();
}

function showSession(result: ConsoleSessionResponse) {
  session = result.session;
  if (!session) {
    login(result.setupRequired);
    return;
  }
  const previous = sessionStorage.getItem(workspaceStorageKey);
  const selected = session.workspaces.find((workspace) => workspace.workspace_id === previous);
  const workspace = selected ?? session.workspaces[0];
  if (workspace) {
    chooseWorkspace(workspace, !!previous && !selected);
    return;
  }
  mount.replaceChildren(panel(
    "你还没有获授权的工作区",
    el("p", "muted", "请联系管理员分配工作区权限。"),
    button("重新读取工作区", restoreSession),
    button("退出", logout),
  ));
}

async function restoreSession() {
  clearView();
  mount.replaceChildren(el("p", "muted", "正在检查登录会话…"));
  try {
    showSession(await api.get<ConsoleSessionResponse>(sessionPath));
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") return;
    const detail = el("div");
    mount.replaceChildren(panel("暂时无法读取登录会话", detail, button("重试", restoreSession), button("退出", logout)));
    message(detail, error);
  }
}

function createWorkspace(parent: ConsoleWorkspace, binding: string, parentSignal: AbortSignal) {
  const dialog = el("dialog");
  const lifetime = new AbortController();
  const signal = AbortSignal.any([parentSignal, lifetime.signal]);
  const name = input("输入工作区名称");
  name.required = true;
  name.maxLength = 256;
  const create = submit("创建工作区");
  const detail = el("div");
  let attempt: { name: string; id: string } | undefined;
  const title = el("h2", "", "新建工作区");
  title.id = "create-workspace-title";
  dialog.setAttribute("aria-labelledby", title.id);
  dialog.append(
    el("div", "dialog-head", title, button("关闭", () => dialog.close())),
    el("p", "muted", `将在「${parent.name}」下创建子工作区。`),
    form(async () => {
      create.disabled = true;
      detail.replaceChildren();
      try {
        const value = name.value.trim();
        if (!value || new TextEncoder().encode(value).length > 256)
          throw new Error("请输入工作区名称，最多 256 字节");
        if (!attempt || attempt.name !== value) attempt = { name: value, id: crypto.randomUUID() };
        const id = attempt.id;
        await api.request(`/v1/workspaces/owned/${id}`, { method: "PUT", data: { name: value }, signal });
        const refreshed = await api.get<ConsoleSessionResponse>(sessionPath, signal);
        if (!refreshed.session || refreshed.session.binding !== binding)
          throw new Error("登录会话已变更，请刷新页面");
        const workspace = refreshed.session.workspaces.find((item) => item.workspace_id === id);
        if (!workspace) throw new Error("工作区已创建，列表暂未更新，请重试");
        session = refreshed.session;
        dialog.close();
        chooseWorkspace(workspace, true);
      } catch (error) {
        message(detail, error);
      } finally {
        create.disabled = false;
      }
    }, field("工作区名称", name), create),
    detail,
  );
  dialog.addEventListener("close", () => { lifetime.abort(); dialog.remove(); }, { once: true });
  document.body.append(dialog);
  dialog.showModal();
}

function login(setupRequired?: true) {
  const username = input("输入账号");
  username.required = true;
  username.autocomplete = "username";
  const secret = input("输入密码", "", "password");
  secret.required = true;
  secret.autocomplete = "current-password";
  const error = el("div");
  const connect = submit("登录");
  mount.replaceChildren(
    el(
      "div",
      "login",
      el(
        "div",
        "brand",
        el("span", "brand-mark", "◎"),
        "origence",
        el("span", "accent", "."),
      ),
      panel(
        "登录 Origence",
        el("h1", "", "Agent 上下文工作台"),
        el(
          "p",
          "muted",
          setupRequired ? "控制台尚未创建账号，请联系管理员。" : "登录后可以切换已获授权的工作区。",
        ),
        form(
          async () => {
            connect.disabled = true;
            error.replaceChildren();
            try {
              const result = await api.post<ConsoleSessionResponse>(sessionPath, {
                username: username.value.trim(),
                password: secret.value,
              });
              secret.value = "";
              showSession(result);
            } catch (e) {
              message(error, e);
            } finally {
              connect.disabled = false;
            }
          },
          field("账号", username),
          field("密码", secret),
          connect,
        ),
        error,
      ),
    ),
  );
}

async function render() {
  if (!user || !session) return;
  route.abort();
  route = new AbortController();
  document.querySelectorAll("dialog").forEach((dialog) => dialog.close());
  const raw = location.hash.slice(1) || "overview";
  const [path = "overview", search = ""] = raw.split("?");
  const [page = "overview", encodedId] = path.split("/");
  let id: string | undefined;
  try {
    id = encodedId ? decodeURIComponent(encodedId) : undefined;
  } catch {
    const error = empty("地址无效", "无法读取该资源标识。");
    mount.replaceChildren(
      error,
      button("回到总览", () => navigate("overview")),
    );
    focusRoute(error);
    return;
  }
  const versionValue = new URLSearchParams(search).get("version");
  const version = versionValue ? Number(versionValue) : undefined;
  if (
    versionValue &&
    (!Number.isInteger(version) || version! < 1 || version! > 2147483647)
  ) {
    const error = empty("版本无效", "请输入正整数版本号。");
    mount.replaceChildren(
      error,
      button("回到总览", () => navigate("overview")),
    );
    focusRoute(error);
    return;
  }
  const title = pages.find(([key]) => key === page)?.[2] ?? "内容详情";
  const nav = el("nav");
  nav.setAttribute("aria-label", "主导航");
  for (const [key, icon, label] of pages) {
    const link = button("", () => navigate(key), "nav-item");
    link.append(el("span", "nav-icon", icon), label);
    if (key === page) link.setAttribute("aria-current", "page");
    nav.append(link);
  }
  const theme = button(
    "◐",
    () => {
      const dark =
        getComputedStyle(document.documentElement).colorScheme === "dark" ||
        (!document.documentElement.dataset.theme &&
          matchMedia("(prefers-color-scheme: dark)").matches);
      document.documentElement.dataset.theme = dark ? "light" : "dark";
    },
    "icon-button",
  );
  theme.setAttribute("aria-label", "切换明暗主题");
  const workspaceHeading = el("h2", "", "工作区");
  workspaceHeading.id = "workspace-list-title";
  const workspaceList = el("ul", "workspace-list");
  workspaceList.setAttribute("role", "list");
  workspaceList.setAttribute("aria-labelledby", workspaceHeading.id);
  for (const workspace of session.workspaces) {
    const item = button(workspace.name, () => {
      if (workspace.workspace_id !== user?.workspace_id) chooseWorkspace(workspace, true);
    }, "nav-item");
    if (workspace.workspace_id === user.workspace_id) item.setAttribute("aria-current", "true");
    workspaceList.append(el("li", "", item));
  }
  const content = el("main", "content");
  const ctx: ViewContext = { signal: route.signal, role: user.role, navigate };
  const activeWorkspace = user;
  const binding = session.binding;
  mount.replaceChildren(
    el(
      "div",
      "shell",
      el(
        "aside",
        "sidebar",
        el(
          "div",
          "brand",
          el("span", "brand-mark", "◎"),
          "origence",
          el("span", "accent", "."),
        ),
        el("div", "workspace", workspaceHeading, workspaceList),
        activeWorkspace.can_create_owned_workspaces
          ? button("新建工作区", () => createWorkspace(activeWorkspace, binding, ctx.signal), "nav-item")
          : null,
        nav,
        el(
          "div",
          "account",
          el("span", "avatar", user.role.slice(0, 1).toUpperCase()),
          el(
            "div",
            "",
            el("strong", "", session.account),
            el("small", "mono", user.role),
          ),
          button("退出", logout, "sidebar-exit"),
        ),
      ),
      el(
        "div",
        "workspace-main",
        el(
          "header",
          "topbar",
          el(
            "div",
            "breadcrumb",
            user.name === user.workspace_id ? short(user.workspace_id) : user.name,
            el("span", "muted", "/"),
            title,
          ),
          el("div", "actions", el("span", "badge", "HTTP API"), theme),
        ),
        content,
      ),
    ),
  );
  // Move focus once when replacing the old page. Async results may arrive
  // after the user has already moved focus to a control on the new page.
  focusRoute(content);
  content.append(el("p", "muted", "正在读取…"));
  try {
    content.replaceChildren();
    if (page === "overview") await overview(content, ctx);
    else if (page === "runs") await runsPage(content, ctx, id);
    else if (page === "memory" || page === "knowledge")
      await assetsPage(content, page, ctx);
    else if (page === "contexts") contextPage(content, ctx);
    else if (page === "asset" && id)
      await assetDetail(content, id, version, ctx);
    else content.append(heading("页面不存在", "请使用左侧导航选择功能。"));
  } catch (error) {
    if (ctx.signal.aborted) return;
    if (error instanceof ApiError && error.status === 401) {
      void restoreSession();
      return;
    }
    content.replaceChildren(
      empty("暂时无法读取", "检查权限、来源状态或连接后重试。"),
      button("重新读取", render),
    );
    const detail = el("div");
    content.append(detail);
    message(detail, error);
  }
}

window.addEventListener("hashchange", () => {
  void render();
});
window.addEventListener("origence:unauthorized", () => {
  if (user) void restoreSession();
});
window.addEventListener("pagehide", clearView);
window.addEventListener("pageshow", (event) => {
  if (event.persisted) void restoreSession();
});
void restoreSession();
