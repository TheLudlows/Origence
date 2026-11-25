import { randomBytes, scrypt, timingSafeEqual } from "node:crypto";
import { closeSync, mkdirSync, openSync } from "node:fs";
import { dirname } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";

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

const applicationId = 0x4f434143;
const schemaVersion = 1;
const sqliteOptions = { enableForeignKeyConstraints: true, timeout: 5000 };
const keyEnvPattern = /^[A-Za-z_][A-Za-z0-9_]*$/;
const maxGrants = 32;

export function consoleAccountsPath(override: string | undefined) {
  return override || fileURLToPath(new URL("../../.data/console-accounts.db", import.meta.url));
}

function hasErrorCode(error: unknown, code: string) {
  return error instanceof Error && "code" in error && error.code === code;
}

export function loadConfiguredConsoleAccounts(env: NodeJS.ProcessEnv): readonly ConsoleAccount[] {
  const override = env.ORIGENCE_UI_ACCOUNTS_DB;
  try {
    return loadConsoleAccounts(consoleAccountsPath(override), env);
  } catch (error) {
    if (!override && hasErrorCode(error, "ENOENT")) return [];
    throw error;
  }
}

export function prepareConsoleAccounts(path: string) {
  try {
    initializeConsoleAccounts(path);
  } catch (error) {
    if (!hasErrorCode(error, "EEXIST")) throw error;
  }
}

export type ConsoleAccountChange =
  | { kind: "set-password"; username: string; passwordHash: string }
  | { kind: "grant"; username: string; workspaceId: string; name: string; keyEnv: string }
  | { kind: "revoke"; username: string; workspaceId: string }
  | { kind: "remove-account"; username: string };

export function initializeConsoleAccounts(path: string) {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  closeSync(openSync(path, "wx", 0o600));
  const db = new DatabaseSync(path, sqliteOptions);
  try {
    db.exec(`
      BEGIN IMMEDIATE;
      CREATE TABLE oc_console_accounts (
        username TEXT PRIMARY KEY,
        password_hash TEXT NOT NULL
      ) STRICT;
      CREATE TABLE oc_console_workspace_grants (
        username TEXT NOT NULL REFERENCES oc_console_accounts(username) ON DELETE CASCADE,
        workspace_id TEXT NOT NULL,
        name TEXT NOT NULL,
        key_env TEXT NOT NULL UNIQUE,
        PRIMARY KEY (username, workspace_id)
      ) STRICT;
      PRAGMA application_id = ${applicationId};
      PRAGMA user_version = ${schemaVersion};
      COMMIT;
    `);
  } finally {
    db.close();
  }
}

function withAccountsDatabase<T>(path: string, readOnly: boolean, run: (db: DatabaseSync) => T): T {
  // Check that the file already exists before opening a writable connection.
  closeSync(openSync(path, "r"));
  const db = new DatabaseSync(path, { ...sqliteOptions, readOnly });
  try {
    if (
      db.prepare("PRAGMA application_id").get()?.application_id !== applicationId ||
      db.prepare("PRAGMA user_version").get()?.user_version !== schemaVersion
    ) throw new Error("Invalid or incompatible Origence console account database");
    return run(db);
  } finally {
    db.close();
  }
}

function validAccount(username: unknown, passwordHash: unknown) {
  return typeof username === "string" && accountPattern.test(username) &&
    typeof passwordHash === "string" && hashPattern.test(passwordHash);
}

function validGrant(workspaceId: unknown, name: unknown, keyEnv: unknown) {
  return typeof workspaceId === "string" && workspaceIdPattern.test(workspaceId) &&
    typeof name === "string" && !!name.trim() && name.length <= 120 &&
    typeof keyEnv === "string" && keyEnvPattern.test(keyEnv);
}

export function configureConsoleAccount(path: string, change: ConsoleAccountChange): void {
  const { username } = change;
  if (!accountPattern.test(username)) throw new Error("Invalid console username");
  withAccountsDatabase(path, false, (db) => {
    switch (change.kind) {
      case "set-password":
        if (!validAccount(username, change.passwordHash)) throw new Error("Invalid console password hash");
        db.prepare(`
          INSERT INTO oc_console_accounts (username, password_hash) VALUES (?, ?)
          ON CONFLICT (username) DO UPDATE SET password_hash = excluded.password_hash
        `).run(username, change.passwordHash);
        break;
      case "grant":
        if (!validGrant(change.workspaceId, change.name, change.keyEnv))
          throw new Error("Invalid console workspace grant");
        db.exec("BEGIN IMMEDIATE");
        try {
          db.prepare(`
            INSERT INTO oc_console_workspace_grants (username, workspace_id, name, key_env)
            VALUES (?, ?, ?, ?)
            ON CONFLICT (username, workspace_id) DO UPDATE SET name = excluded.name, key_env = excluded.key_env
          `).run(username, change.workspaceId.toLowerCase(), change.name.trim(), change.keyEnv);
          const count = db.prepare(
            "SELECT COUNT(*) AS count FROM oc_console_workspace_grants WHERE username = ?",
          ).get(username)?.count;
          if (typeof count !== "number" || count > maxGrants)
            throw new Error(`A console account can have at most ${maxGrants} workspace grants`);
          db.exec("COMMIT");
        } catch (error) {
          db.exec("ROLLBACK");
          throw error;
        }
        break;
      case "revoke":
        if (!workspaceIdPattern.test(change.workspaceId)) throw new Error("Invalid workspace ID");
        db.prepare("DELETE FROM oc_console_workspace_grants WHERE username = ? AND workspace_id = ?")
          .run(username, change.workspaceId.toLowerCase());
        break;
      case "remove-account":
        db.prepare("DELETE FROM oc_console_accounts WHERE username = ?").run(username);
        break;
    }
  });
}

export function loadConsoleAccounts(path: string, env: NodeJS.ProcessEnv): readonly ConsoleAccount[] {
  return withAccountsDatabase(path, true, (db) => {
    db.exec("BEGIN");
    const accounts = new Map<string, ConsoleAccount & { grants: ConsoleGrant[] }>();
    for (const row of db.prepare("SELECT username, password_hash FROM oc_console_accounts ORDER BY username").all()) {
      const { username, password_hash: passwordHash } = row;
      if (typeof username !== "string" || typeof passwordHash !== "string" || !validAccount(username, passwordHash))
        throw new Error("Invalid entry in the console account table");
      accounts.set(username, { username, passwordHash, grants: [] });
    }
    const keys = new Set<string>();
    for (const row of db.prepare(
      "SELECT username, workspace_id, name, key_env FROM oc_console_workspace_grants ORDER BY username, workspace_id",
    ).all()) {
      const { username, workspace_id: workspaceId, name, key_env: keyEnv } = row;
      if (
        typeof username !== "string" || typeof workspaceId !== "string" ||
        typeof name !== "string" || typeof keyEnv !== "string" || !validGrant(workspaceId, name, keyEnv)
      ) throw new Error("Invalid entry in the console workspace grant table");
      const account = accounts.get(username);
      const key = env[keyEnv];
      if (!account || account.grants.length >= maxGrants || workspaceId !== workspaceId.toLowerCase())
        throw new Error("Invalid entry in the console workspace grant table");
      if (!key || !keyPattern.test(key) || keys.has(key))
        throw new Error("Each console workspace grant requires its own API key");
      keys.add(key);
      account.grants.push({ workspaceId, name, authorization: `Bearer ${key}` });
    }
    db.exec("COMMIT");
    return [...accounts.values()];
  });
}
