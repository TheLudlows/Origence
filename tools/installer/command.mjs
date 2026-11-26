import { closeSync, existsSync, openSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline/promises";
import { Writable } from "node:stream";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { atomicWrite, check, executable, execute, home, outsideRepository, privateDirectory, windows, windowsSettings } from "./platform.mjs";

export const repo = resolve(fileURLToPath(new URL("../../", import.meta.url)));
export const grantKey = "ORIGENCE_DEMO_KEY";
export const defaultEndpoint = "http://127.0.0.1:8080";
export const status = (step, state, detail) => console.log(JSON.stringify({ step, status: state, detail }));
export const tokenPattern = /^[\x21-\x7e]{1,512}$/;

function options(name, strings, flags) {
  const args = process.argv.slice(2);
  const command = ["install", "uninstall"].includes(args[0]) ? args.shift() : undefined;
  let values;
  try {
    ({ values } = parseArgs({ args, options: Object.fromEntries([
      ...strings.map(name => [name, { type: "string" }]),
      ...["from-env", "force", "uninstall", "help", ...flags].map(name => [name, { type: "boolean" }]),
    ]) }));
  } catch {
    throw new Error(`Invalid arguments. Run ${name} --help.`);
  }
  check(command !== "install" || !values.uninstall, `Use ${name} uninstall to roll back the installation.`);
  if (command === "uninstall" || values.uninstall) {
    check(Object.keys(values).every(name => name === "uninstall" || name === "help"),
      "Uninstall accepts no setup options.");
    values.uninstall = true;
  }
  return values;
}

export async function ask(prompt, secret = false) {
  const output = secret ? new Writable({ write(_chunk, _encoding, done) { done(); } }) : process.stderr;
  const terminal = createInterface({ input: process.stdin, output, terminal: true });
  try {
    if (secret) process.stderr.write(prompt);
    return await terminal.question(secret ? "" : prompt, { signal: AbortSignal.timeout(300_000) });
  } finally {
    terminal.close();
    if (secret) process.stderr.write("\n");
  }
}

function stateDirectory() {
  const parent = windows ? process.env.LOCALAPPDATA || join(home, "AppData", "Local") : home;
  return join(parent, windows ? "Origence" : ".origence", "installer");
}

function installerState(dir, scope) {
  const path = join(dir, "state.json");
  const old = existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : { version: 2 };
  check(old.version === 1 || old.version === 2, "This Origence installer state version is unsupported.");
  let state = old;
  if (old.version === 1) {
    // Keep the paths of existing services and shell files. Only split ownership
    // in the journal so each command can undo its own changes independently.
    const { version, claude, shell, registry, ...deploy } = old;
    state = { version: 2, deploy };
    if (claude || shell || registry) {
      const config = old.config && Object.fromEntries(
        ["endpoint", "token", "workspace", "key", "binary"].map(name => [name, old.config[name]]));
      state.agent = { claude, shell, registry, config };
    }
  }
  const value = state[scope] ?? {};
  const save = () => atomicWrite(path, JSON.stringify(state, null, 2) + "\n");
  return {
    dir,
    value,
    get installed() { return Boolean(state[scope]); },
    save() {
      state[scope] = value;
      save();
    },
    clear() {
      delete state[scope];
      if (state.agent || state.deploy) save();
      else rmSync(path, { force: true });
    },
  };
}

function lock(dir) {
  const file = join(dir, "install.lock");
  try { writeFileSync(file, String(process.pid), { flag: "wx", mode: 0o600 }); }
  catch (error) {
    if (error.code !== "EEXIST") throw error;
    throw new Error("Another installation is active. After confirming it exited, remove the installer directory's install.lock and retry.");
  }
  return () => rmSync(file, { force: true });
}

export function withoutWorkspaceKey(env) {
  return Object.fromEntries(Object.entries(env).filter(([name]) =>
    !["OC_API_KEY", grantKey].includes(name.toUpperCase())));
}

export function npm(...args) {
  const program = executable("npm");
  check(program, "npm is required to build the console or provision a client. Install Node.js 24 with npm.");
  execute(program, args, {
    cwd: repo, env: withoutWorkspaceKey(process.env), timeout: 300_000, diagnosis: "npm could not provision the dependency. Check network and registry access",
  });
}

export function origenceBinary(selected) {
  if (selected) {
    const binary = executable(resolve(selected));
    check(binary, "The requested Origence executable could not be found. Check --origence.");
    return binary;
  }
  for (const file of [
    executable("origence"), executable(join(repo, "target", "release", "origence")),
    executable(join(repo, "target", "debug", "origence")),
  ]) if (file) return file;
  const cargo = executable("cargo");
  check(cargo, "Origence is missing. Provide --origence, or install the Rust toolchain and build dependencies.");
  const target = resolve(process.env.CARGO_TARGET_DIR ?? join(repo, "target"));
  execute(cargo, ["build", "--locked", "--bin", "origence", "--target-dir", target], {
    cwd: repo, env: withoutWorkspaceKey(process.env), timeout: 1_800_000, diagnosis: "Origence could not be built. Check the Rust toolchain, native build tools and Cargo registry",
  });
  const binary = executable(join(target, "debug", "origence"));
  check(binary, "Cargo did not produce the expected Origence binary. Pass its location with --origence.");
  return binary;
}

export function saveCredentials(path, credentials) {
  const file = resolve(path);
  outsideRepository(file);
  closeSync(openSync(file, "wx", 0o600));
  try {
    if (windows) windowsSettings({ action: "protect", path: file, file: true });
    writeFileSync(file, JSON.stringify(credentials, null, 2) + "\n");
  } catch (error) {
    rmSync(file, { force: true });
    throw error;
  }
  status("credentials-file", "success", "Private credential file written. Keep it outside Git and do not share it.");
}

export function credentialsPath(value) {
  if (!value) return;
  const file = resolve(value);
  outsideRepository(file);
  check(!existsSync(file) && existsSync(dirname(file)) && statSync(dirname(file)).isDirectory(),
    "Choose a new credential file with an existing parent directory.");
  return file;
}

export async function rollback(state, steps, detail) {
  const failures = [];
  for (const [step, undo] of Object.entries(steps)) {
    try { await undo(); }
    catch (error) {
      failures.push(step);
      status("rollback-" + step, "failure", error.message);
    }
  }
  check(!failures.length, "Uninstall kept the rollback journal. Resolve the reported steps and retry.");
  state.clear();
  status("uninstall", "success", detail);
}

export async function runInstaller({ name, scope, usage, strings, flags, install, uninstall }) {
  try {
    const args = options(name, strings, flags);
    if (args.help) {
      console.log(usage);
      return;
    }
    check(Number(process.versions.node.split(".")[0]) >= 24, "Use Node.js 24 or later.");
    if (args["from-env"]) check(!args.endpoint && !args.token, "Use --from-env separately from --endpoint and --token.");
    const dir = stateDirectory();
    outsideRepository(dir);
    if (args.uninstall && !existsSync(join(dir, "state.json"))) {
      status("uninstall", "skipped", "No settings were found for this command.");
      return;
    }
    privateDirectory(dir);
    const release = lock(dir);
    try {
      const state = installerState(dir, scope);
      if (!args.uninstall) await install(state, args);
      else if (state.installed) await uninstall(state);
      else status("uninstall", "skipped", "No settings were found for this command.");
    } finally { release(); }
  } catch (error) {
    status("installation", "failure", error.message);
    process.exitCode = 1;
  }
}
