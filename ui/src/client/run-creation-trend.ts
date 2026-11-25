import { api } from "./api.js";
import { button, el, field, message, panel, select, table } from "./components.js";

interface RunCreationBucket {
  start_ms: number;
  end_ms: number;
  created_runs: number;
}

interface RunCreationOverview extends RunCreationBucket {
  scope: "workspace" | "current_logical_key";
  series: RunCreationBucket[];
}

function utc(ms: number) {
  return new Date(ms).toISOString().replace("T", " ").replace("Z", "");
}

export function runCreationTrend(signal: AbortSignal) {
  const range = select([
    ["24h", "最近 24 小时"],
    ["7d", "最近 7 天"],
  ]);
  const result = el("div");
  const status = el("p");
  status.setAttribute("role", "status");
  const retry = button("重试读取创建量", () => {
    if (retry.getAttribute("aria-disabled") !== "true") return load();
  });
  retry.hidden = true;
  let pending: AbortController | undefined;
  const load = async () => {
    pending?.abort();
    const request = new AbortController();
    pending = request;
    const requestSignal = AbortSignal.any([signal, request.signal]);
    // Keep the retry control focusable while its request is in flight.
    retry.setAttribute("aria-disabled", "true");
    status.textContent = "正在读取创建量…";
    result.replaceChildren();
    try {
      const data = await api.get<RunCreationOverview>(
        `/v1/runs/overview?window=${encodeURIComponent(range.value)}`,
        requestSignal,
      );
      if (requestSignal.aborted) return;
      const scale = Math.max(1, ...data.series.map((bucket) => bucket.created_runs));
      status.textContent = `新建运行 ${data.created_runs.toLocaleString("zh-CN")} 条`;
      result.append(
        el("p", "muted", data.scope === "workspace" ? "统计当前工作区的全部新建运行。" : "统计当前 Key 的新建运行。"),
        el("p", "muted", `UTC ${utc(data.start_ms)}（含）至 ${utc(data.end_ms)}（不含）`),
        table(
          ["桶起点（UTC，含）", "桶终点（UTC，不含）", "创建数", "相对趋势"],
          data.series.map((bucket) => {
            const bar = el("meter");
            bar.max = scale;
            bar.value = bucket.created_runs;
            bar.setAttribute("aria-hidden", "true");
            return [utc(bucket.start_ms), utc(bucket.end_ms), bucket.created_runs, bar];
          }),
        ),
      );
      if (document.activeElement === retry) range.focus();
      retry.hidden = true;
    } catch (error) {
      if (requestSignal.aborted) return;
      status.textContent = "创建量暂不可用";
      const failure = el("div");
      message(failure, error);
      result.append(failure);
      retry.hidden = false;
    } finally {
      if (!requestSignal.aborted) retry.removeAttribute("aria-disabled");
    }
  };
  range.addEventListener("change", () => void load());
  void load();
  return panel(
    "运行创建趋势",
    field("统计范围", range),
    status,
    el("div", "", result, retry),
  );
}
