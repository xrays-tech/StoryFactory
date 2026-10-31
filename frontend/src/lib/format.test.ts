import { describe, expect, it } from "vitest";
import { countWords, formatCost, formatDuration, truncate } from "./format";

describe("format", () => {
  it("counts non-whitespace characters like the backend", () => {
    expect(countWords("")).toBe(0);
    expect(countWords("第一段 正文\n\n第二段")).toBe(8);
    expect(countWords("a\tb\n c")).toBe(3);
  });

  it("formats durations", () => {
    expect(formatDuration(0)).toBe("00:00");
    expect(formatDuration(65)).toBe("01:05");
    expect(formatDuration(3661)).toBe("1:01:01");
  });

  it("formats costs", () => {
    expect(formatCost(0)).toBe("$0.00");
    expect(formatCost(0.0023)).toBe("$0.0023");
    expect(formatCost(1.5)).toBe("$1.50");
  });

  it("places the minus sign before the currency symbol", () => {
    expect(formatCost(-0.5)).toBe("-$0.50");
    expect(formatCost(-0.0023)).toBe("-$0.0023");
    expect(formatCost(-0)).toBe("$0.00");
  });

  it("truncates long text", () => {
    expect(truncate("一二三四五", 3)).toBe("一二三…");
    expect(truncate("短", 3)).toBe("短");
  });
});
