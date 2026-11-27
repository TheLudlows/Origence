import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { loadConfiguredConsoleAccounts, workspaceIdPattern, type ConsoleAccount } from "./console-accounts.js";
import { checkedConsoleIdentity, consoleSessionPath, ConsoleSessions, type OwnedWorkspace } from "./console-session.js";
import { HttpError } from "./http-error.js";

const MAX_BODY = 10 * 1024 * 1024;
const uuid = "[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}";
const createWorkspacePath = new RegExp(`^/v1/workspaces/owned/${uuid}$`);
const managedKeyPath = new RegExp(`^/admin/keys/(${uuid})(?:/rotate)?$`);
const agentPath = "/admin/agents";
const keyRules: [string, RegExp][] = [
  ["GET", /^\/admin\/(keys|workspace-key-grants)$/],
  ["POST", new RegExp(`^/admin/keys(?:/${uuid}/rotate)?$`)],
  ["DELETE", new RegExp(`^/admin/(?:keys|workspace-key-grants)/${uuid}$`)],
  ["PUT", new RegExp(`^/admin/workspace-key-grants/${uuid}$`)],
];
const rules: [string, RegExp][] = [
  ["GET", /^\/health\/(live|ready)$/],
  ["GET", /^\/v1\/(whoami|session|assets|runs)$/],
  ["GET", /^\/v1\/runs\/overview$/],
  [
    "POST",
    /^\/v1\/(search|context|knowledge|files|memories\/(identified|semantic|lookup)|captures\/(identified|semantic))$/,
  ],
  ["GET", new RegExp(`^/v1/(assets|events|jobs|files|runs)/${uuid}$`)],
  ["GET", new RegExp(`^/v1/memories/long-term/${uuid}$`)],
  ["DELETE", new RegExp(`^/v1/(assets|events|files)/${uuid}$`)],
  ["POST", new RegExp(`^/v1/assets/${uuid}/restore$`)],
  ["POST", new RegExp(`^/v1/jobs/${uuid}/(retry|cancel)$`)],
  ["GET", new RegExp(`^/v1/runs/${uuid}/prefix$`)],
  ["GET", new RegExp(`^/v1/runs/${uuid}/pdf-source$`)],
  ["GET", new RegExp(`^/v1/runs/${uuid}/verifications$`)],
  ["GET", new RegExp(`^/v1/skill-candidates/${uuid}$`)],
  ["PUT", new RegExp(`^/v1/skill-candidates/${uuid}$`)],
  ["POST", new RegExp(`^/v1/skill-candidates/${uuid}/reviews/${uuid}$`)],
  ["GET", new RegExp(`^/v1/assets/${uuid}/runs$`)],
  ["GET", /^\/v1\/memory-properties\/[a-z0-9_.-]+$/],
  ["PUT", createWorkspacePath],
];
const security = {
  "Cache-Control": "no-store",
  "X-Content-Type-Options": "nosniff",
  "Referrer-Policy": "no-referrer",
  "Content-Security-Policy":
    "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
};

export interface ServerOptions {
  apiBaseUrl: string;
  publicOrigin?: string;
  timeoutMs?: number;
  accounts?: readonly ConsoleAccount[];
}

async function body(req: IncomingMessage): Promise<Buffer | undefined> {
  if (req.method === "GET" || req.method === "HEAD") return;
  if (Number(req.headers["content-length"]) > MAX_BODY)
    throw new HttpError(413, "请求超过 10 MiB");
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of req) {
    size += chunk.length;
    if (size > MAX_BODY) throw new HttpError(413, "请求超过 10 MiB");
    chunks.push(Buffer.from(chunk));
  }
  return Buffer.concat(chunks);
}

