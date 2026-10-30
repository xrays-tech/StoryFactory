import { zh } from "../copy/zh";
import type { FieldErrors } from "../types/api";
import { isFieldErrorArray } from "../types/api";

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly details?: unknown;
  readonly requestId?: string;
  readonly retryAfter?: number;
  readonly fieldErrors: FieldErrors;

  constructor(input: {
    status: number;
    code?: string;
    message: string;
    details?: unknown;
    requestId?: string;
    retryAfter?: number;
  }) {
    super(input.message);
    this.name = "ApiError";
    this.status = input.status;
    this.code = input.code ?? codeForStatus(input.status);
    this.details = input.details;
    this.requestId = input.requestId;
    this.retryAfter = input.retryAfter;
    this.fieldErrors = extractFieldErrors(input.details);
  }
}

export function extractFieldErrors(details: unknown): FieldErrors {
  if (!isFieldErrorArray(details)) return {};
  const out: FieldErrors = {};
  for (const item of details) {
    if (!out[item.field]) out[item.field] = item.message;
  }
  return out;
}

export function codeForStatus(status: number): string {
  switch (status) {
    case 400:
      return "validation_failed";
    case 401:
      return "token_required";
    case 403:
      return "origin_forbidden";
    case 404:
      return "route_not_found";
    case 409:
      return "session_conflict";
    case 413:
      return "payload_too_large";
    case 422:
      return "validation_failed";
    case 429:
      return "rate_limited";
    default:
      return "internal";
  }
}

const codeMessage: Record<string, string> = {
  novel_not_found: zh.errors.notFound,
  character_not_found: zh.errors.notFound,
  item_not_found: zh.errors.notFound,
  volume_not_found: zh.errors.notFound,
  chapter_not_found: zh.errors.notFound,
  session_not_found: zh.errors.notFound,
  recycle_item_not_found: zh.errors.notFound,
  route_not_found: zh.errors.notFound,
  session_conflict: zh.errors.conflictSession,
  idempotency_conflict: zh.errors.idempotency,
  name_conflict: zh.errors.nameConflict,
  validation_failed: zh.errors.validation,
  payload_too_large: zh.errors.tooLarge,
  rate_limited: zh.errors.rateLimited,
  host_forbidden: zh.errors.forbidden,
  origin_forbidden: zh.errors.forbidden,
  token_required: zh.errors.auth,
  token_invalid: zh.errors.auth,
  unsupported_import_version: "导入包版本不受支持。",
  llm_unavailable: zh.errors.llmUnavailable,
  llm_rate_limited: zh.errors.rateLimited,
  llm_config_missing: zh.errors.llmConfigMissing,
  config_invalid: "配置校验失败。",
  crypto_error: "加密或解密失败。",
  internal: zh.errors.internal,
};

export function userMessage(error: unknown): string {
  if (error instanceof ApiError) {
    const mapped = codeMessage[error.code];
    const base = mapped ?? zh.errors.unknown;
    const rateLimited = error.code === "rate_limited" || error.code === "llm_rate_limited";
    if (rateLimited && error.retryAfter) {
      return `${error.message || base}（约 ${error.retryAfter} 秒后可重试）`;
    }
    if (error.code === "internal" || error.code === "crypto_error") return base;
    return error.message || base;
  }
  if (error instanceof DOMException && error.name === "AbortError") {
    return zh.errors.timeout;
  }
  if (error instanceof TypeError) {
    return zh.errors.network;
  }
  if (error instanceof Error && error.message) return error.message;
  return zh.errors.unknown;
}

export function errorTitle(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 401 || error.status === 403) return "无法访问本地服务";
    if (error.status === 404) return "内容不存在";
    if (error.status === 409) return "状态冲突";
    if (error.status === 422) return "内容需要修改";
    if (error.status === 429) return "请求过于频繁";
  }
  return zh.common.error;
}

export function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}
