import { api } from "./api.js";
import {
  badge,
  button,
  code,
  el,
  message,
  status,
  type ViewContext,
} from "./components.js";
import type { Asset, Job, Receipt } from "./types.js";
const terminal = new Set(["completed", "failed", "cancelled", "superseded"]);

export async function trackJob(
  receipt: Receipt,
  target: HTMLElement,
  ctx: ViewContext,
) {
  if (!receipt.job_id) {
    target.replaceChildren(status(receipt.state ?? "已记录"), code(receipt));
    return;
  }
  const id = receipt.job_id;
  const state = el("div");
  const refresh = async () => {
    try {
      const job = await api.get<Job>(`/v1/jobs/${id}`, ctx.signal);
      state.replaceChildren(
        status(job.state),
        job.outcome ? badge(job.outcome) : "",
        job.error_code ? el("p", "error", job.error_code) : "",
      );
      const asset =
        receipt.asset_id ??
        job.result?.asset_id ??
        job.result?.memories?.[0]?.asset_id;
      if (asset && job.state === "completed")
        state.append(
          button("查看资产 →", () => ctx.navigate("asset", asset), "link"),
        );
      if (asset && ["failed", "cancelled", "superseded"].includes(job.state)) {
        const current = el("div");
        state.append(
          el(
            "p",
            "muted",
            "任务状态不代表当前资产版本。重新读取以确认仍可访问的已发布内容。",
          ),
          button("读取当前资产", async () => {
            try {
              const latest = await api.get<Asset>(`/v1/assets/${asset}`, ctx.signal);
              current.replaceChildren(
                el("p", "", `当前已发布 v${latest.version} · ${latest.title}`),
                button(
                  "查看当前资产 →",
                  () => ctx.navigate("asset", asset),
                  "link",
                ),
              );
            } catch (error) {
              message(current, error);
            }
          }),
          current,
        );
      }
      const action = async (name: string) => {
        try {
          await api.post(
            `/v1/jobs/${id}/${name}`,
            {},
            ctx.signal,
            crypto.randomUUID(),
          );
          await refresh();
        } catch (error) {
          message(state, error);
        }
      };
      if (["failed", "cancelled"].includes(job.state))
        state.append(button("重试任务", () => action("retry")));
      if (!terminal.has(job.state))
        state.append(button("取消任务", () => action("cancel")));
      return job;
    } catch (error) {
      message(state, error);
      return undefined;
    }
  };
  target.replaceChildren(
    el("p", "muted", "任务已受理。关闭页面不会取消后台任务。"),
    el("p", "mono", id),
    state,
    button("刷新状态", () => refresh().then(() => {})),
  );
  for (let attempts = 0; attempts < 80 && !ctx.signal.aborted; attempts++) {
    const job = await refresh();
    if (!job || terminal.has(job.state)) return;
    await new Promise<void>((resolve) => {
      const done = () => {
        clearTimeout(timer);
        ctx.signal.removeEventListener("abort", done);
        resolve();
      };
      const timer = setTimeout(done, 1500);
      ctx.signal.addEventListener("abort", done, { once: true });
    });
  }
}
