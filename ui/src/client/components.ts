export type Child = Node | string | number | null | undefined | false;
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls = "",
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  element.className = cls;
  for (const child of children)
    if (child !== undefined && child !== null && child !== false)
      element.append(child instanceof Node ? child : String(child));
  return element;
}
export function button(
  text: string,
  run: () => void | Promise<void>,
  cls = "",
) {
  const control = el("button", cls, text);
  control.type = "button";
  control.addEventListener("click", () => void run());
  return control;
}
export function input(placeholder: string, value = "", type = "text") {
  const control = el("input");
  control.type = type;
  control.placeholder = placeholder;
  control.value = value;
  return control;
}
export function select(values: [string, string][], value?: string) {
  const control = el("select");
  for (const [id, label] of values) {
    const option = el("option", "", label);
    option.value = id;
    control.append(option);
  }
  if (value) control.value = value;
  return control;
}
export function field(label: string, control: HTMLElement) {
  return el("label", "field", el("span", "", label), control);
}
export function panel(title: string, ...children: Child[]) {
  return el("section", "panel", el("h2", "", title), ...children);
}
export function heading(title: string, subtitle: Child, ...actions: Child[]) {
  return el(
    "header",
    "page-heading",
    el("div", "", el("h1", "", title), el("p", "muted", subtitle)),
    el("div", "actions", ...actions),
  );
}
export function empty(title: string, description: string) {
  return el("div", "empty", el("h3", "", title), el("p", "muted", description));
}
export function badge(text: string, tone = "") {
  return el("span", `badge ${tone}`, text);
}
export function code(value: unknown) {
  return el(
    "pre",
    "code",
    typeof value === "string" ? value : JSON.stringify(value, null, 2),
  );
}
export function table(headers: string[], rows: Child[][]) {
  const body = el("tbody");
  for (const row of rows)
    body.append(el("tr", "", ...row.map((cell) => el("td", "", cell))));
  return el(
    "div",
    "table-scroll",
    el(
      "table",
      "",
      el(
        "thead",
        "",
        el("tr", "", ...headers.map((text) => el("th", "", text))),
      ),
      body,
    ),
  );
}
export function message(target: HTMLElement, error: unknown) {
  if (error instanceof DOMException && error.name === "AbortError") return;
  target.replaceChildren(
    el(
      "p",
      "error",
      error instanceof Error ? error.message : "操作失败，请重试",
    ),
  );
  target.setAttribute("role", "alert");
}
export function status(text: string) {
  return badge(
    text,
    ["completed", "confirmed", "success", "ready"].includes(text)
      ? "good"
      : ["failed", "failure", "conflict", "superseded"].includes(text)
        ? "danger"
        : "",
  );
}
export function short(id: string) {
  return id.slice(0, 8);
}
export function date(ms: number) {
  return new Date(ms).toLocaleString("zh-CN", { timeZoneName: "short" });
}
export function parseObject(text: string): Record<string, string> {
  const value: unknown = JSON.parse(text || "{}");
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    Object.values(value).some((v) => typeof v !== "string")
  )
    throw new Error("条件应为键和值均为字符串的 JSON 对象");
  return value as Record<string, string>;
}
export function form(
  action: (event: SubmitEvent) => void | Promise<void>,
  ...children: Child[]
) {
  const element = el("form", "", ...children);
  element.addEventListener("submit", (event) => {
    event.preventDefault();
    void action(event);
  });
  return element;
}
export function submit(text: string) {
  const control = el("button", "primary", text);
  control.type = "submit";
  return control;
}
export function labelControl<T extends HTMLElement>(
  control: T,
  label: string,
): T {
  control.setAttribute("aria-label", label);
  return control;
}
export interface ViewContext {
  signal: AbortSignal;
  navigate: (page: string, id?: string, version?: number) => void;
  role: string;
}
