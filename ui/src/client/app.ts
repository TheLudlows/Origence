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
import type { User } from "./types.js";

const mount = document.getElementById("app")!;
const pages = [
  ["overview", "◫", "运行总览"],
  ["runs", "⑂", "调用链路"],
  ["memory", "◇", "记忆管理"],
  ["knowledge", "▤", "知识管理"],
  ["contexts", "⊞", "上下文工作台"],
] as const;
let user: User | undefined;
let route = new AbortController();

function navigate(page: string, id?: string, version?: number) {
  const hash =
    "#" +
    [page, id && encodeURIComponent(id)].filter(Boolean).join("/") +
    (version ? "?version=" + version : "");
  if (location.hash === hash) void render();
  else location.hash = hash;
}

function logout() {
  api.setToken("");
  user = undefined;
  route.abort();
  document.querySelectorAll("dialog").forEach((dialog) => dialog.close());
  mount.replaceChildren();
  login();
}

function login() {
  const token = input("输入当前用户的 Origence API key", "", "password");
  token.required = true;
  token.autocomplete = "off";
  const error = el("div");
  const connect = submit("连接工作空间");
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
        "连接 Origence",
        el("h1", "", "Agent 上下文工作台"),
        el(
          "p",
          "muted",
          "使用你自己的 API key。凭据仅在当前页面内存保留，刷新后需重新连接。",
        ),
        form(
          async () => {
            connect.disabled = true;
            error.replaceChildren();
            try {
              api.setToken(token.value.trim());
              user = await api.get<User>("/v1/whoami");
              token.value = "";
              await render();
            } catch (e) {
              api.setToken("");
              user = undefined;
              message(error, e);
            } finally {
              connect.disabled = false;
            }
          },
          field("API key", token),
          connect,
        ),
        error,
      ),
    ),
  );
}

async function render() {
  if (!user) return;
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
    mount.replaceChildren(
      empty("地址无效", "无法读取该资源标识。"),
      button("回到总览", () => navigate("overview")),
    );
    return;
  }
  const versionValue = new URLSearchParams(search).get("version");
  const version = versionValue ? Number(versionValue) : undefined;
  if (
    versionValue &&
    (!Number.isInteger(version) || version! < 1 || version! > 2147483647)
  ) {
    mount.replaceChildren(
      empty("版本无效", "请输入正整数版本号。"),
      button("回到总览", () => navigate("overview")),
    );
    return;
  }
  const title = pages.find(([key]) => key === page)?.[2] ?? "资产详情";
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
  const content = el("main", "content");
  const ctx: ViewContext = { signal: route.signal, role: user.role, navigate };
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
        el(
          "div",
          "workspace",
          el("small", "", "WORKSPACE"),
          el("p", "mono", short(user.workspace_id)),
        ),
        nav,
        el(
          "div",
          "account",
          el("span", "avatar", user.role.slice(0, 1).toUpperCase()),
          el(
            "div",
            "",
            el("strong", "", user.role),
            el("small", "mono", short(user.id)),
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
            "工程工作空间",
            el("span", "muted", "/"),
            title,
          ),
          el("div", "actions", el("span", "badge", "HTTP API"), theme),
        ),
        content,
      ),
    ),
  );
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
      logout();
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
  if (user) logout();
});
window.addEventListener("pagehide", () => {
  api.setToken("");
  route.abort();
  user = undefined;
  mount.replaceChildren();
});
window.addEventListener("pageshow", (event) => {
  if (event.persisted) login();
});
login();
