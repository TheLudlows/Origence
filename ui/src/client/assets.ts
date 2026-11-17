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
  labelControl,
  message,
  panel,
  select,
  short,
  status,
  submit,
  table,
  type ViewContext,
} from "./components.js";
import type { Asset, Page, Receipt } from "./types.js";
import { sourcePreview, textDelta } from "./source.js";

import { editor, capture } from "./asset-editor.js";
import { trackJob } from "./jobs.js";

const canWrite = (role: string) =>
  ["writer", "reviewer", "admin"].includes(role);

export async function assetsPage(
  root: HTMLElement,
  kind: Asset["kind"],
  ctx: ViewContext,
) {
  const title = kind === "memory" ? "记忆管理" : "知识管理";
  const list = el("div");
  const error = el("div");
  const search = input("筛选本页：标题、主体、属性或条件");
  const query = input("检索全部可见内容");
  query.required = true;
  const mode = select([
    ["keyword", "关键词"],
    ["hybrid", "混合"],
    ["vector", "向量"],
  ]);
  const hits = el("div");
  const retrieve = form(
    async () => {
      try {
        const result = await api.post<{
          hits: (Asset & { locator: Record<string, unknown> })[];
          warnings?: string[];
          effective_mode?: string;
        }>(
          "/v1/search",
          { query: query.value, mode: mode.value, limit: 50 },
          ctx.signal,
        );
        if (ctx.signal.aborted) return;
        const matches = result.hits.filter((item) => item.kind === kind);
        hits.replaceChildren(
          el(
            "p",
            "muted",
            `混合资产前 50 项命中中的${title}结果 · 实际模式 ${result.effective_mode ?? mode.value}`,
          ),
        );
        for (const warning of result.warnings ?? [])
          hits.append(el("p", "notice", warning));
        if (!matches.length)
          hits.append(
            empty(
              "本次没有匹配结果",
              "搜索命中不是完整目录；可以尝试调整关键词或检索方式。",
            ),
          );
        for (const item of matches) {
          const source = el("div");
          hits.append(
            panel(
              item.title || short(item.asset_id),
              code(item.content),
              code(item.locator),
              button(
                `查看命中版本 v${item.version}`,
                () => ctx.navigate("asset", item.asset_id, item.version),
                "link",
              ),
              canWrite(ctx.role)
                ? button("核验原始引用", () =>
                    sourcePreview(
                      source,
                      item.source_event_id,
                      item.locator,
                      ctx,
                    ),
                  )
                : null,
              source,
            ),
          );
        }
      } catch (e) {
        message(hits, e);
      }
    },
    el("div", "form-grid", field("查询", query), field("检索方式", mode)),
    submit("检索预览"),
  );
  const direct = input("资产 UUID");
  const jobId = input("任务 UUID");
  let items: Asset[] = [],
    after: string | null = null;
  const render = () => {
    const filtered = items.filter((a) =>
      JSON.stringify([a.title, a.identity])
        .toLowerCase()
        .includes(search.value.toLowerCase()),
    );
    const rows = filtered.map((a) => [
      el(
        "div",
        "",
        el("strong", "", a.title || a.identity?.predicate || short(a.asset_id)),
        el(
          "p",
          "mono muted",
          a.identity
            ? `${a.identity.subject.kind}:${a.identity.subject.stable_id} · ${a.identity.predicate}`
            : a.asset_id,
        ),
      ),
      kind === "memory" && a.identity
        ? code(a.identity.context)
        : badge("来源有效"),
      kind === "memory"
        ? status(a.semantic_status ?? a.normalization_status)
        : badge("已发布", "good"),
      `v${a.version}`,
      button("查看详情 →", () => ctx.navigate("asset", a.asset_id), "link"),
    ]);
    list.replaceChildren(
      filtered.length
        ? table(
            [
              "标题 / 主体",
              kind === "memory" ? "适用条件" : "来源",
              "状态",
              "版本",
              "操作",
            ],
            rows,
          )
        : empty(
            "没有匹配的资产",
            "此处仅显示当前权限下已发布且来源有效的内容。",
          ),
    );
  };
  const more = button("加载下一页", () => load());
  const load = async () => {
    more.disabled = true;
    try {
      const page = await api.get<Page<Asset>>(
        `/v1/assets?kind=${kind}&limit=25${after ? "&after=" + after : ""}`,
        ctx.signal,
      );
      if (ctx.signal.aborted) return;
      items.push(...page.items);
      after = page.next_after;
      more.hidden = !after;
      render();
    } catch (e) {
      message(error, e);
    } finally {
      more.disabled = false;
    }
  };
  search.addEventListener("input", render);
  root.append(
    heading(
      title,
      kind === "memory"
        ? "管理可复用的事实、偏好、决策与经验。"
        : "管理文档、来源及异步入库生命周期。",
      canWrite(ctx.role) && kind === "memory"
        ? button("从文本提取", () => capture(ctx))
        : null,
      canWrite(ctx.role)
        ? button(
            kind === "memory" ? "＋ 新建记忆" : "＋ 新建 / 上传",
            () => editor(kind, ctx),
            "primary",
          )
        : null,
    ),
    panel("已发布资产", labelControl(search, "筛选已加载的资产"), list, more),
    panel("检索预览", retrieve, hits),
    panel(
      "通过标识打开",
      form(
        () => ctx.navigate("asset", direct.value.trim()),
        field("资产 ID", direct),
        submit("打开资产"),
      ),
      canWrite(ctx.role)
        ? form(
            async () => {
              try {
                await trackJob({ job_id: jobId.value.trim() }, error, ctx);
              } catch (e) {
                message(error, e);
              }
            },
            field("任务 ID", jobId),
            submit("查看入库任务"),
          )
        : null,
    ),
    error,
  );
  await load();
}

