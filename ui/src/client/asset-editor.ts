import { api, ApiError } from "./api.js";
import {
  button,
  el,
  empty,
  field,
  form,
  input,
  message,
  parseObject,
  select,
  submit,
  type ViewContext,
} from "./components.js";
import type { Asset, Receipt } from "./types.js";
import { trackJob } from "./jobs.js";

function dialog(title: string, ctx: ViewContext) {
  const modal = el("dialog");
  const lifetime = new AbortController();
  const body = el("div");
  modal.append(
    el(
      "div",
      "dialog-head",
      el("h2", "", title),
      button("关闭", () => modal.close()),
    ),
    body,
  );
  modal.addEventListener(
    "close",
    () => {
      lifetime.abort();
      modal.remove();
    },
    { once: true },
  );
  document.body.append(modal);
  modal.showModal();
  return {
    body,
    ctx: { ...ctx, signal: AbortSignal.any([ctx.signal, lifetime.signal]) },
  };
}

/** Keep the same receipt identity when a user retries an unchanged form. */
function operation() {
  let previous = "",
    key = "";
  return (value: unknown) => {
    const fingerprint = JSON.stringify(value);
    if (fingerprint !== previous) {
      previous = fingerprint;
      key = crypto.randomUUID();
    }
    return key;
  };
}

