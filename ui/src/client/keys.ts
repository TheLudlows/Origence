import { api, ApiError } from "./api.js";
import { button, date, el, empty, field, form, heading, input, message, panel, select, submit, table, type Child, type ViewContext } from "./components.js";

interface KeyGrant {
  logical_key_id: string;
  role: string;
}

interface IssuedKey extends KeyGrant {
  key_id: string;
  default_access: boolean;
  allow_owned_workspaces: boolean;
  created_at_ms: number;
}

interface IncomingGrant extends KeyGrant { active: boolean }
interface NewKey { key_id: string; token: string }
interface KeyPage<T> { items: T[]; next_after: string | null }

const uuid = "[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}";
const roles: [string, string][] = [
  ["reader", "reader · 检索"], ["writer", "writer · 写入、审阅、发布及 Run 上报"],
  ["admin", "admin · 工作区管理"],
];

export function keysPage(mount: HTMLElement, ctx: ViewContext, workspace: { workspace_id: string; id?: string; name: string }, adminPath = "/admin") {
  if (ctx.role !== "admin") {
    mount.append(heading("API Key", "只有工作区管理员可以管理 API Key。"));
    return;
  }
  const keysPath = `${adminPath}/keys`;
  const grantsPath = `${adminPath}/workspace-key-grants`;

  const notice = el("p", "muted");
  notice.setAttribute("role", "status");
  function refresh() {
    void issued.load();
    void incoming.load();
  }

  function action(title: string, description: string, label: string, execute: (signal: AbortSignal) => Promise<NewKey | void>, fields: Child[] = [], yieldsToken = false) {
    const dialog = el("dialog");
    const opener = document.activeElement;
    const h2 = el("h2", "", title);
    h2.id = "key-action-title";
    dialog.setAttribute("aria-labelledby", h2.id);
    const lifetime = new AbortController();
    const signal = AbortSignal.any([ctx.signal, lifetime.signal]);
    const error = el("div");
    const close = button("关闭", () => dialog.close());
    const confirm = submit(label);
    let completed = false;
    let secretField: HTMLInputElement | undefined;

    function reveal(key: NewKey) {
      h2.textContent = "保存新 API Key";
      const token = input("", key.token);
      secretField = token;
      token.readOnly = true;
      token.autocomplete = "off";
      token.spellcheck = false;
      const copyStatus = el("p", "muted");
      copyStatus.setAttribute("role", "status");
      const copy = button("复制 API Key", async () => {
        try {
          await navigator.clipboard.writeText(token.value);
          copyStatus.textContent = "已复制";
        } catch {
          token.select();
          copyStatus.textContent = "请手动复制选中的 API Key";
        }
      });
      dialog.replaceChildren(
        el("div", "dialog-head", h2, close),
        el("p", "", "明文仅在此显示一次。请保存到你的凭据管理工具。"),
        el("p", "mono", key.key_id),
        field("新 API Key", token), copy, copyStatus,
      );
      token.focus();
      token.select();
    }

    dialog.append(
      el("div", "dialog-head", h2, close),
      el("p", "", description),
      form(async () => {
        if (confirm.disabled) return;
        confirm.disabled = close.disabled = true;
        error.replaceChildren();
        let uncertain = false;
        try {
          const result = await execute(signal);
          signal.throwIfAborted();
          completed = true;
          notice.textContent = `${label}成功。`;
          refresh();
          if (result) reveal(result);
          else dialog.close();
        } catch (e) {
          if (signal.aborted) return;
          uncertain = yieldsToken && (!(e instanceof ApiError) || e.status >= 500);
          message(error, uncertain ? new Error("结果未确认。请关闭此窗口并检查列表。若操作已生效，可轮换该凭据获取新 token。") : e);
          if (uncertain || (e instanceof ApiError && [404, 409].includes(e.status))) refresh();
        } finally {
          confirm.disabled = uncertain;
          close.disabled = false;
        }
      }, ...fields, confirm),
      error,
    );
    function eraseSecret() {
      if (secretField) secretField.value = "";
    }
    const closeOnLeave = () => {
      eraseSecret();
      dialog.close();
    };
    ctx.signal.addEventListener("abort", closeOnLeave, { once: true });
    dialog.addEventListener("cancel", (event) => { if (close.disabled) event.preventDefault(); });
    dialog.addEventListener("close", () => {
      lifetime.abort();
      eraseSecret();
      ctx.signal.removeEventListener("abort", closeOnLeave);
      dialog.replaceChildren();
      dialog.remove();
      if (ctx.signal.aborted) return;
      if (!completed && opener instanceof HTMLElement && opener.isConnected) opener.focus();
      else mount.focus();
    }, { once: true });
    document.body.append(dialog);
    dialog.showModal();
    close.focus();
  }

  const issue = button("创建 API Key", () => {
    const role = select(roles);
    action("创建 API Key", `在「${workspace.name}」签发新凭据。`, "创建", (signal) =>
      api.post<NewKey>(keysPath, { workspace_id: workspace.workspace_id, role: role.value }, signal), [field("权限", role)], true);
  }, "primary");

  function grant(existing?: KeyGrant) {
    const target = input("输入逻辑 Key ID", existing?.logical_key_id);
    target.required = true;
    target.pattern = uuid;
    target.readOnly = !!existing;
    const role = select(roles, existing?.role);
    action(existing ? "调整工作区授权" : "添加工作区授权", `指定这个逻辑 Key 在「${workspace.name}」的权限。`, "保存授权", async (signal) => {
      await api.request(`${grantsPath}/${target.value.toLowerCase()}`, { method: "PUT", data: { role: role.value }, signal });
    }, [field("逻辑 Key ID", target), field("权限", role)]);
  }

  function removeGrant(key: KeyGrant) {
    action("移除工作区授权", `逻辑 Key ${key.logical_key_id} 将失去对「${workspace.name}」的访问。`, "确认移除", async (signal) => {
      await api.request(`${grantsPath}/${key.logical_key_id}`, { method: "DELETE", signal });
    });
  }

  const issued = pagination<IssuedKey>("本工作区签发", keysPath, ctx.signal,
    ["凭据 ID", "逻辑 Key ID", "权限", "创建时间", "操作"], (key) => [
      el("code", "mono", key.key_id), el("code", "mono", key.logical_key_id),
      key.default_access ? key.role + (key.allow_owned_workspaces ? " · 含子工作区" : "") : "本工作区访问已停用",
      date(key.created_at_ms), key.key_id === workspace.id ? "当前控制台凭据" : el("div", "actions",
        button(key.default_access ? "调整权限" : "启用本区访问", () => grant(key)),
        key.default_access && button("移除本区授权", () => removeGrant(key)),
        button("轮换", () => action("轮换 API Key", `凭据 ${key.key_id} 的旧 token 将立即失效。逻辑 Key 和权限会保留。操作需要该 Key 的额外授权工作区的管理员权限。`, "确认轮换", (signal) =>
          api.request<NewKey>(`${keysPath}/${key.key_id}/rotate`, { method: "POST", signal }), [], true)),
        button("撤销", () => action("撤销 API Key", `凭据 ${key.key_id} 在所有工作区的访问都将停止。`, "确认撤销", async (signal) => {
          await api.request(`${keysPath}/${key.key_id}`, { method: "DELETE", signal });
        }), "danger"),
      ),
    ]);

  const incoming = pagination<IncomingGrant>("其他工作区签发的 Key", grantsPath, ctx.signal,
    ["逻辑 Key ID", "本工作区权限", "操作"], (key) => [
      el("code", "mono", key.logical_key_id), key.role,
      key.active ? el("div", "actions",
        button("调整权限", () => grant(key)),
        button("移除授权", () => removeGrant(key), "danger"),
      ) : "凭据已撤销",
    ], button("添加工作区授权", () => grant()));

  mount.append(heading("API Key", `管理「${workspace.name}」的 API 凭据与授权。`, issue), notice, issued.section, incoming.section);
  refresh();
}

