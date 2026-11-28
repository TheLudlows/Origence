import { realpathSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { boundedJson, origin, projectRoot, uuid } from "./protocol.mjs";
import { observation } from "./observations.mjs";
import { recordRun } from "./runs.mjs";
import { syncArchive } from "./archive.mjs";

export async function run(client) {
  try {
    if (!["codex", "claude-code"].includes(client)) throw new Error("invalid agent client");
    const env = process.env;
    const enabled = env.OC_PLUGIN_RUN_HOOK !== "0";
    const archive = env.OC_TURN_SYNC_PROJECT;
    if (!enabled && !archive) return;
    const token = env.OC_API_KEY;
    if (!token || !/^[\x21-\x7e]{1,1024}$/.test(token)) return;
    const data = env[client === "codex" ? "PLUGIN_DATA" : "CLAUDE_PLUGIN_DATA"];
    if (!data) throw new Error("plugin has no storage");
    const workspace = env.OC_WORKSPACE_ID === undefined ? undefined : uuid(env.OC_WORKSPACE_ID);
    if (workspace === null) throw new Error("invalid workspace selector");
    const base = origin(env.OC_SERVER_URL || "http://127.0.0.1:8080");
    const project = projectRoot(realpathSync.native(process.cwd()));
    const input = await boundedJson(process.stdin, 1024 * 1024);
    if (!input || typeof input.hook_event_name !== "string") return;
    const context = { client, token, workspace, data, base, project };
    await Promise.all([
      enabled && recordRun(context, input, observation(input, client)).catch(() => {
        process.stderr.write("Origence plugin hook: Run observation could not be delivered\n");
      }),
      syncArchive(context, input, archive)
        .catch(() => "synchronization could not finish")
        .then(notice => { if (notice) process.stderr.write("Origence turn sync: " + notice + "\n"); }),
    ]);
  } catch {
    process.stderr.write("Origence plugin hook: could not read the observation\n");
  } finally {
    process.stdout.write("{}\n");
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) void run(process.argv[2]);
