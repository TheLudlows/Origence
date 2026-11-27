import { check } from "./platform.mjs";

const loopback = new Set(["127.0.0.1", "localhost", "[::1]"]);
export const isLoopback = url => loopback.has(url.hostname);
export const uuid = /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i;

export function endpointUrl(input) {
  let url;
  try { url = new URL(input); } catch { throw new Error("Supply an absolute Origence API endpoint."); }
  check(!url.username && !url.password && !url.search && !url.hash && url.pathname === "/",
    "Supply only the API origin, with no credentials, query, fragment or /mcp path.");
  check(url.protocol === "https:" || url.protocol === "http:" && isLoopback(url),
    "Use HTTPS for a remote endpoint and HTTP or HTTPS for loopback.");
  return url;
}

async function responseAt(base, path, options) {
  try {
    return await fetch(base + path, { ...options, redirect: "manual", signal: AbortSignal.timeout(8000) });
  } catch {
    throw new Error(`${path} could not reach ${base}. Check the API/UI address, listener and TLS certificate.`);
  }
}

async function expect(response, status, operation) {
  if (response.status !== status) {
    await response.body?.cancel();
    const cause = response.status === 401 ? "HTTP 401. Check the credential and its expiry or revocation"
      : response.status === 403 ? "HTTP 403. Check the workspace, token role and server policy"
      : response.status >= 300 && response.status < 400 ? "redirect refused. Use the final endpoint"
      : `HTTP ${response.status}. Check the configured service and endpoint`;
    throw new Error(`${operation} failed (${cause}).`);
  }
  return response;
}

async function json(response, operation) {
  try { return await response.json(); }
  catch { throw new Error(`${operation} did not return valid JSON.`); }
}

export async function identity(endpoint, token) {
  const response = await responseAt(endpoint, "/v1/whoami", { headers: { Authorization: "Bearer " + token } });
  const result = await json(await expect(response, 200, "API identity"), "API identity");
  check(uuid.test(result.workspace_id) && uuid.test(result.id) && ["admin", "writer", "reader", "workspace"].includes(result.role),
    "The API did not return an Origence workspace and key identity.");
  return result;
}

const requiredTools = [
  "context_search", "context_resolve", "context_bundle", "context_get",
  "context_run_create", "context_run_prepare", "context_run_report", "context_run_get",
  "context_session", "context_turn_get", "context_turn_archive",
];

export async function connectMcp(endpoint, token, workspace) {
  let id = 0;
  const headers = {
    "Content-Type": "application/json",
    Accept: "application/json, text/event-stream",
    Authorization: "Bearer " + token,
    "X-Origence-Workspace": workspace,
  };
  async function rpc(method, params, notification = false) {
    const message = { jsonrpc: "2.0", method, params };
    if (!notification) message.id = ++id;
    const response = await expect(await responseAt(endpoint, "/mcp", {
      method: "POST", headers, body: JSON.stringify(message),
    }), notification ? 202 : 200, `MCP ${method}`);
    if (notification) return;
    // Origence's non-streaming JSON-RPC responses use application/json even
    // though the Streamable HTTP client advertises both media types.
    const result = await json(response, `MCP ${method}`);
    check(result.jsonrpc === "2.0" && result.id === message.id && result.result && !result.error,
      `MCP ${method} returned an invalid response or a tool error.`);
    return result.result;
  }
  async function call(name, args) {
    const result = await rpc("tools/call", { name, arguments: args });
    check(!result.isError && result.content?.[0]?.type === "text", `MCP ${name} returned a tool error.`);
    try { return JSON.parse(result.content[0].text); }
    catch { throw new Error(`MCP ${name} did not return a JSON result.`); }
  }

  const init = await rpc("initialize", {
    protocolVersion: "2025-11-25", capabilities: {},
    clientInfo: { name: "origence-installer", version: "1" },
  });
  check(init.capabilities?.tools && typeof init.protocolVersion === "string", "MCP initialize did not negotiate tools.");
  headers["MCP-Protocol-Version"] = init.protocolVersion;
  await rpc("notifications/initialized", {}, true);
  const catalog = await rpc("tools/list", {});
  const names = catalog.tools?.map(tool => tool.name) ?? [];
  check(requiredTools.every(name => names.includes(name)),
    "MCP tools/list did not return the required Origence tools. Check the server version.");
  const session = await json(await expect(await responseAt(endpoint, "/v1/session", {
    headers: { Authorization: headers.Authorization, "X-Origence-Workspace": workspace },
  }), 200, "HTTP session"), "HTTP session");
  const context = await call("context_session", {});
  check(session.workspace_id === workspace && uuid.test(session.key_id) && uuid.test(session.session_id)
    && context.workspace_id === workspace && context.key_id === session.key_id && context.session_id === session.session_id,
    "MCP context_session does not match the API workspace, logical key and session.");
  return { call, toolCount: names.length };
}

const sessionPath = "/api/console/session";
export async function consoleLogin(base, credentials, workspace, key) {
  await (await expect(await responseAt(base, "/"), 200, "UI root")).body?.cancel();
  const response = await expect(await responseAt(base, sessionPath, {
    method: "POST", headers: { "Content-Type": "application/json", Origin: base },
    body: JSON.stringify(credentials),
  }), 200, "UI login");
  const cookie = response.headers.getSetCookie()[0]?.split(";")[0];
  const profile = await json(response, "UI login");
  const mode = credentials.deployment_key ? "deployment" : credentials.workspace_key ? "workspace" : "account";
  check(cookie && profile.mode === mode && profile.session?.binding, "The UI did not establish the requested credential session.");
  const headers = { Cookie: cookie, "X-Origence-Session": profile.session.binding };
  async function read(path) {
    const response = await responseAt(base, "/api" + path, { headers });
    return json(await expect(response, 200, "Authenticated UI read"), "Authenticated UI read");
  }
  if (mode === "deployment") {
    const catalog = await read("/admin/deployment/workspaces?limit=1");
    check(Array.isArray(catalog.items), "The UI did not verify the deployment administrator roster.");
  } else {
    const grant = profile.session.workspaces.find(item => item.workspace_id === workspace);
    check(grant?.id === key && (mode !== "account" || profile.session.account === credentials.username),
      "The UI session is not bound to the same workspace and key as MCP.");
    headers["X-Origence-Workspace"] = workspace;
    const who = await read("/v1/whoami");
    check(who.workspace_id === workspace && who.id === key, "The UI proxy resolved a different workspace or key.");
  }
  return { read, logout: async () => {
    const response = await responseAt(base, sessionPath, { method: "DELETE", headers: { ...headers, Origin: base } });
    await response.body?.cancel();
  } };
}
