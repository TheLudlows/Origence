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
  metering?: {
    duration_p95_ms: number | null;
    duration_runs: number;
    agent_tokens: string | null;
    token_runs: number;
    costs: {
      basis: "estimated" | "billed";
      currency: string;
      micros: string;
      runs: number;
    }[];
  };
}

const exactTime = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric", month: "2-digit", day: "2-digit",
  hour: "2-digit", minute: "2-digit", second: "2-digit", fractionalSecondDigits: 3,
  hourCycle: "h23", timeZoneName: "shortOffset",
});
const localZone = exactTime.resolvedOptions().timeZone;

function creationChart(buckets: RunCreationBucket[], daily: boolean) {
  if (!buckets.length) return el("p", "muted", "暂无时段数据。");

  const peak = Math.max(...buckets.map((bucket) => bucket.created_runs));
  const step = Math.max(1, Math.ceil(peak / 4));
  const top = Math.max(step, Math.ceil(peak / step) * step);
  const scale = el("div", "run-chart-scale");
  const guides = el("div", "run-chart-guides");
  const ticks = el("div", "run-chart-ticks");
  for (const visual of [scale, guides, ticks]) visual.setAttribute("aria-hidden", "true");
  for (let value = 0; value <= top; value += step) {
    const position = `${value / top * 100}%`;
    const label = el("span", "", value.toLocaleString("zh-CN"));
    const line = el("span", value === 0 ? "run-chart-baseline" : "");
    label.style.bottom = position;
    line.style.bottom = position;
    scale.append(label);
    guides.append(line);
  }

  const tickTime = new Intl.DateTimeFormat("zh-CN", daily
    ? { month: "numeric", day: "numeric" }
    : { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  const readout = el("p", "run-chart-readout", "悬停、点击或用方向键查看各时段的创建量。");
  const plot = el("div", "run-chart-plot", guides);
  plot.setAttribute("role", "group");
  plot.setAttribute("aria-label", "各时段新建 Run 数，使用左右方向键切换时段");
  const bars = buckets.map((bucket, index) => {
    const label = `${exactTime.format(bucket.start_ms)}（含）至 ${exactTime.format(bucket.end_ms)}（不含），新建 ${bucket.created_runs.toLocaleString("zh-CN")} 个 Run`;
    const show = () => { readout.textContent = label; };
    const mark = el("span", bucket.created_runs === 0 ? "run-chart-mark is-zero" : "run-chart-mark");
    mark.style.height = `${bucket.created_runs / top * 100}%`;
    const bar = button("", show, "run-chart-bar");
    bar.append(mark);
    bar.setAttribute("aria-label", label);
    bar.tabIndex = index === 0 ? 0 : -1;
    bar.addEventListener("pointerenter", show);
    bar.addEventListener("focus", () => {
      plot.querySelector<HTMLElement>('[tabindex="0"]')?.setAttribute("tabindex", "-1");
      bar.tabIndex = 0;
      show();
    });
    if (index % (daily ? 2 : 6) === 0 || index === buckets.length - 1) {
      const tick = el("span", "", tickTime.format(bucket.start_ms));
      tick.style.gridColumn = String(index + 1);
      ticks.append(tick);
    }
    return bar;
  });
  plot.append(...bars);
  plot.addEventListener("keydown", (event) => {
    const current = bars.findIndex((bar) => bar === event.target);
    if (current === -1) return;
    let next: number;
    switch (event.key) {
      case "ArrowLeft": next = Math.max(0, current - 1); break;
      case "ArrowRight": next = Math.min(bars.length - 1, current + 1); break;
      case "Home": next = 0; break;
      case "End": next = bars.length - 1; break;
      default: return;
    }
    event.preventDefault();
    bars[next]?.focus();
  });
  const chart = el("figure", "run-chart",
    el("figcaption", "run-chart-label", "新建 Run 数"),
    el("div", "run-chart-grid", scale, plot, ticks),
    el("p", "run-chart-time", `时段起点（本地时间 · ${localZone}）`),
    readout,
  );
  chart.style.setProperty("--run-chart-columns", String(bars.length));
  return chart;
}

function creationDetails(data: RunCreationOverview) {
  return el("details", "run-creation-buckets",
    el("summary", "", "查看明细"),
    el("p", "muted", `${exactTime.format(data.start_ms)}（含）至 ${exactTime.format(data.end_ms)}（不含）`),
    table(["时段开始（含）", "时段结束（不含）", "新建 Run 数"], data.series.map((bucket) => [
      exactTime.format(bucket.start_ms), exactTime.format(bucket.end_ms), bucket.created_runs,
    ])),
  );
}

function amount(currency: string, micros: string) {
  const units = BigInt(micros);
  const whole = (units / 1_000_000n).toLocaleString("zh-CN");
  const fraction = (units % 1_000_000n).toString().padStart(6, "0");
  return `${currency} ${whole}.${fraction}`;
}

function metering(data: RunCreationOverview) {
  const metrics = data.metering;
  if (!metrics) return panel("运行计量", el("p", "muted", "当前服务未返回计量数据。"));
  const duration = metrics.duration_p95_ms === null
    ? "未知" : `${metrics.duration_p95_ms.toLocaleString("zh-CN")} ms`;
  const tokens = metrics.agent_tokens === null
    ? "未知" : BigInt(metrics.agent_tokens).toLocaleString("zh-CN");
  const costs = metrics.costs.map(({ basis, currency, micros, runs }) => [
    basis === "billed" ? "账单金额" : "费用估算",
    amount(currency, micros),
    runs,
  ]);
  return panel(
    "运行计量",
    el("p", "muted", `统计窗口内创建的 ${data.created_runs.toLocaleString("zh-CN")} 个 Run。只汇总已上报的整轮计量，费用按币种和来源分别列出。`),
    table(["指标", "已上报数值", "覆盖 Run 数"], [
      ["端到端耗时 P95", duration, metrics.duration_runs],
      ["Agent Token 总量", tokens, metrics.token_runs],
      ...costs,
      ...(costs.length ? [] : [["费用", "未知", 0]]),
    ]),
  );
}

export function runCreationTrend(signal: AbortSignal) {
  const range = select([
    ["24h", "最近 24 小时"],
    ["7d", "最近 7 天"],
  ]);
  const result = el("div", "run-creation-result");
  const status = el("p", "run-trend-status");
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
      status.textContent = `新建运行 ${data.created_runs.toLocaleString("zh-CN")} 条`;
      result.append(
        el("div", "run-trend-series",
          creationChart(data.series, range.value === "7d"),
          el("p", "muted", data.scope === "workspace" ? "统计当前工作区的全部新建运行。" : "统计当前 Key 的新建运行。"),
          creationDetails(data),
        ),
        metering(data),
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
  return el("section", "panel run-trend",
    el("div", "run-trend-heading",
      el("div", "", el("h2", "", "运行创建趋势"), status),
      field("统计范围", range),
    ),
    el("div", "", result, retry),
  );
}