async function listOwnedWorkspaces(upstream: URL, authorization: string, signal: AbortSignal) {
  const target = new URL("/v1/workspaces/owned?limit=100", upstream);
  const items: OwnedWorkspace[] = [];
  let after: string | null = null;
  do {
    if (after) target.searchParams.set("after", after);
    const response = await fetch(target, { headers: { authorization }, redirect: "manual", signal });
    if (after === null && (response.status === 403 || response.status === 404)) return null;
    if (!response.ok) throw new HttpError(502, "暂时无法读取子工作区");
    const page = await response.json() as { items?: unknown; next_after?: unknown };
    if (page === null || !Array.isArray(page.items) || page.items.length > 100)
      throw new HttpError(502, "子工作区列表无效");
    for (const item of page.items) {
      if (!item || typeof item.workspace_id !== "string" || !workspaceIdPattern.test(item.workspace_id) ||
          typeof item.name !== "string" || !["reader", "writer", "reviewer", "admin"].includes(item.role))
        throw new HttpError(502, "子工作区列表无效");
      items.push({ workspace_id: item.workspace_id, name: item.name, role: item.role });
    }
    if (page.next_after === null) break;
    if (typeof page.next_after !== "string" || !workspaceIdPattern.test(page.next_after) || (after && page.next_after <= after))
      throw new HttpError(502, "子工作区分页无效");
    after = page.next_after;
  } while (after);
  return items;
}

function json(res: ServerResponse, status: number, message: string) {
  res.writeHead(status, {
    ...security,
    "Content-Type": "application/json; charset=utf-8",
  });
  res.end(JSON.stringify({ error: { code: status, message } }));
}

