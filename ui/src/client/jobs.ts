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
  let latest: AbortController | undefined;
  let observation: Promise<Job | undefined>;
  let polling = false, changing = false;
  const refresh = () => {
    const read = new AbortController();
    latest?.abort();
    latest = read;
    observation = observe(read);
    return observation;
  };
  const observe = async (read: AbortController): Promise<Job | undefined> => {
    try {
      const job = await api.get<Job>(
        `/v1/jobs/${id}`,
        AbortSignal.any([ctx.signal, read.signal]),
      );
      if (ctx.signal.aborted) return;
      // A stale poll must neither repaint the status nor decide when to stop.
      if (latest !== read) return observation;
      state.removeAttribute("role");
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
          button("查看内容 →", () => ctx.navigate("asset", asset), "link"),
        );
      if (asset && ["failed", "cancelled", "superseded"].includes(job.state)) {
        const current = el("div");
        state.append(
          el(
            "p",
            "muted",
            "此前发布的版本可能仍可读取。",
          ),
          button("读取当前版本", async () => {
            try {
              const latest = await api.get<Asset>(`/v1/assets/${asset}`, ctx.signal);
              current.replaceChildren(
                el("p", "", `当前已发布 v${latest.version} · ${latest.title}`),
                button(
                  "查看当前版本 →",
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
      if (["failed", "cancelled"].includes(job.state))
        state.append(button("重试任务", () => action("retry")));
      if (!terminal.has(job.state))
        state.append(button("取消任务", () => action("cancel")));
      return job;
    } catch (error) {
      if (ctx.signal.aborted) return;
      if (latest !== read) return observation;
      message(state, error);
    }
  };
  const poll = async (observed?: Job) => {
    if (polling) return;
    polling = true;
    try {
      for (let attempts = 0; attempts < 80 && !ctx.signal.aborted; attempts++) {
        const job = observed ?? await refresh();
        if (!job || terminal.has(job.state) || ctx.signal.aborted) return;
        observed = undefined;
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
    } finally {
      polling = false;
    }
  };
  const action = async (name: "retry" | "cancel") => {
    if (changing || ctx.signal.aborted) return;
    changing = true;
    try {
      await api.post(`/v1/jobs/${id}/${name}`, {}, ctx.signal, crypto.randomUUID());
      if (ctx.signal.aborted) return;
      const job = await refresh();
      // Retry starts a new generation after the original terminal state stopped polling.
      if (name === "retry" && job && !terminal.has(job.state)) void poll(job);
    } catch (error) {
      if (!ctx.signal.aborted) message(state, error);
    } finally {
      changing = false;
    }
  };
  target.replaceChildren(
    el("p", "muted", "任务已受理。关闭页面不会取消后台任务。"),
    el("p", "mono", id),
    state,
    button("刷新状态", () => refresh().then(() => {})),
  );
  await poll();
}
