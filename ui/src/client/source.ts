import { api } from "./api.js";
import {
  button,
  code,
  el,
  message,
  panel,
  type ViewContext,
} from "./components.js";

/** Reject invalid UTF-8 boundaries instead of highlighting a different character. */
export function byteRange(
  text: string,
  start: number,
  end: number,
): [string, string, string] {
  const bytes = new TextEncoder().encode(text),
    decode = new TextDecoder("utf-8", { fatal: true });
  if (
    !Number.isInteger(start) ||
    !Number.isInteger(end) ||
    start < 0 ||
    end < start ||
    end > bytes.length
  )
    throw new Error("引用区间超出原文");
  return [
    decode.decode(bytes.slice(0, start)),
    decode.decode(bytes.slice(start, end)),
    decode.decode(bytes.slice(end)),
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
) {
  try {
    const source = await api.get<{ content: string; file_id?: string }>(
      `/v1/events/${sourceId}`,
      ctx.signal,
    );
    if (ctx.signal.aborted) return;
    const content = el("pre", "code");
    const page = locator.page ?? locator.page_number;
    if (
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
    target.replaceChildren(panel("原始来源", code(locator), content));
    if (source.file_id)
      target.append(
        button("下载原始文件", async () => {
          try {
            const blob = await api.request<Blob>(
              `/v1/files/${source.file_id}`,
              { signal: ctx.signal, blob: true },
            );
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
          `PDF 第 ${String(page)} 页；locator 的字节区间基于页内提取文本，不套用到原始 PDF 字节。`,
        ),
      );
  } catch (e) {
    message(target, e);
  }
}
