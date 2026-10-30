import type { RecycleEntity } from "./domain";

/**
 * ts-rs 把 Rust 的 i64/u64 生成为 bigint，但 JSON 传输实际是 number。
 * 所有 API 边界用它把生成类型转换成运行时真实形态；请求体同理。
 */
export type Wire<T> = T extends bigint
  ? number
  : T extends Array<infer U>
    ? Array<Wire<U>>
    : T extends object
      ? { [K in keyof T]: Wire<T[K]> }
      : T;

export type WirePage<T> = {
  items: Array<Wire<T>>;
  total: number;
  page: number;
  page_size: number;
};

export type FieldError = {
  field: string;
  message: string;
};

export type ApiErrorEnvelope = {
  error?: {
    code?: string;
    message?: string;
    details?: unknown;
    request_id?: string;
  };
};

export type FieldErrors = Record<string, string>;

export type ListQuery = {
  page?: number;
  pageSize?: number;
};

export type NovelListQuery = ListQuery & {
  q?: string;
  status?: "active" | "archived" | "";
};

export type RecycleItem = {
  entity_type: RecycleEntity;
  id: string;
  name: string;
  deleted_at: number;
  novel_id: string;
  novel_title: string;
};

export type SearchItem = {
  entity_type: string;
  entity_id: string;
  title: string;
  snippet: string;
};

export type HealthReport = {
  status: "ok" | "degraded";
  app_version: string;
  schema_version: number;
  format_version: number;
  db: string;
  llm_configured: boolean;
};

export type LlmView = {
  provider: string;
  base_url: string;
  model: string;
  fallbacks: string[];
  connect_timeout_seconds: number;
  attempt_timeout_seconds: number;
  max_retries: number;
};

export type SandboxView = {
  max_concurrent: number;
  max_steps: number;
  step_timeout_seconds: number;
  session_timeout_seconds: number;
  history_max_tokens: number;
  context_hard_cap_tokens: number;
  rolling_summary_max_tokens: number;
  director_repeat_limit: number;
  waiting_input_timeout_seconds: number;
};

export type HttpView = {
  body_limit_bytes: number;
  import_body_limit_bytes: number;
  sse_heartbeat_seconds: number;
  dev_cors: boolean;
};

export type AppConfig = {
  bind: string;
  data_dir: string;
  llm: LlmView;
  sandbox: SandboxView;
  http: HttpView;
  recycle: { retention_days: number };
  cost: { monthly_budget_usd: number };
  log: { level: string; trace_prompt: boolean };
  fts_min_query_length: number;
  warnings?: string[];
};

export type ConfigPatch = {
  model?: string;
  provider?: string;
  base_url?: string;
  fallbacks?: string[];
  max_concurrent?: number;
  monthly_budget_usd?: number;
  sse_heartbeat_seconds?: number;
  log_level?: string;
  attempt_timeout_seconds?: number;
  step_timeout_seconds?: number;
  session_timeout_seconds?: number;
  retention_days?: number;
};

export type UsageDay = { date: string; cost_usd: number; calls: number };
export type UsagePurpose = { purpose: string; calls: number };

export type UsageReport = {
  month: string;
  total_cost_usd: number;
  total_calls: number;
  by_day: UsageDay[];
  by_purpose: UsagePurpose[];
};

export type ImportReport = {
  novel_id: string;
  imported: Record<string, number>;
  renamed: string[];
  skipped: string[];
};

export type ClientErrorReport = {
  message: string;
  stack?: string;
  component_stack?: string;
  route?: string;
  user_agent?: string;
  at: number;
};

export function isFieldErrorArray(details: unknown): details is FieldError[] {
  return (
    Array.isArray(details) &&
    details.every(
      (d) =>
        typeof d === "object" &&
        d !== null &&
        typeof (d as FieldError).field === "string" &&
        typeof (d as FieldError).message === "string",
    )
  );
}
