import { api } from "./api.js";

export interface RunPrefix {
  run_id: string;
  through_sequence: number;
  digest: string;
}

export async function readRunPrefix(runId: string, sequence: number, signal: AbortSignal) {
  const prefix = await api.get<RunPrefix | null>(
    `/v1/runs/${encodeURIComponent(runId)}/prefix?through_sequence=${sequence}`, signal,
  );
  if (!prefix || prefix.run_id !== runId.toLowerCase() || prefix.through_sequence !== sequence ||
      typeof prefix.digest !== "string" || !/^[0-9a-f]{64}$/.test(prefix.digest)) {
    throw new Error("固定前缀响应与当前 Run 或事件不一致");
  }
  return { run_id: prefix.run_id, through_sequence: prefix.through_sequence, digest: prefix.digest };
}
