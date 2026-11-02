import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CreateSessionReq, SessionSnapshot } from "../types/domain";
import type { Wire, WirePage } from "../types/api";
import type { SandboxEvent } from "../api/sse";

const { listSessions, createSession } = vi.hoisted(() => ({
  listSessions: vi.fn(),
  createSession: vi.fn(),
}));

vi.mock("../api/endpoints", () => ({
  listSessions,
  createSession,
}));

import { useSandboxStore } from "./sandboxStore";
import { buildCreateSessionPayload, type CreateSessionDraft } from "../lib/sandboxSession";

type Session = Wire<SessionSnapshot>;

function session(overrides: Partial<Session> = {}): Session {
  return {
    id: "s1",
    novel_id: "n1",
    chapter_id: null,
    prose_draft_id: null,
    status: "created",
    mode: "director",
    current_step: 0,
    step_limit: 10,
    beats_total: 0,
    beats_done_count: 0,
    actor_ids: [],
    actor_names: [],
    last_event_seq: 0,
    can_recover: false,
    usage: {
      prompt_tokens: 0,
      completion_tokens: 0,
      cost_usd: 0,
      calls: 0,
      by_purpose: [],
    },
    error: null,
    created_at: 1,
    updated_at: 1,
    finished_at: null,
    session_deadline: null,
    ...overrides,
  };
}

function event(seq: number): SandboxEvent {
  return {
    seq,
    event_type: "step_started",
    original_type: "step_started",
    payload: {},
    received_at: 0,
  };
}

function emptyPage(page = 1): WirePage<SessionSnapshot> {
  return { items: [], total: 0, page, page_size: 50 };
}

const createReq: Wire<CreateSessionReq> = {
  actor_ids: [],
  mode: "director",
  scene_prompt: "场景",
  step_limit: 10,
  director_config: null,
  user_directive: null,
  autostart: true,
  chapter_id: null,
  auto_prose: null,
};

function resetStore() {
  useSandboxStore.setState({
    novelId: "n1",
    sessions: [],
    sessionsTotal: 0,
    sessionsLoading: false,
    sessionsError: null,
    sessionFilter: "",
    sessionsPage: 1,
    session: session(),
    snapshotLoading: false,
    snapshotError: null,
    events: [],
    lastEventSeq: 0,
    connection: "closed",
    reconnectAttempt: 0,
    autoScroll: true,
    outline: null,
    outlineChapterId: null,
    streamError: null,
  });
}

describe("useSandboxStore.ingestEvent", () => {
  beforeEach(() => {
    resetStore();
  });

  it("drops replayed events using the highest seen seq instead of scanning the trimmed buffer", () => {
    const events = Array.from({ length: 500 }, (_, index) => event(501 + index));
    useSandboxStore.setState({ events, lastEventSeq: 1000 });

    useSandboxStore.getState().ingestEvent(event(900));

    const state = useSandboxStore.getState();
    expect(state.events).toHaveLength(500);
    expect(state.events[0].seq).toBe(501);
    expect(state.events.at(-1)?.seq).toBe(1000);
    expect(state.lastEventSeq).toBe(1000);
  });

  it("appends newer events and trims the buffer", () => {
    const events = Array.from({ length: 500 }, (_, index) => event(501 + index));
    useSandboxStore.setState({ events, lastEventSeq: 1000 });

    useSandboxStore.getState().ingestEvent(event(1001));

    const state = useSandboxStore.getState();
    expect(state.events).toHaveLength(500);
    expect(state.events[0].seq).toBe(502);
    expect(state.events.at(-1)?.seq).toBe(1001);
    expect(state.lastEventSeq).toBe(1001);
  });
});

describe("buildCreateSessionPayload", () => {
  const draft: CreateSessionDraft = {
    actorIds: ["a1", "a2"],
    mode: "director",
    scenePrompt: "雨夜的旧书店，两人重逢。",
    stepLimit: 12,
    directorConfig: null,
    userDirective: "保持悬疑",
    autostart: true,
    chapterId: null,
    autoProse: true,
  };

  it("puts the bound chapter into the request body", () => {
    const payload = buildCreateSessionPayload({ ...draft, chapterId: "c1" });

    expect(payload.chapter_id).toBe("c1");
    expect(payload.actor_ids).toEqual(["a1", "a2"]);
    expect(payload.scene_prompt).toBe("雨夜的旧书店，两人重逢。");
    expect(payload.step_limit).toBe(12);
  });

  it("sends null when the session is not bound to a chapter", () => {
    expect(buildCreateSessionPayload(draft).chapter_id).toBeNull();
    expect(buildCreateSessionPayload({ ...draft, chapterId: "   " }).chapter_id).toBeNull();
  });

  it("sends auto_prose only when a chapter is bound", () => {
    expect(buildCreateSessionPayload({ ...draft, chapterId: "c1", autoProse: true }).auto_prose).toBe(true);
    expect(buildCreateSessionPayload({ ...draft, chapterId: "c1", autoProse: false }).auto_prose).toBe(false);
    expect(buildCreateSessionPayload({ ...draft, autoProse: true }).auto_prose).toBeNull();
    expect(buildCreateSessionPayload({ ...draft, chapterId: "  ", autoProse: true }).auto_prose).toBeNull();
  });
});

describe("useSandboxStore.createSession", () => {
  beforeEach(() => {
    listSessions.mockReset();
    createSession.mockReset();
    listSessions.mockResolvedValue(emptyPage());
    resetStore();
  });

  it("inserts a matching session on the first page", async () => {
    const created = session();
    createSession.mockResolvedValue(created);

    await useSandboxStore.getState().createSession("n1", null, createReq);

    expect(useSandboxStore.getState().sessions).toEqual([created]);
    expect(useSandboxStore.getState().sessionsTotal).toBe(1);
    expect(listSessions).not.toHaveBeenCalled();
  });

  it("reloads silently when the session misses the status filter", async () => {
    useSandboxStore.setState({ sessionFilter: "running" });
    createSession.mockResolvedValue(session({ status: "created" }));

    await useSandboxStore.getState().createSession("n1", null, createReq);

    expect(useSandboxStore.getState().sessions).toEqual([]);
    expect(useSandboxStore.getState().sessionsTotal).toBe(0);
    expect(listSessions).toHaveBeenCalledWith("n1", {
      status: "running",
      page: 1,
      pageSize: 50,
    });
  });

  it("reloads silently when the list is not on the first page", async () => {
    useSandboxStore.setState({ sessionsPage: 2 });
    createSession.mockResolvedValue(session());

    await useSandboxStore.getState().createSession("n1", null, createReq);

    expect(useSandboxStore.getState().sessions).toEqual([]);
    expect(listSessions).toHaveBeenCalledWith("n1", {
      status: "",
      page: 2,
      pageSize: 50,
    });
  });
});
