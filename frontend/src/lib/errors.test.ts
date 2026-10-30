import { describe, expect, it } from "vitest";
import { ApiError, extractFieldErrors, userMessage } from "./errors";

describe("ApiError", () => {
  it("extracts field errors from details", () => {
    const error = new ApiError({
      status: 422,
      code: "validation_failed",
      message: "校验失败",
      details: [{ field: "scene_prompt", message: "至少 10 个字符" }],
    });
    expect(error.fieldErrors).toEqual({ scene_prompt: "至少 10 个字符" });
    expect(extractFieldErrors({ bad: true })).toEqual({});
  });

  it("maps rate limiting with retry-after guidance", () => {
    const error = new ApiError({
      status: 429,
      code: "rate_limited",
      message: "并发已达上限",
      retryAfter: 7,
    });
    expect(userMessage(error)).toBe("并发已达上限（约 7 秒后可重试）");
  });

  it("keeps readable backend messages for known codes", () => {
    const error = new ApiError({
      status: 409,
      code: "session_conflict",
      message: "只有 paused 会话可以 resume",
    });
    expect(userMessage(error)).toBe("只有 paused 会话可以 resume");
  });
});
