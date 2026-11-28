import { randomBytes } from "node:crypto";
import type { IncomingMessage, ServerResponse } from "node:http";
import {
  passwordMatches,
  validPasswordInput,
  workspaceIdPattern,
  type ConsoleAccount,
} from "./console-accounts.js";
import type { ConsoleSessionResponse, User } from "./client/types.js";
import { HttpError } from "./http-error.js";

export const consoleSessionPath = "/api/console/session";
const sessionSeconds = 8 * 60 * 60;
const loginWindowMs = 10 * 60 * 1000;
const cookieIdPattern = /^[A-Za-z0-9_-]{43}$/;
const workspaceKeyPattern = /^oc_[a-f0-9]{64}$/;
const loopbackHosts = new Set(["localhost", "127.0.0.1", "[::1]"]);

type Access =
  | { kind: "deployment"; authorization: string }
  | { kind: "workspace"; authorization: string; workspaceId: string };

type Session = Access & {
  account: string;
  binding: string;
  origin: string;
  expiresAt: number;
};

function cookieName(origin: URL) {
  const prefix = origin.protocol === "https:" ? "__Secure-" : "";
  return `${prefix}origence-console-${origin.port || (prefix ? "443" : "80")}`;
}

function cookieId(req: IncomingMessage, origin: URL) {
  const prefix = cookieName(origin) + "=";
  const cookies = req.headers.cookie?.split(";").map((part) => part.trim());
  const matches = cookies?.filter((part) => part.startsWith(prefix)) ?? [];
  const id = matches[0]?.slice(prefix.length);
  return matches.length === 1 && id && cookieIdPattern.test(id) ? id : "";
}

function setCookie(res: ServerResponse, origin: URL, value: string, seconds: number) {
  const secure = origin.protocol === "https:" ? "; Secure" : "";
  res.setHeader(
    "Set-Cookie",
    `${cookieName(origin)}=${value}; Path=/api; Max-Age=${seconds}; HttpOnly; SameSite=Strict${secure}`,
  );
}

async function loginPayload(req: IncomingMessage): Promise<Record<string, unknown>> {
  if (req.headers["content-type"]?.split(";")[0] !== "application/json")
    throw new HttpError(415, "登录请求必须使用 JSON");
  if (Number(req.headers["content-length"]) > 4096)
    throw new HttpError(413, "登录请求过大");
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of req) {
    const part = Buffer.from(chunk);
    size += part.length;
    if (size > 4096) throw new HttpError(413, "登录请求过大");
    chunks.push(part);
  }
  try {
    const value: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    if (value && typeof value === "object" && !Array.isArray(value))
      return value as Record<string, unknown>;
  } catch {
    // Malformed input receives the same body-free error.
  }
  throw new HttpError(400, "无效的登录请求");
}

export class ConsoleSessions {
  private readonly sessions = new Map<string, Session>();
  private readonly accounts: ReadonlyMap<string, ConsoleAccount>;
  private readonly attempts = new Map<string, { count: number; expiresAt: number }>();
  private verifying = 0;
  private readonly dummyHash = `scrypt-v1$${randomBytes(16).toString("hex")}$${randomBytes(32).toString("hex")}`;

  constructor(
    accounts: readonly ConsoleAccount[],
    private readonly identify: (authorization: string, signal: AbortSignal) => Promise<User | null>,
    private readonly identifyDeployment: (authorization: string, signal: AbortSignal) => Promise<boolean>,
  ) {
    this.accounts = new Map(accounts.map((account) => [account.username, account]));
  }

  private signedOut(): ConsoleSessionResponse {
    return this.accounts.size
      ? { mode: "account", session: null }
      : { mode: "account", session: null, setupRequired: true };
  }

  private current(req: IncomingMessage, origin: URL) {
    const id = cookieId(req, origin);
    const session = this.sessions.get(id);
    if (session?.origin !== origin.origin) return;
    if (session.expiresAt <= Date.now()) {
      this.sessions.delete(id);
      return;
    }
    return session;
  }

  private boundSession(req: IncomingMessage, origin: URL) {
    const session = this.current(req, origin);
    if (!session || session.binding !== req.headers["x-origence-session"])
      throw new HttpError(401, "请刷新登录会话");
    if (req.method !== "GET" && req.headers.origin !== origin.origin)
      throw new HttpError(403, "跨源请求不被允许");
    return session;
  }

  deploymentAuthorization(req: IncomingMessage, origin: URL) {
    const session = this.boundSession(req, origin);
    if (session.kind !== "deployment") throw new HttpError(403, "此入口需要管理员权限");
    return session.authorization;
  }

  authorization(req: IncomingMessage, origin: URL) {
    const session = this.boundSession(req, origin);
    if (session.kind !== "workspace" || session.workspaceId !== req.headers["x-origence-workspace"])
      throw new HttpError(403, "工作区与凭据不匹配");
    return session;
  }

