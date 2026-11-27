import { test } from "node:test";
import assert from "node:assert/strict";
import { Window } from "happy-dom";
import { archivePage } from "../dist/client/archive.js";

test("archive clear resumes the failed page with the original cutoff", async (t) => {
  const window = new Window();
  const saved = new Map(["window", "document", "Node", "fetch"].map(
    (key) => [key, Object.getOwnPropertyDescriptor(globalThis, key)],
  ));
  t.after(async () => {
    for (const [key, descriptor] of saved) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
    await window.happyDOM.close();
  });
  Object.assign(globalThis, { window, document: window.document, Node: window.Node });
  window.confirm = () => true;
  const archive = "/api/v1/sessions/private-session";
  const deletes = [];
  let refreshed = false;
  globalThis.fetch = async (path, request) => {
    if (path === "/api/v1/session") return Response.json({ session_id: "private-session" });
    if (request.method === "DELETE") {
      deletes.push(path);
      if (deletes.length === 1) return Response.json({ next_after: "16" });
      if (deletes.length === 2) {
        return Response.json({ error: { message: "AnySeek unavailable" } }, { status: 503 });
      }
      refreshed = true;
      return Response.json({ next_after: null });
    }
    assert.match(path, /\/turns\?/);
    return Response.json({
      next_after: null,
      through_sequence: refreshed ? "21" : "20",
      turns: [refreshed
        ? { turn_id: "new", question: "保留的问答", answer: "新问题" }
        : { turn_id: "old", question: "待清空问答", answer: "历史内容" }],
    });
  };
  const root = document.body;
  await archivePage(root, { role: "writer", signal: new AbortController().signal, navigate() {} });
  const click = (name) => {
    const button = [...root.querySelectorAll("button")].find((control) => control.textContent === name);
    assert.ok(button, name);
    button.click();
  };
  async function waitFor(status) {
    for (let attempt = 0; attempt < 50; attempt++) {
      if (root.textContent.includes(status)) return;
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    assert.fail("Missing text: " + status);
  }
  click("清空归档历史");
  await waitFor("删除未确认");
  assert.match(root.textContent, /待清空问答/);
  assert.doesNotMatch(root.textContent, /删除完成/);
  click("重试删除");
  await waitFor("删除完成");
  assert.deepEqual(deletes, [
    archive + "?through_sequence=20",
    archive + "?through_sequence=20&after=16",
    archive + "?through_sequence=20&after=16",
  ]);
  assert.match(root.textContent, /保留的问答/);
  assert.doesNotMatch(root.textContent, /待清空问答/);
});
