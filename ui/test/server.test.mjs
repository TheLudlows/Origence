import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { once } from "node:events";
import { createUiServer } from "../dist/server.js";
import { byteRange, textDelta } from "../dist/client/source.js";

async function listen(server, t) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => {
    server.closeAllConnections();
    server.close();
  });
  return `http://127.0.0.1:${server.address().port}`;
}

test("proxy isolates credentials, preserves binary uploads and business errors", async (t) => {
  const received = [];
  const upstream = await listen(
    createServer(async (req, res) => {
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      received.push({
        url: req.url,
        key: req.headers.authorization,
        idempotency: req.headers["idempotency-key"],
        bytes: Buffer.concat(chunks),
      });
      res.setHeader("Content-Type", "application/json");
      if (req.url === "/v1/memories/identified") {
        res.statusCode = 409;
        res.end(
          JSON.stringify({
            error: { code: "CONFLICT", message: "expected_version mismatch" },
          }),
        );
      } else res.end(JSON.stringify({ user: req.headers.authorization }));
    }),
    t,
  );
  const ui = await listen(createUiServer({ apiBaseUrl: upstream }), t);
  const [a, b] = await Promise.all(
    ["alice", "bob"].map((user) =>
      fetch(ui + "/api/v1/whoami", {
        headers: { Authorization: `Bearer ${user}` },
      }).then((r) => r.json()),
    ),
  );
  assert.equal(a.user, "Bearer alice");
  assert.equal(b.user, "Bearer bob");
  const verificationPath = "/api/v1/runs/22222222-2222-4222-8222-222222222222/verifications?limit=25";
  const verificationResponse = await fetch(ui + verificationPath, { headers: { Authorization: "Bearer alice" } });
  assert.equal(verificationResponse.status, 200);
  assert.equal(received.at(-1).url, verificationPath.replace("/api", ""));
  assert.equal(received.at(-1).key, "Bearer alice");
  const bytes = new Uint8Array([0, 255, 1, 128]);
  await fetch(ui + "/api/v1/files?name=中文.pdf&format=pdf", {
    method: "POST",
    headers: {
      Authorization: "Bearer alice",
      "Idempotency-Key": "upload-1",
      "Content-Type": "application/octet-stream",
    },
    body: bytes,
  });
  assert.deepEqual(received.at(-1).bytes, Buffer.from(bytes));
  assert.equal(received.at(-1).idempotency, "upload-1");
  const conflict = await fetch(ui + "/api/v1/memories/identified", {
    method: "POST",
    headers: {
      Authorization: "Bearer bob",
      "Content-Type": "application/json",
    },
    body: "{}",
  });
  assert.equal(conflict.status, 409);
  assert.match((await conflict.json()).error.message, /expected_version/);
  assert.equal(conflict.headers.get("cache-control"), "no-store");
});

test("proxy rejects unknown methods, ambient origins, redirects and oversized bodies", async (t) => {
  let calls = 0;
  const upstream = await listen(
    createServer((req, res) => {
      calls++;
      res.writeHead(302, { Location: "https://example.invalid/" });
      res.end();
    }),
    t,
  );
  const ui = await listen(createUiServer({ apiBaseUrl: upstream }), t);
  assert.equal((await fetch(ui + "/api/admin/keys")).status, 404);
  assert.equal((await fetch(ui + "/api/v1/whoami")).status, 401);
  assert.equal(
    (
      await fetch(ui + "/api/v1/whoami", {
        headers: {
          Authorization: "Bearer private",
          Origin: "https://evil.invalid",
        },
      })
    ).status,
    403,
  );
  assert.equal(
    (
      await fetch(ui + "/api/v1/whoami", {
        headers: { Authorization: "Bearer private" },
      })
    ).status,
    502,
  );
  assert.equal(calls, 1);
  assert.equal(
    (
      await fetch(ui + "/api/v1/files", {
        method: "POST",
        headers: { Authorization: "Bearer private" },
        body: Buffer.alloc(10 * 1024 * 1024 + 1),
      })
    ).status,
    413,
  );
  const index = await fetch(ui);
  assert.match(
    index.headers.get("content-security-policy"),
    /script-src 'self'/,
  );
  assert.match(await index.text(), /Origence/);
  assert.equal((await fetch(ui + "/package.json")).status, 404);
  assert.throws(() =>
    createUiServer({ apiBaseUrl: "http://user:password@example.invalid" }),
  );
});

test("upstream timeouts are bounded", async (t) => {
  const upstream = await listen(
    createServer(() => {}),
    t,
  );
  const ui = await listen(
    createUiServer({ apiBaseUrl: upstream, timeoutMs: 30 }),
    t,
  );
  assert.equal((await fetch(ui + "/api/health/ready")).status, 504);
});

test("UTF-8 source highlighting preserves Chinese, emoji and combining characters", () => {
  const text = "前言\n中文 🦀 é 后文";
  const start = new TextEncoder().encode("前言\n").length;
  const end = start + new TextEncoder().encode("中文 🦀 é").length;
  assert.deepEqual(byteRange(text, start, end), [
    "前言\n",
    "中文 🦀 é",
    " 后文",
  ]);
  assert.throws(() => byteRange(text, start + 1, end));
  assert.throws(() => byteRange(text, -1, end));
});

test("version diffs reconstruct both fixed inputs without changing line endings", () => {
  for (const [before, after] of [
    ["", "新增 🦀"],
    ["相同\r\n旧内容\r\n结尾", "相同\r\n新内容 é\r\n结尾"],
    ["same", "same"],
    ["delete\n", ""],
  ]) {
    const delta = textDelta(before, after);
    assert.equal(delta.prefix + delta.removed + delta.suffix, before);
    assert.equal(delta.prefix + delta.added + delta.suffix, after);
  }
});