  async handle(
    req: IncomingMessage,
    res: ServerResponse,
    origin: URL,
    signal: AbortSignal,
  ): Promise<ConsoleSessionResponse> {
    const current = this.current(req, origin);
    const existingId = cookieId(req, origin);
    if (req.method === "GET") {
      if (!current) return this.signedOut();
      const user = current.kind === "workspace" ? await this.identify(current.authorization, signal) : null;
      const valid = current.kind === "workspace"
        ? user?.role === "workspace" && user.workspace_id === current.workspaceId
        : await this.identifyDeployment(current.authorization, signal);
      if (this.current(req, origin) !== current) return this.signedOut();
      if (!valid) {
        this.sessions.delete(existingId);
        setCookie(res, origin, "", 0);
        return this.signedOut();
      }
      return this.response(current, user);
    }
    if (req.method !== "POST" && req.method !== "DELETE")
      throw new HttpError(405, "方法不被允许");
    if (req.headers.origin !== origin.origin)
      throw new HttpError(403, "跨源请求不被允许");
    if (req.method === "DELETE") {
      if (current) this.sessions.delete(existingId);
      setCookie(res, origin, "", 0);
      return this.signedOut();
    }
    if (origin.protocol !== "https:" && !loopbackHosts.has(origin.hostname))
      throw new HttpError(403, "远程控制台登录需要 HTTPS");
    const payload = await loginPayload(req);
    const [session, identity] = Object.hasOwn(payload, "workspace_key")
      ? await this.keySession(payload, origin, signal)
      : [await this.accountSession(payload, origin, signal), null] as const;
    signal.throwIfAborted();
    const now = Date.now();
    for (const [id, entry] of this.sessions) {
      if (entry.expiresAt <= now) this.sessions.delete(id);
    }
    if (current) this.sessions.delete(existingId);
    if (this.sessions.size >= 1024)
      throw new HttpError(503, "登录会话已满，请稍后重试");
    const id = randomBytes(32).toString("base64url");
    this.sessions.set(id, session);
    setCookie(res, origin, id, sessionSeconds);
    return this.response(session, identity);
  }

  private response(session: Session, user: User | null): ConsoleSessionResponse {
    return {
      mode: session.kind,
      session: {
        account: session.account,
        binding: session.binding,
        workspaces: user ? [{ ...user, name: user.workspace_id }] : [],
      },
    };
  }

  private async accountSession(payload: Record<string, unknown>, origin: URL, signal: AbortSignal): Promise<Session> {
    const { username, password } = payload;
    if (Object.keys(payload).length !== 2 || typeof username !== "string" || username.length > 64 || !validPasswordInput(password))
      throw new HttpError(400, "无效的登录请求");
    if (!this.accounts.size)
      throw new HttpError(503, "控制台尚未创建账号，请联系管理员");
    const account = this.accounts.get(username);
    const bucket = account?.username ?? "";
    const now = Date.now();
    const previous = this.attempts.get(bucket);
    const attempts = previous && previous.expiresAt > now
      ? previous
      : { count: 0, expiresAt: now + loginWindowMs };
    if (this.verifying >= 4 || attempts.count >= 10)
      throw new HttpError(429, "登录尝试过多，请稍后重试");
    attempts.count++;
    this.attempts.set(bucket, attempts);
    this.verifying++;
    try {
      if (!(await passwordMatches(password, account?.passwordHash ?? this.dummyHash)) || !account)
        throw new HttpError(401, "账号或密码错误");
      this.attempts.delete(bucket);
      const authorization = account.deploymentAuthorization;
      if (!await this.identifyDeployment(authorization, signal))
        throw new HttpError(401, "管理员凭据已失效");
      return this.session(account.username, origin, { kind: "deployment", authorization });
    } finally {
      this.verifying--;
    }
  }

  private async keySession(payload: Record<string, unknown>, origin: URL, signal: AbortSignal) {
    const key = payload.workspace_key;
    if (Object.keys(payload).length !== 1 || typeof key !== "string" || !workspaceKeyPattern.test(key))
      throw new HttpError(400, "无效的 Key 登录请求");
    if (this.verifying >= 4)
      throw new HttpError(429, "登录尝试过多，请稍后重试");
    this.verifying++;
    try {
      const authorization = `Bearer ${key}`;
      const identity = await this.identify(authorization, signal);
      if (identity?.role !== "workspace") throw new HttpError(401, "工作空间 Key 无效");
      const session = this.session("工作空间 Key", origin, { kind: "workspace", authorization, workspaceId: identity.workspace_id });
      return [session, identity] as const;
    } finally {
      this.verifying--;
    }
  }

  private session(account: string, origin: URL, access: Access): Session {
    return {
      ...access,
      account,
      binding: randomBytes(16).toString("base64url"),
      origin: origin.origin,
      expiresAt: Date.now() + sessionSeconds * 1000,
    };
  }
}

export function checkedConsoleIdentity(value: unknown): User {
  if (!value || typeof value !== "object")
    throw new HttpError(502, "无法读取工作区身份");
  const { id, workspace_id, role } = value as Record<string, unknown>;
  if (
    typeof id !== "string" || !workspaceIdPattern.test(id) ||
    typeof workspace_id !== "string" || !workspaceIdPattern.test(workspace_id) ||
    typeof role !== "string"
  )
    throw new HttpError(502, "无法读取工作区身份");
  return {
    id,
    workspace_id: workspace_id.toLowerCase(),
    role,
  };
}
