import { parseArgs } from "node:util";
import {
  configureConsoleAccount,
  consoleAccountsPath,
  hashConsolePassword,
  initializeConsoleAccounts,
  prepareConsoleAccounts,
} from "../dist/console-accounts.js";

const usage = `Usage: node scripts/console-accounts.mjs [--db PATH] COMMAND
  init
  set-password USERNAME                 read password from stdin
  grant USERNAME WORKSPACE_ID NAME KEY_ENV
  revoke USERNAME WORKSPACE_ID
  remove-account USERNAME`;

async function readPassword() {
  const chunks = [];
  let bytes = 0;
  for await (const chunk of process.stdin) {
    bytes += chunk.length;
    if (bytes > 1026) throw new Error("Password input is too long.");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString("utf8").replace(/\r?\n$/, "");
}

try {
  const { values, positionals } = parseArgs({
    options: { db: { type: "string" } },
    allowPositionals: true,
  });
  const [command, username, workspaceId, name, keyEnv] = positionals;
  const path = consoleAccountsPath(values.db ?? process.env.ORIGENCE_UI_ACCOUNTS_DB);
  const length = positionals.length;
  if (command === "init" && length === 1) initializeConsoleAccounts(path);
  else if (command === "set-password" && length === 2) {
    const passwordHash = await hashConsolePassword(await readPassword());
    prepareConsoleAccounts(path);
    configureConsoleAccount(path, { kind: command, username, passwordHash });
  } else if (command === "grant" && length === 5)
    configureConsoleAccount(path, { kind: command, username, workspaceId, name, keyEnv });
  else if (command === "revoke" && length === 3)
    configureConsoleAccount(path, { kind: command, username, workspaceId });
  else if (command === "remove-account" && length === 2)
    configureConsoleAccount(path, { kind: command, username });
  else throw new Error(usage);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
