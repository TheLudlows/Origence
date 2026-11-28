import { test } from "node:test";
import assert from "node:assert/strict";
import { Window } from "happy-dom";
import { sessionsPage } from "../dist/client/sessions.js";

test("session history remains read-only and retries the same page", async (t) => {
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
  const pages = [];
  globalThis.fetch = async (path, request) => {
    assert.equal(request.method, "GET");
    if (path === "/api/v1/session") return Response.json({ session_id: "private-session" });
    pages.push(path);
    if (pages.length === 2) return Response.json({ error: { message: "Unavailable" } }, { status: 503 });
    if (pages.length === 1) return Response.json({
      next_after: 16,
      turns: [{ question: "第一轮", answer: "第一条答复" }],
    });
    return Response.json({ next_after: null, turns: [{ question: "第二轮", answer: "第二条答复" }] });
  };
  const root = document.body;
  await sessionsPage(root, { signal: new AbortController().signal, navigate() {} });
  const click = (name) => {
    const button = [...root.querySelectorAll("button")].find((control) => control.textContent === name);
    assert.ok(button, name);
    button.click();
  };
  async function waitFor(text) {
    for (let attempt = 0; attempt < 50; attempt++) {
      if (root.textContent.includes(text)) return;
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    assert.fail("Missing text: " + text);
  }
  assert.match(root.textContent, /第一条答复/);
  click("加载更多");
  await waitFor("重试读取");
  click("重试读取");
  await waitFor("第二条答复");
  const endpoint = "/api/v1/sessions/private-session/turns?after=";
  assert.deepEqual(pages, [endpoint + "0", endpoint + "16", endpoint + "16"]);
  assert.match(root.textContent, /第一条答复/);
  assert.doesNotMatch(root.textContent, /清空会话记录|删除这一轮/);
});
