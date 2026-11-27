export interface Identity {
  subject: { kind: string; stable_id: string };
  predicate: string;
  context: Record<string, string>;
}
export interface User {
  id: string;
  workspace_id: string;
  role: string;
}
export interface ConsoleWorkspace extends User {
  name: string;
  can_create_owned_workspaces?: boolean;
}
export interface ConsoleSession {
  account: string;
  binding: string;
  workspaces: ConsoleWorkspace[];
}
export interface ConsoleSessionResponse {
  mode: "account";
  setupRequired?: true;
  session: ConsoleSession | null;
}
export interface Asset {
  asset_id: string;
  kind: "memory" | "knowledge";
  title: string;
  version: number;
  content?: string;
  identity: Identity | null;
  normalization_status: string;
  semantic_status?: string;
  semantic_values?: unknown[];
  source_event_id: string;
  content_hash: string;
  restored_from: number | null;
}
export interface Page<T> {
  items: T[];
  next_after: string | null;
}
export interface Run {
  run_id: string;
  owner_key_id: string;
  is_owner: boolean;
  summary: string | null;
  created_at_ms: number;
  status: "reported_success" | "reported_failure" | "unknown";
}
export interface RunEvent {
  sequence: number;
  reporter_principal_id: string;
  evidence_origin: string;
  recorded_at_ms: number;
  references: unknown[];
  report: {
    event?: Record<string, unknown>;
    reporter?: string;
    observed_at_ms?: number;
    kind?: string;
    [key: string]: unknown;
  };
}
export interface Trace {
  run: { run_id: string; summary: string };
  events: RunEvent[];
  next_after: number | null;
  coverage_notice: string;
}
export interface Receipt {
  job_id?: string;
  asset_id?: string;
  source_event_id?: string;
  state?: string;
  version?: number;
}
export interface Job {
  id: string;
  state: string;
  outcome?: string;
  error_code?: string;
  result?: { asset_id?: string; memories?: { asset_id: string }[] };
  asset_id?: string;
}
export interface Provenance {
  type: string;
  asset_id?: string;
  asset_version?: number;
  source_event_id?: string;
  locator?: Record<string, unknown>;
  [key: string]: unknown;
}
export type LongTermMemoryState =
  | { status: "current"; value: string }
  | { status: "conflicting" | "unknown" };
export interface LongTermMemoryCitation {
  source_event_id: string;
  effective_at_ms: number | null;
  statement: { value: string; quote: string; byte_start: number; byte_end: number };
}
export interface LongTermMemoryProvenance extends Provenance {
  type: "long_term_memory";
  memory_id: string;
  version: number;
  as_of_ms: number;
  effective_from_ms: number | null;
  state: LongTermMemoryState;
  timeline: { effective_from_ms: number | null; state: LongTermMemoryState }[];
  citations: LongTermMemoryCitation[];
}
export interface ContextItem {
  evidence_id: string;
  route: string;
  title: string;
  content: string;
  provenance: LongTermMemoryProvenance | (Provenance & { type: "asset_chunk" | "session_turn" });
  normalization_status: string;
  semantic_status?: string;
  conditions?: string[];
  applicability?: string;
}
export interface Bundle {
  budget: number;
  used_bytes: number;
  items: ContextItem[];
  warnings: string[];
  rendered_context: string;
  effective_mode: string;
  requested_mode: string;
  routes: {
    route: string;
    retained: number;
    omitted_for_budget: number;
    used_bytes: number;
  }[];
}
