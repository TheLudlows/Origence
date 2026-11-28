import {
  closeSync, constants, fstatSync, fsyncSync, lstatSync, mkdirSync, openSync,
  readSync, realpathSync, renameSync, unlinkSync, writeFileSync,
} from "node:fs";
import { dirname, isAbsolute, join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { under } from "./protocol.mjs";

function privateFile(info, directory = false) {
  if (!(directory ? info.isDirectory() : info.isFile()) ||
      process.platform !== "win32" && (info.mode & 0o077) !== 0) {
    throw new Error("invalid private spool entry");
  }
}

export function spoolDirectory(data, project, name) {
  if (!isAbsolute(data)) throw new Error("invalid plugin storage path");
  let existing = data;
  while (!lstatSync(existing, { throwIfNoEntry: false })) {
    const parent = dirname(existing);
    if (parent === existing) throw new Error("unavailable plugin storage");
    existing = parent;
  }
  if (under(project, realpathSync.native(existing))) throw new Error("storage inside project");
  mkdirSync(data, { recursive: true, mode: 0o700 });
  const parent = realpathSync.native(data);
  if (under(project, parent)) throw new Error("storage inside project");
  const path = join(parent, name);
  try {
    mkdirSync(path, { mode: 0o700 });
    syncDirectory(parent);
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
  }
  privateFile(lstatSync(path), true);
  return path;
}

function syncDirectory(directory) {
  if (process.platform === "win32") return;
  const fd = openSync(directory, "r");
  try { fsyncSync(fd); } finally { closeSync(fd); }
}

// Node supplies no portable advisory file lock. A short SQLite transaction in
// an otherwise empty file gives us a kernel-backed lock that dies with its
// process. The spool itself stays in the v1 JSON files.
function lock(directory, name, retries) {
  const path = join(directory, name + ".node-lock");
  const info = lstatSync(path, { throwIfNoEntry: false });
  if (info) privateFile(info);
  else {
    try { closeSync(openSync(path, "wx", 0o600)); }
    catch (error) { if (error.code !== "EEXIST") throw error; }
  }
  const db = new DatabaseSync(path, { timeout: retries });
  try {
    db.exec("BEGIN IMMEDIATE");
    return db;
  } catch (error) {
    db.close();
    if (error.errcode === 5 || error.errcode === 6) return null;
    throw error;
  }
}

export function spool(directory, filename, limit, initial, valid) {
  const path = join(directory, filename);
  function read() {
    const info = lstatSync(path, { throwIfNoEntry: false });
    if (!info) return initial();
    privateFile(info);
    const fd = openSync(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    try {
      const file = fstatSync(fd);
      privateFile(file);
      if (file.size > limit) throw new Error("spool exceeded byte limit");
      const bytes = Buffer.alloc(file.size + 1);
      let length = 0;
      while (length < bytes.length) {
        const count = readSync(fd, bytes, length, bytes.length - length, null);
        if (!count) break;
        length += count;
      }
      if (length !== file.size) throw new Error("spool changed while reading");
      const state = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(0, length)));
      if (!valid(state)) throw new Error("invalid spool state");
      return state;
    } finally { closeSync(fd); }
  }

  function update(mutate) {
    const guard = lock(directory, "state", 75);
    if (!guard) throw new Error("spool busy");
    try {
      const current = read();
      const result = mutate(current);
      const bytes = Buffer.from(JSON.stringify(current));
      if (bytes.length > limit || !valid(current)) throw new Error("spool exceeded limit");
      const temporary = path + ".node-stage";
      const previous = lstatSync(temporary, { throwIfNoEntry: false });
      if (previous) {
        privateFile(previous);
        unlinkSync(temporary);
      }
      const fd = openSync(temporary, "wx", 0o600);
      try {
        writeFileSync(fd, bytes);
        fsyncSync(fd);
      } finally { closeSync(fd); }
      renameSync(temporary, path);
      syncDirectory(directory);
      return result;
    } finally { guard.close(); }
  }

  async function send(deliver) {
    const guard = lock(directory, "sender", 0);
    if (!guard) return;
    try { return await deliver(); } finally { guard.close(); }
  }

  return { read, update, send };
}
