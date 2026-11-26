import { spawn } from "node:child_process";
import { existsSync, mkdirSync, openSync, closeSync, writeFileSync } from "node:fs";
import { connect } from "node:net";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import { check } from "./platform.mjs";

const worker = fileURLToPath(import.meta.url);
const serviceSpec = "ORIGENCE_INSTALLER_SERVICE";

function supervise() {
  const { file, args, cwd, dir } = JSON.parse(process.env[serviceSpec]);
  delete process.env[serviceSpec];
  const child = spawn(file, args, { cwd, stdio: ["ignore", "inherit", "inherit"], windowsHide: true });
  let requestedStop = false;
  let force;
  const poll = setInterval(() => {
    if (requestedStop || !existsSync(join(dir, "stop"))) return;
    requestedStop = true;
    child.kill();
    force = setTimeout(() => child.kill("SIGKILL"), 15_000);
  }, 200);
  const complete = () => {
    clearInterval(poll);
    clearTimeout(force);
    writeFileSync(join(dir, "stopped"), "", { mode: 0o600 });
  };
  child.on("error", () => {
    if (child.pid === undefined) complete();
    else console.error("Origence installer could not signal its managed process.");
  });
  child.once("exit", complete);
}

export function listening(url) {
  return new Promise(resolve => {
    const port = Number(url.port || (url.protocol === "https:" ? 443 : 80));
    const socket = connect({ host: url.hostname.replace(/^\[|\]$/g, ""), port });
    const finish = (open) => { socket.destroy(); resolve(open); };
    socket.setTimeout(1000, () => finish(false));
    socket.once("connect", () => finish(true));
    socket.once("error", () => finish(false));
  });
}

export function startService(state, name, file, args, cwd, env) {
  check(!state.value.services?.[name], "Previous managed process still has an active supervisor.");
  const dir = join(state.dir, name + "-" + randomUUID());
  mkdirSync(dir, { mode: 0o700 });
  state.value.services ??= {};
  state.value.services[name] = dir;
  state.save();
  const log = openSync(join(dir, "service.log"), "wx", 0o600);
  try {
    const command = { file, args, cwd, dir };
    const processWorker = spawn(process.execPath, [worker], {
      env: { ...env, [serviceSpec]: JSON.stringify(command) },
      detached: true, stdio: ["ignore", log, log], windowsHide: true,
    });
    processWorker.once("error", () => writeFileSync(join(dir, "stopped"), "", { mode: 0o600 }));
    processWorker.unref();
  } finally {
    closeSync(log);
  }
  return dir;
}

export function activeService(state, name) {
  const dir = state.value.services?.[name];
  return Boolean(dir && !existsSync(join(dir, "stopped")));
}

export async function stopService(state, name) {
  const dir = state.value.services?.[name];
  if (!dir) return false;
  if (!existsSync(join(dir, "stopped"))) {
    writeFileSync(join(dir, "stop"), "", { mode: 0o600 });
    for (let attempt = 0; attempt < 100 && !existsSync(join(dir, "stopped")); attempt++) await delay(200);
  }
  check(existsSync(join(dir, "stopped")),
    `The installer could not confirm the managed ${name} stopped. Inspect its private service.log and stop it before retrying.`);
  delete state.value.services[name];
  state.save();
  return true;
}

export async function waitForService(base, path, dir, name) {
  for (let attempt = 0; attempt < 180; attempt++) {
    if (dir && existsSync(join(dir, "stopped"))) break;
    try {
      const response = await fetch(base + path, { redirect: "manual", signal: AbortSignal.timeout(1000) });
      await response.body?.cancel();
      if (response.status === 200) return;
    } catch {
      // The service can accept sockets before its routes become ready.
    }
    await delay(250);
  }
  throw new Error(`${name} did not become ready. Check the address, port and private service.log if the installer started it.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) supervise();
