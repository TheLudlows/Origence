import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
  chmodSync, existsSync, lstatSync, mkdirSync, readFileSync, realpathSync,
  renameSync, rmSync, statSync, writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { basename, delimiter, dirname, join, resolve } from "node:path";

export const windows = process.platform === "win32";
export const home = homedir();
const helper = new URL("./windows.ps1", import.meta.url);
const shellMarker = "# Origence installer managed environment";

export function check(condition, message) {
  if (!condition) throw new Error(message);
}

function powershell(script, input, options = {}) {
  // Passing the static program as an encoded command works under the default
  // Windows PowerShell policy. Use the UTF-8 pipe directly: a hidden process
  // may have no console whose code page can be changed.
  const program = `
$ErrorActionPreference = 'Stop'
$utf8 = [Text.UTF8Encoding]::new($false)
$reader = [IO.StreamReader]::new([Console]::OpenStandardInput(), $utf8)
$request = $reader.ReadToEnd() | ConvertFrom-Json
${script}`;
  return spawnSync("powershell.exe", [
    "-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand",
    Buffer.from(program, "utf16le").toString("base64"),
  ], { ...options, input: JSON.stringify(input), encoding: "utf8", windowsHide: true });
}

export function windowsSettings(input) {
  const result = powershell(readFileSync(helper, "utf8"), input, { timeout: 20_000 });
  check(result.status === 0, "Cannot update the Windows user environment or protect the installer directory. Check the current user's registry and directory permissions.");
  return result.stdout.trim() ? JSON.parse(result.stdout) : undefined;
}

export function execute(file, args, { cwd, env, input, timeout = 60_000, diagnosis } = {}) {
  const options = { cwd, env, input, timeout, encoding: "utf8", windowsHide: true, maxBuffer: 4 * 1024 * 1024 };
  const wrapper = `
$arguments = @($request.arguments)
& ([string]$request.file) @arguments
exit $LASTEXITCODE
`;
  const result = windows && /\.(cmd|bat)$/i.test(file)
    ? powershell(wrapper, { file, arguments: args }, options)
    : spawnSync(file, args, options);
  if (result.status !== 0) {
    const cause = result.error?.code === "ETIMEDOUT" ? "timed out"
      : result.error?.code === "ENOENT" ? "executable not found"
      : result.status === null ? "could not start" : `exit ${result.status}`;
    // Never relay tools' raw output. Claude can print expanded MCP headers.
    throw new Error(`${diagnosis ?? basename(file)} (${cause}).`);
  }
  return result.stdout;
}

export function executable(name, path = process.env.PATH ?? "") {
  const extensions = windows && !/\.(exe|cmd|bat)$/i.test(name) ? [".exe", ".cmd", ".bat"] : [""];
  const candidates = /[/\\]/.test(name) ? [name]
    : path.split(delimiter).filter(Boolean).map(dir => join(dir, name));
  for (const candidate of candidates) {
    for (const extension of extensions) {
      const file = resolve(candidate + extension);
      try {
        const stat = statSync(file);
        if (stat.isFile() && (windows || stat.mode & 0o111)) return file;
      } catch (error) {
        if (!["ENOENT", "ENOTDIR"].includes(error.code)) throw error;
      }
    }
  }
}

export function privateDirectory(path) {
  mkdirSync(path, { recursive: true, mode: 0o700 });
  const info = lstatSync(path);
  check(info.isDirectory() && !info.isSymbolicLink() && (windows || info.uid === process.getuid()),
    "Installer state must be a directory owned by the current user.");
  if (windows) windowsSettings({ action: "protect", path });
  else chmodSync(path, 0o700);
}

export function outsideRepository(path) {
  function checkParents(start) {
    for (let current = start; ; current = dirname(current)) {
      const dotGit = join(current, ".git");
      const gitFile = existsSync(dotGit) && statSync(dotGit).isFile();
      check(!gitFile && !existsSync(join(dotGit, "HEAD")),
        "Choose a path outside your Git checkout for credentials or installer state.");
      if (current === dirname(current)) return;
    }
  }
  let existing = resolve(path);
  while (!existsSync(existing)) existing = dirname(existing);
  checkParents(resolve(path));
  checkParents(realpathSync(existing));
}

export function atomicWrite(path, content, mode = 0o600) {
  const temporary = join(dirname(path), `.origence-${randomUUID()}.tmp`);
  try {
    writeFileSync(temporary, content, { mode, flag: "wx" });
    renameSync(temporary, path);
  } finally {
    rmSync(temporary, { force: true });
  }
}

function splitPath(value) {
  return (value ?? "").split(delimiter).filter(Boolean);
}

const pathKey = entry => windows ? entry.replace(/^"|"$/g, "").replace(/[\\/]+$/, "").toLowerCase() : entry;
export function prependPath(value, dirs) {
  const keys = new Set(dirs.map(pathKey));
  return [...dirs, ...splitPath(value).filter(entry => !keys.has(pathKey(entry)))].join(delimiter);
}

