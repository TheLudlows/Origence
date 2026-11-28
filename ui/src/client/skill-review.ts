import { api } from "./api.js";
import { button, code, cursor, el, message, paged, short } from "./components.js";
import type { Page } from "./types.js";

const decisions = { pending: "待审核", approved: "已批准", rejected: "已拒绝" };
type Decision = keyof typeof decisions;
type Review = { review_id: string; revision: number; decision: Decision };
type Detail = {
  packet: {
    digest: string; basis: string;
    package: { manifest: { name: string; version: number }; files: Record<string, string> };
  };
  decision: Decision;
  published: boolean;
};

export function skillReviewPage(root: HTMLElement, base: string, signal: AbortSignal) {
  const view = el("div");
  root.append(view);
  let active = new AbortController();
  function reset() {
    active.abort();
    active = new AbortController();
    view.replaceChildren();
    return AbortSignal.any([signal, active.signal]);
  }
  function list() {
    const request = reset();
    paged<Review, string>(view, request, (after) => api.get<Page<Review>>(cursor(base, after), request),
      (review) => button(`${short(review.review_id)} · 修订 ${review.revision} · ${decisions[review.decision]}`,
        () => open(review.review_id), "link"));
  }
  async function open(id: string) {
    const request = reset();
    const path = `${base}/${encodeURIComponent(id)}`;
    const feedback = el("div");
    view.append(button("返回列表", list), feedback);
    try {
      const { packet, decision, published } = await api.get<Detail>(path, request);
      if (request.aborted) return;
      const { manifest, files } = packet.package;
      const status = el("p", "muted", published ? "已发布" : decisions[decision]);
      status.setAttribute("role", "status");
      const actions = el("div", "actions");
      function action(label: string, operation: "decision" | "publish", data: { approve?: boolean }, done: string) {
        return button(label, async () => {
          const controls = Array.from(actions.querySelectorAll("button"));
          controls.forEach((control) => { control.disabled = true; });
          feedback.replaceChildren();
          try {
            await api.post(`${path}/${operation}`, { digest: packet.digest, ...data }, request);
            if (!request.aborted) {
              status.textContent = done;
              actions.replaceChildren(...(data.approve ? [publish()] : []));
            }
          } catch (error) {
            if (!request.aborted) message(feedback, error);
          } finally {
            controls.forEach((control) => { control.disabled = false; });
          }
        });
      }
      const publish = () => action("发布 Skill", "publish", {}, "已发布");
      if (decision === "pending") actions.append(action("批准", "decision", { approve: true }, "已批准"), action("拒绝", "decision", { approve: false }, "已拒绝"));
      if (decision === "approved" && !published) actions.append(publish());
      view.append(el("h3", "", `${manifest.name} · v${manifest.version}`), status,
        el("h4", "", "选定依据"), code(packet.basis), el("p", "code", packet.digest),
        ...Object.entries(files).map(([path, body]) => el("section", "", el("h4", "", path), code(body))), actions);
    } catch (error) {
      if (!request.aborted) message(feedback, error);
    }
  }
  list();
}
