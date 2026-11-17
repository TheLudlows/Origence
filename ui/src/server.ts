import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const MAX_BODY = 10 * 1024 * 1024;
const uuid = "[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}";
const rules: [string, RegExp][] = [
  ["GET", /^\/health\/(live|ready)$/],
  ["GET", /^\/v1\/(whoami|session|assets|runs)$/],
  [
    "POST",
    /^\/v1\/(search|context|knowledge|files|memories\/(identified|semantic|lookup)|captures\/(identified|semantic))$/,
  ],
  ["GET", new RegExp(`^/v1/(assets|events|jobs|files|runs)/${uuid}$`)],
  ["DELETE", new RegExp(`^/v1/(assets|events|files)/${uuid}$`)],
  ["POST", new RegExp(`^/v1/assets/${uuid}/restore$`)],
  ["POST", new RegExp(`^/v1/jobs/${uuid}/(retry|cancel)$`)],
  ["GET", new RegExp(`^/v1/runs/${uuid}/prefix$`)],
  ["GET", new RegExp(`^/v1/assets/${uuid}/runs$`)],
  ["GET", /^\/v1\/memory-properties\/[a-z0-9_.-]+$/],
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
}

class HttpError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
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
        const path = target.pathname.slice(4);
        if (
          !rules.some(
            ([method, pattern]) => method === req.method && pattern.test(path),
          )
        )
          throw new HttpError(404, "接口不被允许");
        if (
          !path.startsWith("/health/") &&
          !req.headers.authorization?.startsWith("Bearer ")
        )
          throw new HttpError(401, "请先连接 Origence");
        const payload = await body(req);
        const abort = new AbortController();
        res.on("close", () => {
          if (!res.writableFinished) abort.abort();
        });
        const headers = new Headers();
        for (const name of [
          "authorization",
          "content-type",
          "idempotency-key",
        ]) {
          const value = req.headers[name];
          if (typeof value === "string") headers.set(name, value);
        }
        const response = await fetch(new URL(path + target.search, upstream), {
          method: req.method,
          headers,
          body: payload ? new Uint8Array(payload) : undefined,
          redirect: "manual",
          signal: AbortSignal.any([
            abort.signal,
            AbortSignal.timeout(options.timeoutMs ?? 30_000),
          ]),
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
  const server = createUiServer({
    apiBaseUrl: process.env.ORIGENCE_API_BASE_URL ?? "http://127.0.0.1:8080",
    publicOrigin: process.env.ORIGENCE_UI_ORIGIN,
  });
  server.listen(
    port,
    values.host ?? process.env.ORIGENCE_UI_HOST ?? "127.0.0.1",
    () => console.log(`Origence UI: http://127.0.0.1:${port}`),
  );
  for (const signal of ["SIGTERM", "SIGINT"] as const)
    process.on(signal, () => server.close());
}
