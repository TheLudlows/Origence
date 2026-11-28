import { createHash } from "node:crypto";
import { lstatSync, realpathSync, statSync } from "node:fs";
import { dirname, isAbsolute, join, relative, sep, toNamespacedPath } from "node:path";

const UUID = /^[a-f0-9]{8}-(?:[a-f0-9]{4}-){3}[a-f0-9]{12}$/i;
export const NIL = "00000000-0000-0000-0000-000000000000";
export const uuid = value => typeof value === "string" && UUID.test(value) && value !== NIL
  ? value.toLowerCase() : null;
export const clientId = value => typeof value === "string" && /^[a-zA-Z0-9_\-.:]{1,256}$/.test(value)
  ? value : null;
export const rawUuid = (value = NIL) => Buffer.from(value.replaceAll("-", ""), "hex");
export const pathBytes = path => Buffer.from(toNamespacedPath(path));
export const workspaceRole = role => ["writer", "admin", "workspace"].includes(role);

export function digest(domain, ...parts) {
  const hash = createHash("sha256").update(domain);
  const length = Buffer.alloc(8);
  for (const value of parts) {
    const bytes = Buffer.from(value);
    length.writeBigUInt64BE(BigInt(bytes.length));
    hash.update(length).update(bytes);
  }
  return hash.digest();
}

export function scopedId(domain, ...parts) {
  const bytes = digest(domain, ...parts).subarray(0, 16);
  bytes[6] = (bytes[6] & 15) | 128;
  bytes[8] = (bytes[8] & 63) | 128;
  const value = bytes.toString("hex");
  return [value.slice(0, 8), value.slice(8, 12), value.slice(12, 16),
    value.slice(16, 20), value.slice(20)].join("-");
}

export async function boundedJson(input, limit) {
  const chunks = [];
  let length = 0;
  for await (const chunk of input ?? []) {
    length += chunk.length;
    if (length > limit) throw new Error("input exceeded limit");
    chunks.push(chunk);
  }
  // Reject invalid UTF-8 instead of silently modifying the source text.
  return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(Buffer.concat(chunks)));
}

export function origin(input) {
  const url = new URL(input);
  const loopback = ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname);
  if (!(url.protocol === "https:" || loopback && url.protocol === "http:") ||
      url.username || url.password || url.pathname !== "/" || url.search || url.hash ||
      input.includes("?") || input.includes("#")) {
    throw new Error("invalid Origence origin");
  }
  return url.href;
}

export class HttpStatus extends Error {
  constructor(status) {
    super("host rejected request");
    this.status = status;
  }
}

export function host(base, token, workspace, signal) {
  return {
    withWorkspace: scope => host(base, token, scope, signal),
    async call(path, body, key) {
      const headers = { Authorization: "Bearer " + token };
      if (workspace) headers["X-Origence-Workspace"] = workspace;
      if (key) headers["Idempotency-Key"] = key;
      if (body !== undefined) headers["Content-Type"] = "application/json";
      const response = await fetch(new URL(path, base), {
        headers, signal, redirect: "error", method: body === undefined ? "GET" : "POST",
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const result = await boundedJson(response.body, 8 * 1024 * 1024);
      if (!response.ok) throw new HttpStatus(response.status);
      return result;
    },
  };
}

export async function identify(client, workspace) {
  const who = await client.call("/v1/whoami");
  const actual = uuid(who.workspace_id);
  if (!workspaceRole(who.role)) return null;
  if (!actual || workspace && workspace !== actual) throw new Error("workspace changed");
  const scoped = client.withWorkspace(actual);
  const session = await scoped.call("/v1/session");
  const logicalKey = uuid(session.key_id);
  if (uuid(session.workspace_id) !== actual || !logicalKey) throw new Error("session scope changed");
  return {
    host: scoped,
    target: { tenant: uuid(who.tenant_id), workspace: actual,
      logical_key: logicalKey, session: uuid(session.session_id) },
  };
}

export function under(parent, path) {
  const rest = relative(parent, path);
  return rest === "" || rest !== ".." && !rest.startsWith(".." + sep) && !isAbsolute(rest);
}

function gitMarker(path) {
  const git = join(path, ".git");
  return statSync(git, { throwIfNoEntry: false })?.isFile() ||
    statSync(join(git, "HEAD"), { throwIfNoEntry: false })?.isFile();
}

export function projectRoot(cwd) {
  for (let path = cwd; ; path = dirname(path)) {
    if (gitMarker(path)) return path;
    if (path === dirname(path)) return cwd;
  }
}

export function ownsDirectory(project, directory) {
  if (typeof directory !== "string" || !isAbsolute(directory)) return false;
  try {
    const actual = realpathSync.native(directory);
    if (!under(project, actual) || !statSync(actual).isDirectory()) return false;
    for (let path = actual; relative(project, path) !== ""; path = dirname(path)) {
      // Even an unrecognized nested .git marker is a boundary.
      if (lstatSync(join(path, ".git"), { throwIfNoEntry: false })) return false;
    }
    return true;
  } catch {
    return false;
  }
}
