import { beforeEach, describe, expect, it } from "vitest";
import { setAppToken } from "./client";
import { draftEventsUrl, eventsUrl } from "./endpoints";

describe("eventsUrl", () => {
  beforeEach(() => {
    setAppToken("");
  });

  it("keeps the app token out of the SSE query string", () => {
    setAppToken("secret-token");
    const url = eventsUrl("s1", 42);
    expect(url).toBe("/api/v1/sandbox-sessions/s1/events?after_seq=42");
    expect(url).not.toContain("token=");
    expect(url).not.toContain("secret-token");
  });

  it("omits after_seq when no sequence is given", () => {
    expect(eventsUrl("s1")).toBe("/api/v1/sandbox-sessions/s1/events");
  });

  it("keeps the app token out of draft SSE urls", () => {
    setAppToken("secret-token");
    const url = draftEventsUrl("d1");
    expect(url).toBe("/api/v1/chapter-drafts/d1/events");
    expect(url).not.toContain("token=");
    expect(url).not.toContain("secret-token");
  });
});