function shellQuote(value) {
  return "'" + value.replaceAll("'", "'\\''") + "'";
}

function shellFiles() {
  const login = [".bash_profile", ".bash_login", ".profile"].find(name => existsSync(join(home, name))) ?? ".profile";
  const zshDirectory = process.env.ZDOTDIR || home;
  return [...new Set([join(home, ".bashrc"), join(home, login), join(zshDirectory, ".zshrc"), join(zshDirectory, ".zprofile")])];
}

function shellFile(path) {
  // Preserve dotfile-manager symlinks. The actual target is recorded for rollback.
  return existsSync(path) ? realpathSync(path) : path;
}

function setShellEnvironment(state, vars, dirs) {
  const environment = join(state.dir, "environment.sh");
  const lines = Object.entries(vars).map(([key, value]) => `export ${key}=${shellQuote(value)}`);
  const prefix = shellQuote(dirs.join(delimiter));
  lines.push(`case "$PATH" in ${prefix}:*|${prefix}) ;; *) PATH=${prefix}\${PATH:+":$PATH"} ;; esac`);
  lines.push("export PATH");
  atomicWrite(environment, lines.join("\n") + "\n");
  const block = `${shellMarker}\n[ ! -r ${shellQuote(environment)} ] || . ${shellQuote(environment)}\n# End Origence installer managed environment\n`;
  for (const path of new Set(shellFiles().map(shellFile))) {
    const present = existsSync(path);
    if (present) check(statSync(path).isFile(), "Shell startup path must be a regular file.");
    else check(existsSync(dirname(path)), "The shell startup folder does not exist. Check ZDOTDIR.");
    const original = present ? readFileSync(path, "utf8") : "";
    const existing = state.value.shell?.find(item => item.path === path);
    if (existing && original.includes(existing.added)) continue;
    check(!original.includes(shellMarker), "A shell startup file has an Origence block from a different installation. Restore or uninstall that installation first.");
    const added = (original && !original.endsWith("\n") ? "\n" : "") + block;
    state.value.shell ??= [];
    if (existing) Object.assign(existing, { added, created: !present });
    else state.value.shell.push({ path, added, created: !present });
    state.save();
    atomicWrite(path, original + added, present ? statSync(path).mode & 0o777 : 0o600);
  }
}

export function writeUserEnvironment(state, vars, dirs) {
  if (!windows) return setShellEnvironment(state, vars, dirs);
  const names = [...Object.keys(vars), "Path"];
  const current = windowsSettings({ action: "read", names });
  const journal = state.value.registry ??= {};
  const machine = windowsSettings({ action: "machine-path" }).value;
  const userPath = prependPath(current.Path?.value, dirs);
  const effectivePath = [machine, userPath].filter(Boolean).join(delimiter);
  for (const name of ["origence", "origence-setup"]) {
    check(executable(name, effectivePath) === executable(name, userPath),
      `A different ${name} is earlier in the Windows machine PATH. Fix its priority before installing.`);
  }
  const installed = Object.fromEntries(Object.entries(vars).map(([name, value]) => [name, { value, kind: "String" }]));
  installed.Path = { value: userPath, kind: current.Path?.kind ?? "ExpandString" };
  for (const name of names) {
    const previous = journal[name];
    const before = previous ? previous.before : current[name];
    journal[name] = { before, installed: installed[name] };
    if (name === "Path") {
      journal.Path.added = [...new Set([...(previous?.added ?? []),
        ...dirs.filter(dir => !splitPath(before?.value).some(entry => pathKey(entry) === pathKey(dir)))])];
    }
  }
  state.save();
  windowsSettings({ action: "write", values: installed });
}

export function restoreUserEnvironment(state) {
  if (windows && state.value.registry) {
    const journal = state.value.registry;
    const current = windowsSettings({ action: "read", names: Object.keys(journal) });
    const values = {};
    for (const [name, record] of Object.entries(journal)) {
      if (current[name]?.value === record.installed?.value && current[name]?.kind === record.installed?.kind) {
        values[name] = record.before;
      } else if (name === "Path" && current.Path && record.added.length) {
        const keys = new Set(record.added.map(pathKey));
        const filtered = splitPath(current.Path.value).filter(entry => !keys.has(pathKey(entry)));
        values.Path = { ...current.Path, value: filtered.join(delimiter) };
      }
    }
    windowsSettings({ action: "write", values });
  }
  for (const { path, added, created } of state.value.shell ?? []) {
    if (!existsSync(path)) continue;
    check(!lstatSync(path).isSymbolicLink() && statSync(path).isFile(), "A shell startup file changed type. Restore its Origence block manually before retrying uninstall.");
    const text = readFileSync(path, "utf8");
    if (!text.includes(added)) continue;
    const remaining = text.replace(added, "");
    if (created && !remaining) rmSync(path);
    else atomicWrite(path, remaining, statSync(path).mode & 0o777);
  }
  rmSync(join(state.dir, "environment.sh"), { force: true });
}
