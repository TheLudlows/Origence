import { api } from "./api.js";
import { button, el, field, form, input, message, panel, submit, type ViewContext } from "./components.js";
import { readRunPrefix, type RunPrefix } from "./run-prefix.js";

const uuid = "[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}";
type RunSource = RunPrefix & { kind: "run" };

interface DraftText {
  name: string;
  description: string;
  steps: string;
  selected_basis: string;
  private_notes: string;
}

interface CandidateReceipt {
  candidate_id: string;
  revision: number;
}

function savedNotes(value: string) {
  let notes: { format?: unknown; selected_basis?: unknown; private_notes?: unknown } | null = null;
  try { notes = JSON.parse(value); } catch { /* A manual candidate may have plain-text notes. */ }
  if (notes?.format !== "origence-selected-experience-v1" ||
      typeof notes.selected_basis !== "string" || typeof notes.private_notes !== "string") {
    throw new Error("该候选没有已确认的选定依据");
  }
  return { selected_basis: notes.selected_basis, private_notes: notes.private_notes };
}

interface SavedCandidate extends CandidateReceipt {
  input: {
    draft: { name: string; description: string; body: string; references: Record<string, string> };
    sources: RunSource[];
    private_notes: string;
  };
}

function idInput(value = "") {
  const control = input("", value);
  control.pattern = uuid;
  control.required = true;
  control.autocomplete = "off";
  control.spellcheck = false;
  return control;
}

async function reopen(run: string, id: string, signal: AbortSignal) {
  const saved = await api.get<SavedCandidate>(`/v1/skill-candidates/${id}`, signal);
  const { draft, sources, private_notes } = saved.input;
  const source = sources[0];
  if (saved.candidate_id !== id || !Number.isInteger(saved.revision) || saved.revision < 1 ||
      sources.length !== 1 || source?.kind !== "run" || source.run_id !== run.toLowerCase() ||
      !Number.isSafeInteger(source.through_sequence) || source.through_sequence < 0 ||
      Object.keys(draft.references).length !== 0) {
    throw new Error("该候选不属于此 Run 的选定经历");
  }
  // The existing owner-only API stores selected-experience text in its private notes envelope.
  const notes = savedNotes(private_notes);
  const prefix = await readRunPrefix(run, source.through_sequence, signal);
  if (prefix.digest !== source.digest) throw new Error("候选的固定前缀摘要已不匹配");
  return { prefix, revision: saved.revision, text: {
    name: draft.name, description: draft.description, steps: draft.body,
    ...notes,
  } };
}

export function runSkillDrafting(run: string, ctx: ViewContext) {
  const body = el("div");
  const notice = el("div", "muted");
  notice.setAttribute("role", "status");
  const error = el("div");
  const existing = idInput();
  const open = submit("打开候选");
  let active: AbortController | undefined;
  const loadingForm = form(() => {
    const id = existing.value.toLowerCase();
    return launch(open, id, (signal) => reopen(run, id, signal));
  }, field("已有候选 ID", existing), open);
  const root = panel("Skill 候选", loadingForm, notice, error, body);
  root.hidden = true;

  function reset() {
    active?.abort();
    active = undefined;
    body.replaceChildren();
    error.replaceChildren();
    notice.textContent = "";
    existing.value = "";
    loadingForm.hidden = false;
  }

  async function launch(
    origin: HTMLButtonElement,
    id: string | undefined,
    load: (signal: AbortSignal) => Promise<{ prefix: RunPrefix; revision: number; text?: DraftText }>,
  ) {
    if (ctx.signal.aborted || origin.getAttribute("aria-disabled") === "true") return;
    if (body.hasChildNodes()) {
      body.querySelector("input")?.focus();
      return;
    }
    active?.abort();
    active = new AbortController();
    const signal = AbortSignal.any([active.signal, ctx.signal]);
    origin.setAttribute("aria-disabled", "true");
    notice.textContent = "正在核对固定前缀…";
    error.replaceChildren();
    try {
      const { prefix, revision, text } = await load(signal);
      if (signal.aborted) return;
      const candidateId = id ?? crypto.randomUUID();
      loadingForm.hidden = true;
      editCandidate(body, candidateId, { kind: "run", ...prefix }, revision, text, signal, (keepId) => {
        reset();
        if (keepId) existing.value = candidateId;
        (origin.isConnected && !origin.hidden ? origin : existing).focus();
      });
    } catch (e) {
      if (!signal.aborted) {
        active = undefined;
        loadingForm.hidden = false;
        message(error, e);
      }
    } finally {
      if (!signal.aborted) notice.textContent = "";
      origin.removeAttribute("aria-disabled");
    }
  }

  return {
    root,
    reset,
    action(sequence: number) {
      const draft = button("选取依据并起草", () => launch(draft, undefined, async (signal) => ({
        prefix: await readRunPrefix(run, sequence, signal), revision: 0,
      })));
      return draft;
    },
  };
}

