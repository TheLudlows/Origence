import { test } from "node:test";
import assert from "node:assert/strict";
import { Api } from "../dist/client/api.js";

for (const cancellation of ["credentials", "workspace", "route"]) {
  test(`${cancellation} cancellation rejects a response still being decoded`, async (t) => {
    const originalFetch = globalThis.fetch;
    t.after(() => {
      globalThis.fetch = originalFetch;
    });
    const decoded = Promise.withResolvers();
    const started = Promise.withResolvers();
    globalThis.fetch = async () => ({
      ok: true,
      status: 200,
      json() {
        started.resolve();
        return decoded.promise;
      },
    });
    const client = new Api();
    if (cancellation === "workspace") client.setWorkspace("old-workspace", "fixture-session");
    else client.setToken("old-fixture-key");
    const route = new AbortController();
    const response = client.get("/v1/whoami", route.signal);
    await started.promise;
    if (cancellation === "credentials") client.setToken("new-fixture-key");
    else if (cancellation === "workspace") client.setWorkspace("new-workspace", "fixture-session");
    else route.abort();
    decoded.resolve({ private_content: "old response" });
    await assert.rejects(response, (error) => error.name === "AbortError");
  });
}