export function editor(kind: Asset["kind"], ctx: ViewContext, asset?: Asset) {
  const view = dialog(
    `${asset ? "更新" : "新建"}${kind === "memory" ? "记忆" : "知识"}`,
    ctx,
  );
  ctx = view.ctx;
  const { body } = view;
  const content = el("textarea");
  content.rows = 7;
  content.value = asset?.content ?? "";
  content.required = true;
  const result = el("div", "operation-result");
  result.setAttribute("aria-live", "polite");
  const save = submit(asset ? "提交新版本" : "提交发布");
  const keyFor = operation(),
    uploadKeyFor = operation();
  const fields: HTMLElement[] = [];
  const name = input("文档标题", asset?.title ?? "");
  name.required = kind === "knowledge";
  const format = select([
    ["markdown", "Markdown"],
    ["text", "Text"],
    ["pdf", "PDF"],
  ]);
  const file = input("", "", "file");
  file.accept = ".txt,.md,.markdown,.pdf";
  const identity = asset?.identity;
  const subject = select(
    ["user", "agent", "project", "service", "team"].map((v) => [v, v]),
    identity?.subject.kind ?? "service",
  );
  const stable = input("例如 Atlas", identity?.subject.stable_id);
  const predicate = input("例如 database.engine", identity?.predicate);
  const conditions = input(
    '{"env":"production"}',
    JSON.stringify(identity?.context ?? {}),
  );
  const semantic = select(
    [
      ["identified", "普通事实"],
      ["semantic", "语义事实（已登记属性）"],
    ],
    asset?.semantic_status ? "semantic" : "identified",
  );
  const verb = select(
    [
      ["assert", "确认事实 assert"],
      ["update", "更新单值 update"],
      ["supplement", "补充多值 supplement"],
      ["negate", "否定 negate"],
      ["propose", "提议 propose"],
      ["ambiguous", "待澄清 ambiguous"],
    ],
    asset ? "update" : "assert",
  );
  const value = input("语义值，例如 postgresql");
  if (kind === "memory") {
    if (asset && !identity) {
      body.append(
        empty(
          "此记忆尚未绑定身份",
          "请创建明确的新身份；不能通过编辑替旧资产改绑身份。",
        ),
      );
      return;
    }
    for (const control of [subject, stable, predicate, conditions])
      control.disabled = !!identity;
    fields.push(
      field("主体类型", subject),
      field("主体稳定 ID", stable),
      field("属性", predicate),
      field("适用条件（字符串键值）", conditions),
      field("事实类型", semantic),
      field("语义操作", verb),
      field("语义值（语义事实必填）", value),
    );
  } else {
    fields.push(
      field("文档标题", name),
      field("格式", format),
      field("上传文件（选填，替代正文）", file),
    );
    file.addEventListener("change", () => {
      content.required = !file.files?.length;
      content.disabled = !!file.files?.length;
      const name = file.files?.[0]?.name.toLowerCase();
      if (name)
        format.value = name.endsWith(".pdf")
          ? "pdf"
          : /\.(md|markdown)$/.test(name)
            ? "markdown"
            : "text";
    });
  }
  const formNode = form(
    async () => {
      save.disabled = true;
      try {
        let payload: Record<string, unknown>, path: string;
        if (kind === "memory") {
          if (!stable.value.trim() || !predicate.value.trim())
            throw new Error("请填写主体 ID 和属性");
          payload = {
            identity: identity ?? {
              subject: { kind: subject.value, stable_id: stable.value.trim() },
              predicate: predicate.value.trim(),
              context: parseObject(conditions.value),
            },
            content: content.value,
            expected_version: asset?.version ?? 0,
          };
          path = `/v1/memories/${semantic.value}`;
          if (semantic.value === "semantic") {
            if (!value.value.trim()) throw new Error("请填写语义值");
            const size = new TextEncoder().encode(content.value).length;
            if (size > 8192)
              throw new Error("此表单的引用正文须在 8,192 UTF-8 字节以内");
            payload.operation = verb.value;
            payload.statements = [
              {
                value: value.value.trim(),
                quote: content.value,
                byte_start: 0,
                byte_end: size,
              },
            ];
          }
        } else {
          const selected = file.files?.[0];
          if (format.value === "pdf" && !selected)
            throw new Error("PDF 需上传原始文件");
          payload = {
            title: name.value,
            format: format.value,
            asset_id: asset?.asset_id ?? null,
            expected_version: asset?.version ?? null,
          };
          if (selected) {
            if (selected.size > 10 * 1024 * 1024)
              throw new Error("文件不能超过 10 MiB");
            const hash = Array.from(
              new Uint8Array(
                await crypto.subtle.digest(
                  "SHA-256",
                  await selected.arrayBuffer(),
                ),
              ),
              (byte) => byte.toString(16).padStart(2, "0"),
            ).join("");
            const fileKey = uploadKeyFor({
              name: selected.name,
              format: format.value,
              hash,
            });
            const upload = await api.post<{ file_id: string }>(
              `/v1/files?name=${encodeURIComponent(selected.name)}&format=${format.value}`,
              selected,
              ctx.signal,
              fileKey + "-upload",
            );
            payload.file_id = upload.file_id;
          } else payload.content = content.value;
          path = "/v1/knowledge";
        }
        const receipt = await api.post<Receipt>(
          path,
          payload,
          ctx.signal,
          keyFor(payload),
        );
        formNode.hidden = true;
        await trackJob(receipt, result, ctx);
      } catch (error) {
        message(result, error);
        if (
          kind === "knowledge" &&
          asset &&
          error instanceof ApiError &&
          error.status === 409
        ) {
          const current = el("div");
          result.append(
            el(
              "p",
              "muted",
              "版本冲突。草稿和提交版本保持不变；请比较当前版本后重新打开更新表单。",
            ),
            button("读取当前版本", async () => {
              try {
                const latest = await api.get<Asset>(
                  `/v1/assets/${asset.asset_id}`,
                  ctx.signal,
                );
                current.replaceChildren(
                  el("p", "", `当前已发布 v${latest.version} · ${latest.title}`),
                  el("pre", "", latest.content),
                );
              } catch (failure) {
                message(current, failure);
              }
            }),
            current,
          );
        }
      } finally {
        save.disabled = false;
      }
    },
    ...fields,
    field(kind === "memory" ? "事实正文 / 原始引用" : "正文", content),
    asset
      ? el(
          "p",
          "muted",
          `基于 v${asset.version} 提交；版本竞争时重新读取后再提交。`,
        )
      : el(
          "p",
          "muted",
          "提交后由 Origence 异步发布，来源与权限仍由服务端核验。",
        ),
    save,
  );
  body.append(formNode, result);
}

export function capture(ctx: ViewContext) {
  const view = dialog("从文本提取记忆", ctx);
  ctx = view.ctx;
  const { body } = view;
  const content = el("textarea");
  content.rows = 7;
  content.required = true;
  const targets = el("textarea");
  targets.rows = 4;
  targets.value = "[]";
  const result = el("div");
  const save = submit("提交提取");
  const keyFor = operation();
  body.append(
    form(
      async () => {
        save.disabled = true;
        try {
          const payload = {
            content: content.value,
            targets: JSON.parse(targets.value) as unknown,
          };
          if (!Array.isArray(payload.targets))
            throw new Error("目标须为 JSON 数组");
          await trackJob(
            await api.post<Receipt>(
              "/v1/captures/semantic",
              payload,
              ctx.signal,
              keyFor(payload),
            ),
            result,
            ctx,
          );
        } catch (error) {
          message(result, error);
        } finally {
          save.disabled = false;
        }
      },
      el(
        "p",
        "muted",
        "仅提取本次显式提交的文本；空目标仅产生未归一化证据。需要服务端配置提取模型。",
      ),
      field("原始文本", content),
      field(
        "允许的目标身份（含 identity、expected_version；可留 []）",
        targets,
      ),
      save,
    ),
    result,
  );
}