function editCandidate(
  root: HTMLElement, id: string, source: RunSource, revision: number,
  initial: DraftText | undefined, signal: AbortSignal, close: (keepId: boolean) => void,
) {
  const name = input("例如 verify-result", initial?.name);
  name.required = true;
  name.pattern = "[a-z0-9\\-]{1,64}";
  name.autocomplete = "off";
  const description = el("textarea");
  description.rows = 2;
  description.value = initial?.description ?? "";
  description.required = true;
  const steps = el("textarea");
  steps.value = initial?.steps ?? "";
  steps.required = true;
  const basis = el("textarea");
  basis.value = initial?.selected_basis ?? "";
  basis.required = true;
  const notes = el("textarea");
  notes.value = initial?.private_notes ?? "";
  const disclosure = el("input");
  disclosure.type = "checkbox";
  disclosure.required = true;
  disclosure.checked = !!initial;
  const save = submit("保存候选");
  const dismiss = button("关闭草稿", () => {
    if (!dirty() || window.confirm("放弃未保存的修改？")) close(revision > 0 || saveAttempted);
  });
  const candidateId = idInput(id);
  candidateId.readOnly = true;
  const reviewer = idInput();
  const skill = idInput(crypto.randomUUID());
  const version = input("", "1", "number");
  version.required = true;
  version.min = "1";
  version.max = "4294967295";
  const send = submit("提交审阅");
  const activity = el("p", "muted");
  activity.setAttribute("role", "status");
  const error = el("div");
  const receipt = el("div");
  const read = (): DraftText => ({
    name: name.value, description: description.value, steps: steps.value,
    selected_basis: basis.value, private_notes: notes.value,
  });
  let snapshot = JSON.stringify(read());
  let savedBasis = initial?.selected_basis ?? "";
  let busy = false;
  let saveAttempted = false;
  let review: { fingerprint: string; id: string; submitted: boolean } | undefined;
  const dirty = () => JSON.stringify(read()) !== snapshot;
  const reviewFingerprint = () => JSON.stringify([
    revision, reviewer.value.toLowerCase(), skill.value.toLowerCase(), version.valueAsNumber,
  ]);
  const fields = [name, description, steps, basis, notes];
  function update() {
    for (const control of [...fields, disclosure, reviewer, skill, version, dismiss]) control.disabled = busy;
    save.disabled = busy || (revision > 0 && !dirty());
    send.disabled = busy || revision === 0 || dirty() || !disclosure.checked ||
      (review?.submitted === true && review.fingerprint === reviewFingerprint());
  }
  async function perform(action: () => Promise<void>) {
    if (busy || signal.aborted) return;
    busy = true;
    update();
    error.replaceChildren();
    try {
      await action();
    } catch (e) {
      if (!signal.aborted) message(error, e);
    } finally {
      busy = false;
      update();
    }
  }
  const editor = form(() => perform(async () => {
    const values = read();
    saveAttempted = true;
    const saved = await api.request<CandidateReceipt>(`/v1/skill-candidates/${id}`, {
      method: "PUT", signal, data: {
        expected_revision: revision,
        candidate: { ...values, source, disclosure_confirmed: disclosure.checked },
      },
    });
    if (saved.candidate_id !== id || saved.revision !== revision + 1) throw new Error("候选修订响应无效");
    const first = revision === 0;
    revision = saved.revision;
    savedBasis = values.selected_basis;
    snapshot = JSON.stringify(values);
    review = undefined;
    receipt.replaceChildren();
    activity.textContent = first ? "已保存修订 1。" : `已保存修订 ${revision}，旧批准失效。`;
  }), field("Skill 名称", name), field("描述", description), field("方法步骤", steps),
  field("选定依据（审阅者可见）", basis), field("私有备注（可选）", notes),
  el("label", "", disclosure, " 我确认可向指定审阅者披露方法和选定依据"), save);
  const submission = form(() => perform(async () => {
    if (revision === 0 || dirty() || !disclosure.checked) return;
    const fingerprint = reviewFingerprint();
    if (review?.fingerprint !== fingerprint) review = { fingerprint, id: crypto.randomUUID(), submitted: false };
    if (review.submitted) return;
    const scope = await api.get<{ tenant_id: string; workspace_id: string }>("/v1/whoami", signal);
    const packet = await api.post<{ review_id: string; digest: string }>(
      `/v1/skill-candidates/${id}/reviews/${review.id}`, {
        reviewer_id: reviewer.value.toLowerCase(), revision, basis: savedBasis,
        target: { tenant_id: scope.tenant_id, workspace_id: scope.workspace_id,
          skill_id: skill.value.toLowerCase(), version: version.valueAsNumber },
      }, signal,
    );
    if (packet.review_id !== review.id || !/^[0-9a-f]{64}$/.test(packet.digest))
      throw new Error("审阅提交响应无效");
    review.submitted = true;
    receipt.replaceChildren(el("p", "", `修订 ${revision} 已提交审阅。`),
      field("审阅 ID", Object.assign(idInput(packet.review_id), { readOnly: true })),
      el("p", "code", `审阅包摘要 ${packet.digest}`),
      el("p", "muted", "指定审阅者可使用审阅 ID 在 Skill CLI 中查看审阅包。"));
  }), field("审阅者凭据 ID", reviewer), field("目标 Skill ID", skill), field("发布版本", version), send);
  editor.addEventListener("input", (event) => {
    if (event.target !== disclosure) disclosure.checked = false;
    update();
  });
  submission.addEventListener("input", update);
  activity.textContent = revision ? `已打开修订 ${revision}。` : "";
  root.replaceChildren(
    el("p", "code", `固定 Run 前缀截至事件 ${source.through_sequence}\n${source.digest}`),
    el("p", "muted", "请手动填写已脱敏的依据。审阅者只能看到保存的方法和选定依据。"),
    field("候选 ID", candidateId), editor, activity,
    el("h3", "", "指定审阅者及发布目标"), submission, receipt, error, dismiss,
  );
  update();
  name.focus();
}
