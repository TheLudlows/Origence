import { api, ApiError } from "./api.js";
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
  parseObject,
  select,
  short,
  status,
  submit,
  table,
  type ViewContext,
} from "./components.js";
import type { Asset, Page, Receipt } from "./types.js";
import { sourcePreview, sourcePreviewTarget, textDelta } from "./source.js";

import { editor, capture } from "./asset-editor.js";
import { trackJob } from "./jobs.js";

const canWrite = (role: string) =>
  ["writer", "reviewer", "admin"].includes(role);

export async function assetsPage(
  root: HTMLElement,
  kind: Asset["kind"],
  ctx: ViewContext,
) {
  const kindLabel = kind === "memory" ? "记忆" : "知识";
  const title = `${kindLabel}管理`;
  const list = el("div");
  const error = el("div");
  const search = input("筛选本页：标题、主体、属性或条件");
  const query = input("检索可见的记忆与知识");
  query.required = true;
  const mode = select([
    ["keyword", "关键词"],
    ["hybrid", "混合"],
    ["vector", "向量"],
  ]);
  const hits = el("div");
  let currentSearch: AbortController | undefined;
  const retrieve = form(
    async () => {
      currentSearch?.abort();
      const searchRequest = new AbortController();
      currentSearch = searchRequest;
      const searchQuery = query.value;
      hits.replaceChildren(el("p", "muted", "正在检索…"));
      try {
        const result = await api.post<{
          hits: (Asset & { locator: Record<string, unknown> })[];
          warnings?: string[];
          effective_mode?: string;
        }>(
          "/v1/search",
          { query: searchQuery, mode: mode.value, limit: 50 },
          AbortSignal.any([ctx.signal, searchRequest.signal]),
        );
        if (ctx.signal.aborted) return;
        const matches = result.hits.filter((item) => item.kind === kind);
        hits.replaceChildren(
          el(
            "p",
            "muted",
            `“${searchQuery}” · 记忆与知识的前 50 项命中，仅显示${kindLabel} · 实际模式 ${result.effective_mode ?? mode.value}`,
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
          const source = canWrite(ctx.role) ? sourcePreviewTarget() : null;
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
              source
                ? button("核验原始引用", () =>
                    sourcePreview(
                      source,
                      item.source_event_id,
                      item.locator,
                      ctx,
                    ),
                  )
                : null,
              source?.root,
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
  const subjectKind = select([
    ["", "不限"], ["user", "user"], ["agent", "agent"],
    ["project", "project"], ["service", "service"], ["team", "team"],
  ]);
  const subjectId = input("精确业务 ID（区分大小写）");
  const predicate = input("精确属性，如 release.rule");
  const conditions = input('完整条件 JSON；留空不限，{} 仅空条件');
  const identified = select([
    ["", "不限"], ["true", "已知显式身份"], ["false", "未知身份"],
  ]);
  let appliedFilter = "{}";
  let catalogRequest: AbortController | undefined;
  const applyFilter = form(async () => {
    try {
      if (Boolean(subjectKind.value) !== Boolean(subjectId.value))
        throw new Error("主体类型与主体 ID 须一起填写");
      const filter: Record<string, unknown> = {};
      if (subjectKind.value)
        filter.subject = { kind: subjectKind.value, stable_id: subjectId.value };
      if (predicate.value) filter.predicate = predicate.value;
      if (conditions.value.trim()) filter.context = parseObject(conditions.value);
      if (identified.value) filter.identified = identified.value === "true";
      appliedFilter = JSON.stringify(filter);
      catalogRequest?.abort();
      items = [];
      after = null;
      list.replaceChildren();
      await load();
    } catch (e) {
      message(error, e);
    }
  },
    el("div", "form-grid",
      field("主体类型", subjectKind), field("主体 ID", subjectId),
      field("属性", predicate), field("完整适用条件", conditions),
      field("身份", identified),
    ),
    submit("应用目录筛选"),
    el("p", "muted",
      "精确匹配、区分大小写；条件比较完整键值集合。未知身份不推断主体。每次最多检查 100 个候选，空页仍可能有下一页。",
    ),
  );
  const direct = input("内容 UUID");
  const jobId = input("任务 UUID");
  let items: Asset[] = [],
    after: string | null = null;
  const render = () => {
    const filtered = kind === "memory" ? items : items.filter((a) =>
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
            after ? `本批暂无匹配的${kindLabel}` : `没有匹配的${kindLabel}`,
            after
              ? "仍有候选可检查，请加载下一页。只显示已发布且来源有效的匹配内容。"
              : "此处仅显示当前权限下已发布且来源有效的内容。",
          ),
    );
  };
  const more = button("加载下一页", () => load());
  const load = async () => {
    more.disabled = true;
    catalogRequest?.abort();
    const request = new AbortController();
    catalogRequest = request;
    error.replaceChildren();
    try {
      const page = await api.get<Page<Asset>>(
        `/v1/assets?kind=${kind}&limit=25${after ? "&after=" + after : ""}${kind === "memory" ? "&memory_filter=" + encodeURIComponent(appliedFilter) : ""}`,
        AbortSignal.any([ctx.signal, request.signal]),
      );
      if (ctx.signal.aborted || request.signal.aborted) return;
      items.push(...page.items);
      after = page.next_after;
      more.hidden = !after;
      render();
    } catch (e) {
      if (!request.signal.aborted) message(error, e);
    } finally {
      if (catalogRequest === request) more.disabled = false;
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
    panel(`已发布${kindLabel}`, kind === "memory" ? applyFilter : labelControl(search, "筛选已加载的知识"), list, more),
    panel("检索预览", retrieve, hits),
    panel(
      "通过标识打开",
      form(
        () => ctx.navigate("asset", direct.value.trim()),
        field("内容 ID", direct),
        submit("打开内容"),
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

function assetContent(asset: Asset, ctx: ViewContext) {
  const body = code(asset.content);
  if (!canWrite(ctx.role)) return panel("正文", body);

  const source = el("div");
  let sourceRequest: AbortController | undefined;
  const loadSource = async () => {
    const request = new AbortController();
    sourceRequest = request;
    const signal = AbortSignal.any([ctx.signal, request.signal]);
    const loading = el("p", "muted", "正在读取原始来源…");
    loading.setAttribute("role", "status");
    source.replaceChildren(loading);
    try {
      const event = await api.get(`/v1/events/${asset.source_event_id}`, signal);
      if (!signal.aborted) source.replaceChildren(code(event));
    } catch (e) {
      if (!signal.aborted) message(source, e);
    }
  };

  let selected = 0;
  const tabs = [
    { id: "asset-body", label: "正文", content: body },
    { id: "asset-source", label: "原始来源", content: source },
  ].map(({ id, label, content }, index) => {
    const tab = button(label, () => activate(index));
    const tabPanel = el("div", "", content);
    tab.id = `${id}-tab`;
    tab.setAttribute("role", "tab");
    tab.setAttribute("aria-selected", String(index === selected));
    tab.setAttribute("aria-controls", id);
    tab.tabIndex = index === selected ? 0 : -1;
    tabPanel.id = id;
    tabPanel.setAttribute("role", "tabpanel");
    tabPanel.setAttribute("aria-labelledby", tab.id);
    tabPanel.tabIndex = 0;
    tabPanel.hidden = index !== selected;
    return { tab, tabPanel };
  });
  const tablist = el("div", "actions asset-tabs", ...tabs.map(({ tab }) => tab));
  tablist.setAttribute("role", "tablist");
  tablist.setAttribute("aria-label", "内容与来源");

  const setTabStop = (index: number) => {
    tabs.forEach(({ tab }, i) => {
      tab.tabIndex = i === index ? 0 : -1;
    });
  };
  function activate(index: number) {
    selected = index;
    setTabStop(index);
    tabs.forEach(({ tab, tabPanel }, i) => {
      tab.setAttribute("aria-selected", String(i === index));
      tabPanel.hidden = i !== index;
    });
    tabs[index]?.tab.focus();
    sourceRequest?.abort();
    source.removeAttribute("role");
    source.replaceChildren();
    if (index === 1) void loadSource();
  }
  tabs.forEach(({ tab }, index) => {
    tab.addEventListener("keydown", (event) => {
      let next: number;
      if (event.key === "ArrowLeft") next = (index - 1 + tabs.length) % tabs.length;
      else if (event.key === "ArrowRight") next = (index + 1) % tabs.length;
      else if (event.key === "Home") next = 0;
      else if (event.key === "End") next = tabs.length - 1;
      else return;

      event.preventDefault();
      setTabStop(next);
      tabs[next]?.tab.focus();
    });
  });
  tablist.addEventListener("focusout", (event) => {
    // Re-enter the tablist on the active tab after browsing without activating.
    if (!tablist.contains(event.relatedTarget as Node | null)) setTabStop(selected);
  });
  return panel("内容与来源", tablist, ...tabs.map(({ tabPanel }) => tabPanel));
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
  const kindLabel = asset.kind === "memory" ? "记忆" : "知识";
  const error = el("div");
  const related = panel(
    `关联 Run · v${asset.version}`,
    el("p", "muted", "仅当前逻辑 key 的已记录引用；不代表完整采集、收到或使用。"),
  );
  const relatedResult = el("div");
  let relatedRequest = 0;
  const loadRelated = async (after?: string) => {
    const request = ++relatedRequest;
    relatedResult.replaceChildren(el("p", "muted", "正在读取关联 Run…"));
    try {
      const page = await api.get<Page<{ run_id: string; created_at_ms: number }>>(
        `/v1/assets/${id}/runs?version=${asset.version}&limit=25${after ? "&after=" + encodeURIComponent(after) : ""}`,
        ctx.signal,
      );
      if (ctx.signal.aborted || request !== relatedRequest) return;
      relatedResult.replaceChildren();
      if (!page.items.length)
        relatedResult.append(empty("本页没有可读取的已记录引用", "未记录不等于未使用；可能没有关联或引用已不可读。"));
      for (const run of page.items)
        relatedResult.append(button(`打开 Run ${run.run_id}`, () => ctx.navigate("runs", run.run_id), "link"));
      if (page.next_after)
        relatedResult.append(button("下一页关联 Run", () => loadRelated(page.next_after!)));
    } catch (e) {
      if (ctx.signal.aborted || request !== relatedRequest) return;
      if (e instanceof ApiError && [403, 404].includes(e.status)) {
        root.replaceChildren(empty("内容不可读取", `${kindLabel}或来源已不可读，请重新检查权限和来源状态。`));
        return;
      }
      message(relatedResult, e);
      relatedResult.append(button("重试关联 Run", () => loadRelated(after)));
    }
  };
  related.append(button("读取已记录引用", () => loadRelated()), relatedResult);
  const oldVersion = input(
    "对比版本",
    String(Math.max(1, asset.version - 1)),
    "number",
  );
  oldVersion.min = "1";
  const historyVersion = input("版本号", String(asset.version), "number");
  historyVersion.min = "1";
  const comparisonStatus = el("p", "muted");
  comparisonStatus.setAttribute("role", "status");
  comparisonStatus.setAttribute("aria-atomic", "true");
  const comparisonResult = el("div");
  let compareRequest: AbortController | undefined;
  const compare = async () => {
    compareRequest?.abort();
    const request = new AbortController();
    compareRequest = request;
    const signal = AbortSignal.any([ctx.signal, request.signal]);
    comparisonResult.removeAttribute("role");
    comparisonResult.replaceChildren();
    const loading = "正在比较正文…";
    if (comparisonStatus.textContent !== loading) comparisonStatus.textContent = loading;
    try {
      const old = await api.get<Asset>(
        `/v1/assets/${id}?version=${Number(oldVersion.value)}`,
        signal,
      );
      if (signal.aborted) return;
      const delta = textDelta(old.content ?? "", asset.content ?? "");
      comparisonResult.replaceChildren(
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
      comparisonStatus.textContent = `正文比较完成，v${old.version} 到 v${asset.version}。`;
    } catch (e) {
      if (signal.aborted) return;
      comparisonStatus.textContent = "";
      message(comparisonResult, e);
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
        empty("内容已阻断", `${kindLabel}或来源已撤回，后续访问以服务端状态为准。`),
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
  const versionStatus = status(asset.semantic_status ?? asset.normalization_status);
  versionStatus.classList.add("asset-version-status");
  root.append(
    heading(
      asset.title || `${kindLabel}详情`,
      `${kindLabel} · v${asset.version} · ${id}`,
      button("返回列表", () => ctx.navigate(asset.kind)),
      canWrite(ctx.role) ? button("更新内容", update, "primary") : null,
    ),
    panel(
      "版本与身份",
      el(
        "div",
        "actions",
        versionStatus,
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
    assetContent(asset, ctx),
    panel(
      "版本比较",
      el(
        "div",
        "actions asset-comparison-controls",
        field("对比版本", oldVersion),
        button("比较正文", compare),
      ),
      el("div", "asset-comparison", comparisonStatus, comparisonResult),
    ),
    panel(
      "来源引用",
      el("p", "mono", asset.source_event_id),
      el(
        "p",
        "muted",
        `${kindLabel}版本与来源版本分别保留，原始来源需要独立读取权限。`,
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
              `删除${kindLabel}`,
              () => mutate(`/v1/assets/${id}`, "DELETE"),
              "danger",
            )
          : null,
      ),
    ),
    related,
    error,
  );
}
