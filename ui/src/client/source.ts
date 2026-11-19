import { api } from "./api.js";
import {
  button,
  code,
  el,
  message,
  panel,
  type ViewContext,
} from "./components.js";

// Keep a UTF-8 BOM as text. The parser includes its bytes in source ranges.
const sourceDecoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

/** Reject invalid UTF-8 boundaries instead of highlighting a different character. */
export function byteRange(
  text: string,
  start: number,
  end: number,
): [string, string, string] {
  const bytes = new TextEncoder().encode(text);
  if (
    !Number.isInteger(start) ||
    !Number.isInteger(end) ||
    start < 0 ||
    end < start ||
    end > bytes.length
  )
    throw new Error("引用区间超出原文");
  return [
    sourceDecoder.decode(bytes.slice(0, start)),
    sourceDecoder.decode(bytes.slice(start, end)),
    sourceDecoder.decode(bytes.slice(end)),
  ];
}

/** A linear, exact block diff: shared complete lines surround one changed region. */
export function textDelta(before: string, after: string) {
  const a = before.split(/(?<=\n)/),
    b = after.split(/(?<=\n)/);
  let start = 0,
    end = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  while (
    end < a.length - start &&
    end < b.length - start &&
    a[a.length - 1 - end] === b[b.length - 1 - end]
  )
    end++;
  return {
    prefix: a.slice(0, start).join(""),
    removed: a.slice(start, a.length - end).join(""),
    added: b.slice(start, b.length - end).join(""),
    suffix: a.slice(a.length - end).join(""),
  };
}

export async function sourcePreview(
  target: HTMLElement,
  sourceId: string,
  locator: Record<string, unknown>,
  ctx: ViewContext,
  knowledgeQuote?: string,
) {
  try {
    const source = await api.get<{
      kind?: string;
      content: string;
      file_id?: string | null;
    }>(`/v1/events/${sourceId}`, ctx.signal);
    if (ctx.signal.aborted) return;
    const content = el("pre", "code");
    const page = locator.page ?? locator.page_number;
    const readFile = (id: string) =>
      api.request<Blob>(`/v1/files/${id}`, { signal: ctx.signal, blob: true });
    let unverified = false;
    if (
      !page &&
      knowledgeQuote !== undefined &&
      source.kind === "knowledge" &&
      locator.parser === "source-ranges-v1"
    ) {
      const blob = source.file_id ? await readFile(source.file_id) : undefined;
      const bytes = await blob?.arrayBuffer();
      if (ctx.signal.aborted) return;
      const { byte_start: start, byte_end: end } = locator;
      try {
        if (typeof start !== "number" || typeof end !== "number")
          throw new Error("引用区间无效");
        const original = bytes ? sourceDecoder.decode(bytes) : source.content;
        const [before, quote, after] = byteRange(original, start, end);
        if (!quote || quote !== knowledgeQuote) throw new Error("引用与原文不匹配");
        content.append(before, el("mark", "", quote), after);
      } catch {
        unverified = true;
        content.textContent = source.content;
      }
    } else if (
      !page &&
      (locator.byte_basis === "source_utf8" || locator.source_span === true)
    ) {
      const [before, quote, after] = byteRange(
        source.content,
        Number(locator.byte_start),
        Number(locator.byte_end),
      );
      content.append(before, el("mark", "", quote), after);
    } else content.textContent = source.content;
    target.replaceChildren(
      panel(
        "原始来源",
        code(locator),
        page ? null : content,
        unverified ? el("p", "muted", "无法核对原文字节区间，未高亮引用。") : null,
      ),
    );
    if (source.file_id)
      target.append(
        button("下载原始文件", async () => {
          try {
            const blob = await readFile(source.file_id!);
            const url = URL.createObjectURL(blob),
              link = el("a");
            link.href = url;
            link.download = source.file_id!;
            link.click();
            setTimeout(() => URL.revokeObjectURL(url), 10_000);
          } catch (e) {
            message(target, e);
          }
        }),
      );
    if (page)
      target.append(
        el(
          "p",
          "muted",
          `PDF 第 ${String(page)} 页；locator 的字节区间基于页内提取文本，不套用到原始 PDF 字节。当前来源接口未提供可核验的页内文本，无法显示页内原文或高亮；原始文件下载与页内文本是不同内容。`,
        ),
      );
  } catch (e) {
    if (!ctx.signal.aborted) message(target, e);
  }
}

interface RunPdfPreview {
  asset_version: number;
  page: number | null;
  span: { before: string; highlight: string; after: string } | null;
}

export async function runPdfPreview(target: HTMLElement, path: string, ctx: ViewContext) {
  target.replaceChildren();
  try {
    const preview = await api.get<RunPdfPreview>(path, ctx.signal);
    if (ctx.signal.aborted || !target.isConnected) return;
    const page = preview.page;
    const span = preview.span;
    const open = button(page ? `打开原件第 ${page} 页` : "打开原始 PDF", async () => {
      open.disabled = true;
      try {
        // Reauthorize the archived citation on every open. Never retain a
        // downloadable URL in the view after the request.
        const blob = await api.request<Blob>(path + "&original=true", {
          signal: ctx.signal,
          blob: true,
        });
        if (ctx.signal.aborted || !target.isConnected) return;
        const url = URL.createObjectURL(blob.slice(0, blob.size, "application/pdf"));
        const close = () => URL.revokeObjectURL(url);
        ctx.signal.addEventListener("abort", close, { once: true });
        const link = el("a");
        link.href = page ? `${url}#page=${page}` : url;
        link.target = "_blank";
        link.rel = "noopener noreferrer";
        link.click();
        setTimeout(() => {
          close();
          ctx.signal.removeEventListener("abort", close);
        }, 60_000);
      } catch (e) {
        if (ctx.signal.aborted || !target.isConnected) return;
        target.replaceChildren();
        message(target, e);
      } finally {
        open.disabled = false;
      }
    });
    target.append(panel(
      page ? `v${preview.asset_version} · PDF 第 ${page} 页` : `v${preview.asset_version} · PDF`,
      span
        ? el("pre", "code", span.before, el("mark", "", span.highlight), span.after)
        : el("p", "muted", "缺少可核验的页内定位，未高亮片段。"),
      span ? el("p", "muted", "高亮位于已核对的页内提取文本。") : null,
      open,
    ));
  } catch (e) {
    if (!ctx.signal.aborted && target.isConnected) message(target, e);
  }
}
