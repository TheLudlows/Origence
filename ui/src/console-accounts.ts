import { randomBytes, scrypt, timingSafeEqual } from "node:crypto";
import { readFile } from "node:fs/promises";

export const workspaceIdPattern = /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i;
const accountPattern = /^[a-zA-Z0-9_.@-]{1,64}$/;
const keyPattern = /^[\x21-\x7e]{1,512}$/;
const hashPattern = /^scrypt-v1\$([a-f0-9]{32})\$([a-f0-9]{64})$/;
const scryptOptions = { N: 32768, r: 8, p: 3, maxmem: 64 * 1024 * 1024 };

export interface ConsoleGrant {
  workspaceId: string;
  name: string;
  authorization: string;
}
export interface ConsoleAccount {
  username: string;
  passwordHash: string;
  grants: readonly ConsoleGrant[];
}

function derive(password: string, salt: Buffer): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    scrypt(password, salt, 32, scryptOptions, (error, hash) => {
      if (error) reject(error);
      else resolve(hash);
    });
  });
}

export function validPasswordInput(value: unknown): value is string {
  return typeof value === "string" && Buffer.byteLength(value) <= 1024;
}

export async function hashConsolePassword(password: string): Promise<string> {
  if (!validPasswordInput(password) || [...password].length < 15)
    throw new Error("Use at least 15 characters and no more than 1024 UTF-8 bytes.");
  const salt = randomBytes(16);
  const hash = await derive(password, salt);
  return `scrypt-v1$${salt.toString("hex")}$${hash.toString("hex")}`;
}

export async function passwordMatches(password: string, stored: string): Promise<boolean> {
  const match = hashPattern.exec(stored);
  if (!match) throw new Error("Invalid console password hash");
  const result = await derive(password, Buffer.from(match[1]!, "hex"));
  return timingSafeEqual(result, Buffer.from(match[2]!, "hex"));
}

function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("Expected an object in the console account file");
  return value as Record<string, unknown>;
}

export async function loadConsoleAccounts(
  path: string,
  env: NodeJS.ProcessEnv,
): Promise<readonly ConsoleAccount[]> {
  const data = object(JSON.parse(await readFile(path, "utf8")));
  if (!Array.isArray(data.accounts) || data.accounts.length === 0)
    throw new Error("The console account file must define at least one account");
  const users = new Set<string>();
  const keys = new Set<string>();
  return data.accounts.map((value: unknown) => {
    const { username, password_hash, workspaces } = object(value);
    if (
      typeof username !== "string" || !accountPattern.test(username) ||
      users.has(username) ||
      typeof password_hash !== "string" || !hashPattern.test(password_hash) ||
      !Array.isArray(workspaces) || workspaces.length > 32
    )
      throw new Error("Invalid or duplicate account in the console account file");
    users.add(username);
    const ids = new Set<string>();
    const grants = workspaces.map((item: unknown) => {
      const { workspace_id, name, key_env } = object(item);
      if (
        typeof workspace_id !== "string" || !workspaceIdPattern.test(workspace_id) ||
        typeof name !== "string" || !name.trim() || name.length > 120 ||
        typeof key_env !== "string" || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(key_env)
      )
        throw new Error("Invalid workspace grant in the console account file");
      const workspaceId = workspace_id.toLowerCase();
      const key = env[key_env];
      if (ids.has(workspaceId) || !key || !keyPattern.test(key) || keys.has(key))
        throw new Error("Each console workspace grant requires its own API key and workspace ID");
      ids.add(workspaceId);
      keys.add(key);
      return { workspaceId, name: name.trim(), authorization: `Bearer ${key}` };
    });
    return { username, passwordHash: password_hash, grants };
  });
}
