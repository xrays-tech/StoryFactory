import { zh } from "../copy/zh";
import { ApiError, userMessage } from "../lib/errors";
import type { ApiErrorEnvelope, ClientErrorReport } from "../types/api";

const API_BASE = "/api/v1";
const TOKEN_KEY = "sa_app_token";
const REQUEST_ID_HEADER = "X-Request-Id";
const APP_TOKEN_HEADER = "X-App-Token";

export type QueryValue = string | number | boolean | null | undefined;

export function buildQuery(query?: Record<string, QueryValue>): string {
  if (!query) return "";
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null || value === "") continue;
    params.set(key, String(value));
  }
  const text = params.toString();
  return text ? `?${text}` : "";
}

export function apiUrl(path: string): string {
  if (/^https?:\/\//.test(path)) return path;
  return `${API_BASE}${path.startsWith("/") ? path : `/${path}`}`;
}

export function getAppToken(): string {
  try {
    return localStorage.getItem(TOKEN_KEY) ?? "";
  } catch {
    return "";
  }
}

export function setAppToken(token: string): void {
  try {
    if (token) localStorage.setItem(TOKEN_KEY, token);
    else localStorage.removeItem(TOKEN_KEY);
  } catch {
    // localStorage 不可用时，至少让当前页面继续工作。
  }
}

/** 首次加载时从 ?token= 提取令牌并写入 localStorage，同时把令牌从地址栏移除。 */
export function bootstrapAppToken(): void {
  try {
    const url = new URL(window.location.href);
    const token = url.searchParams.get("token");
    if (token) {
      setAppToken(token);
      url.searchParams.delete("token");
      const next = `${url.pathname}${url.search}${url.hash}`;
      window.history.replaceState(null, "", next);
    }
  } catch {
    // ignore
  }
}

export function newRequestId(): string {
  try {
    return crypto.randomUUID();
  } catch {
    return `req-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  }
}

export function newIdempotencyKey(prefix = "idem"): string {
  try {
    return `${prefix}-${crypto.randomUUID()}`;
  } catch {
    return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  }
}

export function authHeaders(extra: Record<string, string> = {}): Record<string, string> {
  const token = getAppToken();
  return token ? { ...extra, [APP_TOKEN_HEADER]: token } : extra;
}

/** SSE 无法（在原生 EventSource 下）设置请求头，因此把令牌放进查询串。 */
export function withTokenInQuery(url: string): string {
  const token = getAppToken();
  if (!token) return url;
  const separator = url.includes("?") ? "&" : "?";
  return `${url}${separator}token=${encodeURIComponent(token)}`;
}

export type RequestOptions = {
  query?: Record<string, QueryValue>;
  body?: unknown;
  signal?: AbortSignal;
  idempotencyKey?: string;
  timeoutMs?: number;
  headers?: Record<string, string>;
  requestId?: string;
};

function parseRetryAfter(value: string | null): number | undefined {
  if (!value) return undefined;
  const seconds = Number(value);
  if (Number.isFinite(seconds)) return Math.max(0, Math.floor(seconds));
  const date = Date.parse(value);
  if (!Number.isNaN(date)) return Math.max(0, Math.floor((date - Date.now()) / 1000));
  return undefined;
}

async function parseError(response: Response, fallbackMessage?: string): Promise<ApiError> {
  const headerRequestId = response.headers.get("x-request-id") ?? undefined;
  const retryAfter = parseRetryAfter(response.headers.get("retry-after"));
  let envelope: ApiErrorEnvelope | null = null;
  let text = "";
  try {
    text = await response.text();
    if (text.trim().startsWith("{")) envelope = JSON.parse(text) as ApiErrorEnvelope;
  } catch {
    // ignore
  }
  const inner = envelope?.error;
  const status = response.status;
  let message = inner?.message?.trim() || fallbackMessage || "";
  if (!message) {
    if (status === 401 || status === 403) message = zh.errors.auth;
    else if (status === 404) message = zh.errors.notFound;
    else if (status === 409) message = zh.errors.conflict;
    else if (status === 422) message = zh.errors.validation;
    else if (status === 429) message = zh.errors.rateLimited;
    else if (status >= 500) message = zh.errors.internal;
    else message = `${zh.common.error} (${status})`;
  }
  const apiError = new ApiError({
    status,
    code: inner?.code,
    message,
    details: inner?.details,
    requestId: inner?.request_id ?? headerRequestId,
    retryAfter,
  });
  if (status === 409 || status === 422 || status === 429) {
    window.dispatchEvent(new CustomEvent("sa:api-error", { detail: apiError }));
  }
  return apiError;
}

async function execute(
  method: string,
  path: string,
  opts: RequestOptions = {},
): Promise<Response> {
  const timeoutMs = opts.timeoutMs ?? 20_000;
  const controller = new AbortController();
  const external = opts.signal;
  const onAbort = () => controller.abort();
  if (external) {
    if (external.aborted) controller.abort();
    else external.addEventListener("abort", onAbort, { once: true });
  }
  const timer = window.setTimeout(() => controller.abort(), timeoutMs);

  const headers: Record<string, string> = authHeaders({
    Accept: "application/json",
    [REQUEST_ID_HEADER]: opts.requestId ?? newRequestId(),
    ...(opts.headers ?? {}),
  });
  if (opts.idempotencyKey) headers["Idempotency-Key"] = opts.idempotencyKey;

  let body: BodyInit | undefined;
  if (opts.body !== undefined) {
    if (opts.body instanceof Blob) {
      body = opts.body;
    } else {
      headers["Content-Type"] = "application/json";
      body = JSON.stringify(opts.body);
    }
  }

  try {
    const url = apiUrl(path) + buildQuery(opts.query);
    return await fetch(url, {
      method,
      headers,
      body,
      signal: controller.signal,
      credentials: "same-origin",
    });
  } finally {
    window.clearTimeout(timer);
    external?.removeEventListener("abort", onAbort);
  }
}

export async function request<T>(
  method: string,
  path: string,
  opts: RequestOptions = {},
): Promise<T> {
  let response: Response;
  try {
    response = await execute(method, path, opts);
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      throw new ApiError({
        status: 0,
        code: "Timeout",
        message: opts.signal?.aborted ? "请求已取消" : zh.errors.timeout,
      });
    }
    if (error instanceof TypeError) {
      throw new ApiError({ status: 0, code: "NetworkError", message: zh.errors.network });
    }
    throw error;
  }

  if (!response.ok) throw await parseError(response);
  if (response.status === 204) return undefined as T;
  const text = await response.text();
  if (!text.trim()) return undefined as T;
  const contentType = response.headers.get("content-type") ?? "";
  if (!contentType.includes("application/json")) {
    return text as T;
  }
  return JSON.parse(text) as T;
}

export async function requestBlob(
  method: string,
  path: string,
  opts: RequestOptions = {},
): Promise<{ blob: Blob; filename: string | null }> {
  let response: Response;
  try {
    response = await execute(method, path, { timeoutMs: 120_000, ...opts });
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      throw new ApiError({ status: 0, code: "Timeout", message: zh.errors.timeout });
    }
    if (error instanceof TypeError) {
      throw new ApiError({ status: 0, code: "NetworkError", message: zh.errors.network });
    }
    throw error;
  }
  if (!response.ok) throw await parseError(response);
  const disposition = response.headers.get("content-disposition") ?? "";
  const match = /filename="?([^";]+)"?/i.exec(disposition);
  return { blob: await response.blob(), filename: match?.[1] ?? null };
}

export async function uploadFile<T>(
  path: string,
  file: File,
  opts: { idempotencyKey?: string; timeoutMs?: number; headers?: Record<string, string> } = {},
): Promise<T> {
  let response: Response;
  try {
    response = await execute("POST", path, {
      body: file,
      headers: { "Content-Type": file.type || "application/zip", ...(opts.headers ?? {}) },
      idempotencyKey: opts.idempotencyKey,
      timeoutMs: opts.timeoutMs ?? 180_000,
    });
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      throw new ApiError({ status: 0, code: "Timeout", message: zh.errors.timeout });
    }
    if (error instanceof TypeError) {
      throw new ApiError({ status: 0, code: "NetworkError", message: zh.errors.network });
    }
    throw error;
  }
  if (!response.ok) throw await parseError(response);
  if (response.status === 204) return undefined as T;
  const text = await response.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

export function getErrorDisplay(error: unknown): string {
  return userMessage(error);
}

export async function reportClientError(report: ClientErrorReport): Promise<void> {
  try {
    const body = JSON.stringify(report).slice(0, 32 * 1024 - 16);
    await fetch(apiUrl("/client-errors"), {
      method: "POST",
      headers: authHeaders({ "Content-Type": "application/json" }),
      body,
      keepalive: true,
    });
  } catch {
    // 前端上报不能反过来影响 UI。
  }
}

export const apiBase = API_BASE;