export function createUiServer(options: ServerOptions) {
  const upstream = new URL(options.apiBaseUrl);
  if (
    !["http:", "https:"].includes(upstream.protocol) ||
    upstream.username ||
    upstream.password ||
    upstream.pathname !== "/" ||
    upstream.search ||
    upstream.hash
  ) {
    throw new Error(
      "ORIGENCE_API_BASE_URL must be an HTTP(S) origin without credentials",
    );
  }
  const publicOrigin = options.publicOrigin
    ? new URL(options.publicOrigin).origin
    : undefined;
  const sessions = new ConsoleSessions(options.accounts ?? [], async (authorization, signal) => {
    const response = await fetch(new URL("/v1/whoami", upstream), {
      headers: { authorization },
      redirect: "manual",
      signal,
    });
    if (response.status === 401 || response.status === 403) return null;
    if (!response.ok) throw new HttpError(502, "暂时无法核验工作区权限");
    return checkedConsoleIdentity(await response.json());
  }, (authorization, signal) => listOwnedWorkspaces(upstream, authorization, signal));
  return createServer(async (req, res) => {
    try {
      const host = req.headers.host ?? "";
      const localOrigin = `http://${host}`;
      const origin = publicOrigin ?? localOrigin;
      const expectedHost = new URL(origin).host;
      if (
        host !== expectedHost ||
        (!publicOrigin &&
          !["localhost", "127.0.0.1", "[::1]"].includes(
            new URL(origin).hostname,
          ))
      ) {
        throw new HttpError(403, "Host 不被允许");
      }
      if (req.headers.origin && req.headers.origin !== origin)
        throw new HttpError(403, "跨源请求不被允许");
      const target = new URL(req.url ?? "/", origin);
      if (target.origin !== origin) throw new HttpError(400, "无效请求地址");
      if (target.pathname.startsWith("/api/")) {
        const abort = new AbortController();
        res.on("close", () => {
          if (!res.writableFinished) abort.abort();
        });
        const signal = AbortSignal.any([
          abort.signal,
          AbortSignal.timeout(options.timeoutMs ?? 30_000),
        ]);
        if (target.pathname === consoleSessionPath) {
          const session = await sessions.handle(req, res, new URL(origin), signal);
          res.writeHead(200, { ...security, "Content-Type": "application/json; charset=utf-8" });
          res.end(JSON.stringify(session));
          return;
        }
        const path = target.pathname.slice(4);
        const managingKeys = keyRules.some(([method, pattern]) => method === req.method && pattern.test(path));
        const viewingAgents = req.method === "GET" && path === agentPath;
        if (
          !managingKeys &&
          !viewingAgents &&
          !rules.some(
            ([method, pattern]) => method === req.method && pattern.test(path),
          )
        )
          throw new HttpError(404, "接口不被允许");
        const headers = new Headers();
        const authorization = req.headers.authorization;
        const creatingWorkspace = req.method === "PUT" && createWorkspacePath.test(path);
        if (!managingKeys && !viewingAgents && !creatingWorkspace && authorization?.startsWith("Bearer ")) headers.set("authorization", authorization);
        else if (!path.startsWith("/health/")) {
          const workspace = sessions.authorization(req, new URL(origin));
          if (creatingWorkspace && !workspace.canCreateOwned) throw new HttpError(403, "当前工作区未授权创建子工作区");
          headers.set("authorization", workspace.authorization);
          if (managingKeys || viewingAgents) {
            if (workspace.role !== "admin")
              throw new HttpError(403, viewingAgents ? "只有工作区管理员可以查看 Agent 连接" : "只有工作区管理员可以管理 API Key");
            const targetId = managedKeyPath.exec(path)?.[1];
            if (targetId?.toLowerCase() === workspace.principalId.toLowerCase())
              throw new HttpError(409, "请在控制台外维护当前会话使用的凭据");
            headers.set("X-Origence-Workspace", workspace.workspaceId);
          } else if (workspace.scopedWorkspace) headers.set("X-Origence-Workspace", workspace.scopedWorkspace);
        }
        const payload = await body(req);
        for (const name of ["content-type", "idempotency-key"]) {
          const value = req.headers[name];
          if (typeof value === "string") headers.set(name, value);
        }
        const response = await fetch(new URL(path + target.search, upstream), {
          method: req.method,
          headers,
          body: payload ? new Uint8Array(payload) : undefined,
          redirect: "manual",
          signal,
        });
        if (response.status >= 300 && response.status < 400)
          throw new HttpError(502, "上游返回了不支持的重定向");
        const bytes = new Uint8Array(await response.arrayBuffer());
        res.writeHead(response.status, {
          ...security,
          "Content-Type":
            response.headers.get("content-type") ?? "application/octet-stream",
          ...(response.headers.has("content-disposition")
            ? {
                "Content-Disposition": response.headers.get(
                  "content-disposition",
                )!,
              }
            : {}),
        });
        res.end(bytes);
        return;
      }
      if (req.method !== "GET" && req.method !== "HEAD")
        throw new HttpError(405, "方法不被允许");
      const path = target.pathname === "/" ? "/index.html" : target.pathname;
      if (!/^\/(index\.html|styles\.css|client\/[a-z-]+\.js)$/.test(path))
        throw new HttpError(404, "页面不存在");
      const type = path.endsWith(".js")
        ? "text/javascript"
        : path.endsWith(".css")
          ? "text/css"
          : "text/html";
      let bytes: Buffer;
      try {
        bytes = await readFile(new URL(`./public${path}`, import.meta.url));
      } catch {
        throw new HttpError(404, "页面不存在");
      }
      res.writeHead(200, {
        ...security,
        "Content-Type": `${type}; charset=utf-8`,
      });
      res.end(req.method === "HEAD" ? undefined : bytes);
    } catch (error) {
      if (res.destroyed || res.headersSent) return;
      if (error instanceof HttpError) json(res, error.status, error.message);
      else
        json(
          res,
          error instanceof Error && error.name === "TimeoutError" ? 504 : 502,
          "Origence 暂不可用，请检查连接后重试",
        );
    }
  });
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  const { values } = parseArgs({
    options: {
      host: { type: "string" },
      port: { type: "string" },
      strictPort: { type: "boolean" },
    },
  });
  const port = Number(
    values.port ?? process.env.ORIGENCE_UI_PORT ?? process.env.PORT ?? 3000,
  );
  if (!Number.isInteger(port) || port < 1 || port > 65535)
    throw new Error("Invalid ORIGENCE_UI_PORT");
  const accountDatabase = process.env.ORIGENCE_UI_ACCOUNTS_DB;
  if (!accountDatabase && process.env.ORIGENCE_UI_ACCOUNTS_FILE)
    throw new Error("Configure local console accounts with ORIGENCE_UI_ACCOUNTS_DB");
  const server = createUiServer({
    apiBaseUrl: process.env.ORIGENCE_API_BASE_URL ?? "http://127.0.0.1:8080",
    publicOrigin: process.env.ORIGENCE_UI_ORIGIN,
    accounts: loadConfiguredConsoleAccounts(process.env),
  });
  server.listen(
    port,
    values.host ?? process.env.ORIGENCE_UI_HOST ?? "127.0.0.1",
    () => console.log(`Origence UI: http://127.0.0.1:${port}`),
  );
  for (const signal of ["SIGTERM", "SIGINT"] as const)
    process.on(signal, () => server.close());
}