export async function assetDetail(
  root: HTMLElement,
  id: string,
  version: number | undefined,
  ctx: ViewContext,
) {
  const asset = await api.get<Asset>(
    `/v1/assets/${encodeURIComponent(id)}${version ? "?version=" + version : ""}`,
    ctx.signal,
  );
  if (ctx.signal.aborted) return;
  const info = el("div"),
    error = el("div");
  const oldVersion = input(
    "对比版本",
    String(Math.max(1, asset.version - 1)),
    "number",
  );
  oldVersion.min = "1";
  const historyVersion = input("版本号", String(asset.version), "number");
  historyVersion.min = "1";
  const loadSource = async () => {
    try {
      info.replaceChildren(
        code(await api.get(`/v1/events/${asset.source_event_id}`, ctx.signal)),
      );
    } catch (e) {
      message(info, e);
    }
  };
  const compare = async () => {
    try {
      const old = await api.get<Asset>(
        `/v1/assets/${id}?version=${Number(oldVersion.value)}`,
        ctx.signal,
      );
      const delta = textDelta(old.content ?? "", asset.content ?? "");
      info.replaceChildren(
        el(
          "p",
          "muted",
          `v${old.version} → v${asset.version} · 删除内容以删除线显示，新增内容以绿色显示`,
        ),
        el(
          "pre",
          "code",
          delta.prefix,
          el("del", "", delta.removed),
          el("ins", "", delta.added),
          delta.suffix,
        ),
      );
    } catch (e) {
      message(info, e);
    }
  };
  const mutate = async (path: string, method: string, data?: unknown) => {
    if (
      !confirm(
        "此操作将阻断相关内容的后续读取，包括受影响的历史引用。确定继续？",
      )
    )
      return;
    try {
      await api.request(path, {
        method,
        data,
        signal: ctx.signal,
        key: crypto.randomUUID(),
      });
      root.replaceChildren(
        empty("内容已阻断", "来源或资产已撤回，后续访问以服务端状态为准。"),
      );
    } catch (e) {
      message(error, e);
    }
  };
  const update = async () => {
    try {
      editor(
        asset.kind,
        ctx,
        await api.get<Asset>(`/v1/assets/${id}`, ctx.signal),
      );
    } catch (e) {
      message(error, e);
    }
  };
  const restore = async () => {
    const reason = prompt(
      `将 v${asset.version} 恢复为一个新版本，请填写原因：`,
    );
    if (!reason?.trim()) return;
    try {
      const current = await api.get<Asset>(`/v1/assets/${id}`, ctx.signal);
      await trackJob(
        await api.post<Receipt>(
          `/v1/assets/${id}/restore`,
          {
            target_version: asset.version,
            expected_version: current.version,
            reason,
          },
          ctx.signal,
          crypto.randomUUID(),
        ),
        error,
        ctx,
      );
    } catch (e) {
      message(error, e);
    }
  };
  info.append(code(asset.content));
  root.append(
    heading(
      asset.title || "记忆详情",
      `${asset.kind === "memory" ? "记忆" : "知识"} · v${asset.version} · ${id}`,
      button("返回列表", () => ctx.navigate(asset.kind)),
      canWrite(ctx.role) ? button("更新内容", update, "primary") : null,
    ),
    panel(
      "版本与身份",
      el(
        "div",
        "actions",
        status(asset.semantic_status ?? asset.normalization_status),
        badge(`v${asset.version}`),
      ),
      asset.identity ? code(asset.identity) : null,
      el("p", "mono muted", `SHA256 ${asset.content_hash}`),
      form(
        () => ctx.navigate("asset", id, Number(historyVersion.value)),
        field("读取固定历史版本", historyVersion),
        submit("读取版本"),
      ),
    ),
    panel(
      "内容与来源",
      el(
        "div",
        "tabs",
        button("正文", () => info.replaceChildren(code(asset.content))),
        canWrite(ctx.role) ? button("原始来源", loadSource) : null,
        field("对比版本", oldVersion),
        button("比较正文", compare),
      ),
      info,
    ),
    panel(
      "来源引用",
      el("p", "mono", asset.source_event_id),
      el(
        "p",
        "muted",
        "资产版本与来源版本分别保留；原始来源需要独立读取权限。",
      ),
      asset.semantic_values ? code(asset.semantic_values) : null,
      el(
        "div",
        "actions",
        ["reviewer", "admin"].includes(ctx.role) && !asset.semantic_status
          ? button("恢复此版本", restore)
          : null,
        ctx.role === "admin"
          ? button(
              "撤回来源",
              () => mutate(`/v1/events/${asset.source_event_id}`, "DELETE"),
              "danger",
            )
          : null,
        ctx.role === "admin"
          ? button(
              "删除资产",
              () => mutate(`/v1/assets/${id}`, "DELETE"),
              "danger",
            )
          : null,
      ),
    ),
    error,
  );
}
