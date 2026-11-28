import { randomBytes, scrypt, timingSafeEqual } from "node:crypto";
import { closeSync, mkdirSync, openSync } from "node:fs";
import { dirname } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";

export const workspaceIdPattern = /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i;
export const deploymentKeyPattern = /^oc_admin_[a-f0-9]{64}$/;
const accountPattern = /^[a-zA-Z0-9_.@-]{1,64}$/;
const hashPattern = /^scrypt-v1\$([a-f0-9]{32})\$([a-f0-9]{64})$/;
const scryptOptions = { N: 32768, r: 8, p: 3, maxmem: 64 * 1024 * 1024 };

export interface ConsoleAccount {
  username: string;
  passwordHash: string;
  deploymentAuthorization: string;
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
export function consoleAccountsPath(override: string | undefined) {
  return override || fileURLToPath(new URL("../../.data/console-accounts.db", import.meta.url));
}

function hasErrorCode(error: unknown, code: string) {
  return error instanceof Error && "code" in error && error.code === code;
}

export function loadConfiguredConsoleAccounts(env: NodeJS.ProcessEnv): readonly ConsoleAccount[] {
  const key = env.ORIGENCE_UI_DEPLOYMENT_ADMIN_KEY;
  if (!key) return [];
  if (!deploymentKeyPattern.test(key))
    throw new Error("Invalid deployment administrator key");
  const accounts = loadConsoleAccounts(consoleAccountsPath(env.ORIGENCE_UI_ACCOUNTS_DB));
  const admin = accounts.find(({ username }) => username === "admin");
  if (!admin) throw new Error("Deployment administrator login requires a configured admin account");
  return [{ ...admin, deploymentAuthorization: `Bearer ${key}` }];
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
      case "remove-account":
        db.prepare("DELETE FROM oc_console_accounts WHERE username = ?").run(username);
        break;
    }
  });
}

export function loadConsoleAccounts(path: string): readonly Omit<ConsoleAccount, "deploymentAuthorization">[] {
  return withAccountsDatabase(path, true, (db) =>
    db.prepare("SELECT username, password_hash FROM oc_console_accounts ORDER BY username").all().map((row) => {
      const { username, password_hash: passwordHash } = row;
      if (typeof username !== "string" || typeof passwordHash !== "string" || !validAccount(username, passwordHash))
        throw new Error("Invalid entry in the console account table");
      return { username, passwordHash };
    }));
}
