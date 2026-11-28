import { createHash } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";

function runUuid(value) {
  if (typeof value !== "string" ||
      !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(value) ||
      value === "00000000-0000-0000-0000-000000000000") {
    throw new Error("A non-nil UUID is required.");
  }
  return value.toLowerCase();
}

function observationId(run, identity) {
  const bytes = createHash("sha256")
    .update(JSON.stringify(["origence-managed-run-v1", run, ...identity]))
    .digest().subarray(0, 16);
  bytes[6] = (bytes[6] & 15) | 128;
  bytes[8] = (bytes[8] & 63) | 128;
  const hex = bytes.toString("hex");
  return [hex.slice(0, 8), hex.slice(8, 12), hex.slice(12, 16), hex.slice(16, 20), hex.slice(20)].join("-");
}

export async function openRunReporter({
  runId,
  origin = process.env.OC_SERVER_URL ?? "http://127.0.0.1:8080",
  workspaceId = process.env.OC_WORKSPACE_ID,
  token = process.env.OC_API_KEY,
}) {
  runId = runUuid(runId);
  workspaceId = runUuid(workspaceId);
  let base;
  try {
    base = new URL(origin);
  } catch {
    throw new Error("Invalid Origence server origin.");
  }
  const loopback = ["127.0.0.1", "localhost", "[::1]"].includes(base.hostname);
  if ((base.protocol !== "https:" && !(base.protocol === "http:" && loopback)) ||
      base.pathname !== "/" || base.username || base.password || base.search || base.hash) {
    throw new Error("Origence requires an HTTPS origin or loopback HTTP.");
  }
  if (typeof token !== "string" || !/^oc_[a-zA-Z0-9]+$/.test(token)) {
    throw new Error("Set OC_API_KEY to an Origence credential.");
  }
  const headers = {
    Authorization: `Bearer ${token}`,
    "X-Origence-Workspace": workspaceId,
  };
  const runPath = `/v1/runs/${runId}`;
  async function request(path, body, attemptLimit = 1) {
    for (let attempt = 1; attempt <= attemptLimit; attempt++) {
      let response;
      try {
        response = await fetch(new URL(path, base), {
          headers: body ? { ...headers, "Content-Type": "application/json" } : headers,
          method: body ? "POST" : "GET",
          body,
          redirect: "manual",
          signal: AbortSignal.timeout(5000),
        });
      } catch {
        if (attempt === attemptLimit) throw new Error("Origence request unavailable.");
      }
      if (response?.ok) return response;
      if (response) {
        await response.body?.cancel().catch(() => {});
        const status = response.status;
        if (attempt === attemptLimit || (status < 500 && status !== 408 && status !== 429)) {
          throw new Error(`Origence refused the request (HTTP ${status}).`);
        }
      }
      await delay(100 * attempt);
    }
  }
  const identity = await (await request("/v1/whoami")).json()
    .catch(() => { throw new Error("Invalid Origence identity response."); });
  if (identity?.workspace_id !== workspaceId || identity.role !== "workspace") {
    throw new Error("The selected workspace requires the Origence Workspace Key.");
  }
  const prefix = await request(`${runPath}/prefix?through_sequence=0`);
  await prefix.body?.cancel();

  return async (eventIdentity, event, observedAtMs = Date.now()) => {
    if (!Number.isSafeInteger(observedAtMs) || observedAtMs < 0) {
      throw new Error("Invalid observation time.");
    }
    const body = JSON.stringify({
      event_id: observationId(runId, eventIdentity),
      reporter: "client",
      observed_at_ms: observedAtMs,
      event,
    });
    const response = await request(`${runPath}/events`, body, 3);
    await response.body?.cancel().catch(() => {});
  };
}