function pagination<T>(title: string, path: string, signal: AbortSignal, columns: string[], row: (key: T) => Child[], action?: HTMLButtonElement) {
  const entries = el("div");
  const error = el("div");
  const state = el("p", "muted");
  state.setAttribute("role", "status");
  const more = button("加载更多", () => load(true));
  more.hidden = true;
  let cursor: string | null = null;
  let items: T[] = [];
  let active: AbortController | undefined;
  async function load(append = false) {
    active?.abort();
    const query = new AbortController();
    active = query;
    const request = AbortSignal.any([signal, query.signal]);
    const after = append && cursor ? `?after=${encodeURIComponent(cursor)}` : "";
    more.disabled = true;
    error.replaceChildren();
    state.textContent = "正在读取…";
    try {
      const next = await api.get<KeyPage<T>>(path + after, request);
      items = append ? [...items, ...next.items] : next.items;
      cursor = next.next_after;
      entries.replaceChildren(items.length ? table(columns, items.map(row)) : empty("暂无记录", "此工作区暂无对应的凭据。"));
      more.hidden = !cursor;
    } catch (e) {
      if (request.aborted) return;
      message(error, e);
      error.append(button("重试", () => load(append)));
    } finally {
      if (active === query && !signal.aborted) {
        state.textContent = "";
        more.disabled = false;
      }
    }
  }
  return { section: panel(title, action, state, entries, error, more), load };
}
