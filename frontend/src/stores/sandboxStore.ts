import { create } from "zustand";
import { subscribeWithSelector } from "zustand/middleware";
import { immer } from "zustand/middleware/immer";
import type {
  ControlAction,
  CreateSessionReq,
  SessionSnapshot,
  SessionStatus,
} from "../types/domain";
import type { Wire } from "../types/api";
import * as api from "../api/endpoints";
import {
  SandboxEventStream,
  type SandboxEvent,
  type StreamState,
} from "../api/sse";
import { userMessage } from "../lib/errors";
import { useCharacterStore } from "./characterStore";
import { useOutlineStore } from "./outlineStore";

export type Session = Wire<SessionSnapshot>;

const EVENT_LIMIT = 500;

type SandboxState = {
  novelId: string | null;
  sessions: Session[];
  sessionsTotal: number;
  sessionsLoading: boolean;
  sessionsError: string | null;
  sessionFilter: SessionStatus | "";
  sessionsPage: number;
  session: Session | null;
  snapshotLoading: boolean;
  snapshotError: string | null;
  events: SandboxEvent[];
  lastEventSeq: number;
  connection: StreamState;
  reconnectAttempt: number;
  autoScroll: boolean;
  outline: string | null;
  outlineChapterId: string | null;
  streamError: string | null;
  loadSessions: (
    novelId: string,
    options?: { status?: SessionStatus | ""; page?: number; pageSize?: number; silent?: boolean },
  ) => Promise<void>;
  createSession: (
    novelId: string,
    chapterId: string | null,
    req: Wire<CreateSessionReq>,
    idempotencyKey?: string,
  ) => Promise<Session>;
  loadSession: (sid: string, options?: { silent?: boolean; preserveEvents?: boolean }) => Promise<Session | null>;
  connect: (sid: string, afterSeq?: number) => void;
  disconnect: (final?: boolean) => void;
  control: (action: ControlAction) => Promise<Session>;
  sendInput: (text: string) => Promise<Session>;
  setAutoScroll: (value: boolean) => void;
  clearEvents: () => void;
  ingestEvent: (event: SandboxEvent) => void;
  clear: () => void;
};

let stream: SandboxEventStream | null = null;
let sessionsSeq = 0;
let snapshotSeq = 0;

function mergeSession(base: Session | null, next: Session): Session {
  if (!base || base.id !== next.id) return next;
  return next;
}

function isTerminal(status: SessionStatus): boolean {
  return status === "completed" || status === "failed" || status === "aborted";
}

export const useSandboxStore = create<SandboxState>()(
  subscribeWithSelector(
    immer((set, get) => ({
      novelId: null,
      sessions: [],
      sessionsTotal: 0,
      sessionsLoading: false,
      sessionsError: null,
      sessionFilter: "",
      sessionsPage: 1,
      session: null,
      snapshotLoading: false,
      snapshotError: null,
      events: [],
      lastEventSeq: 0,
      connection: "idle",
      reconnectAttempt: 0,
      autoScroll: true,
      outline: null,
      outlineChapterId: null,
      streamError: null,

      loadSessions: async (novelId, options = {}) => {
        const seq = ++sessionsSeq;
        set((state) => {
          state.novelId = novelId;
          if (options.status !== undefined) state.sessionFilter = options.status;
          state.sessionsPage = options.page ?? 1;
          if (!options.silent) {
            state.sessionsLoading = true;
            state.sessionsError = null;
          }
        });
        try {
          const page = await api.listSessions(novelId, {
            status: options.status ?? get().sessionFilter,
            page: options.page ?? 1,
            pageSize: options.pageSize ?? 50,
          });
          if (seq !== sessionsSeq) return;
          set((state) => {
            state.sessions = page.items;
            state.sessionsTotal = page.total;
            state.sessionsPage = page.page;
            state.sessionsLoading = false;
          });
        } catch (error) {
          if (seq !== sessionsSeq) return;
          set((state) => {
            state.sessionsLoading = false;
            state.sessionsError = userMessage(error);
          });
        }
      },

      createSession: async (novelId, chapterId, req, idempotencyKey) => {
        const session = await api.createSession(novelId, chapterId, req, idempotencyKey);
        const state = get();
        const belongsToCurrentList = state.novelId === novelId;
        const matchesQuery = !state.sessionFilter || session.status === state.sessionFilter;
        const canInsert = belongsToCurrentList && state.sessionsPage === 1 && matchesQuery;

        if (canInsert) {
          set((draft) => {
            draft.sessions.unshift(session);
            draft.sessionsTotal += 1;
          });
          return session;
        }

        // 筛选/分页/其他小说视图下不猜测列表位置，静默回到服务端当前查询。
        void state.loadSessions(state.novelId ?? novelId, {
          silent: true,
          page: state.sessionsPage,
        });
        return session;
      },

      loadSession: async (sid, options = {}) => {
        const seq = ++snapshotSeq;
        set((state) => {
          if (state.session && state.session.id !== sid) {
            state.events = [];
            state.lastEventSeq = 0;
            state.outline = null;
            state.outlineChapterId = null;
          }
          if (!options.silent) {
            state.snapshotLoading = true;
            state.snapshotError = null;
          }
        });
        try {
          const session = await api.getSession(sid);
          if (seq !== snapshotSeq) return null;
          set((state) => {
            state.session = mergeSession(state.session, session);
            state.novelId = session.novel_id;
            state.snapshotLoading = false;
            state.snapshotError = null;
            if (session.chapter_id) state.outlineChapterId = session.chapter_id;
          });
          return session;
        } catch (error) {
          if (seq !== snapshotSeq) return null;
          set((state) => {
            state.snapshotLoading = false;
            state.snapshotError = userMessage(error);
          });
          return null;
        }
      },

      connect: (sid, afterSeq = 0) => {
        get().disconnect();
        set((state) => {
          state.connection = "connecting";
          state.reconnectAttempt = 0;
          state.streamError = null;
          state.lastEventSeq = Math.max(state.lastEventSeq, afterSeq);
        });
        stream = new SandboxEventStream(
          sid,
          {
            onState: (connection, attempt) =>
              set((state) => {
                state.connection = connection;
                state.reconnectAttempt = attempt;
              }),
            onOpen: () => {
              set((state) => {
                state.connection = "open";
                state.reconnectAttempt = 0;
                state.streamError = null;
              });
            },
            onReconnect: (attempt) =>
              set((state) => {
                state.connection = "reconnecting";
                state.reconnectAttempt = attempt;
              }),
            onError: (error) =>
              set((state) => {
                state.streamError = userMessage(error);
              }),
            onEvent: (event) => get().ingestEvent(event),
            onUnknown: (event) => {
              // §8.7 前向兼容：未知事件进入事件流兜底渲染，不当作连接错误。
              get().ingestEvent({ ...event, unknown: true });
            },
            onDone: () => {
              set((state) => {
                state.connection = "closed";
              });
              const current = get().session;
              if (current) void get().loadSession(current.id, { silent: true });
            },
          },
          { afterSeq },
        );
        stream.start();
      },

      disconnect: (final = false) => {
        stream?.close(final);
        stream = null;
        set((state) => {
          state.connection = "closed";
        });
      },

      control: async (action) => {
        const current = get().session;
        if (!current) throw new Error("会话尚未加载");
        const session = await api.controlSession(current.id, { action });
        set((state) => {
          state.session = mergeSession(state.session, session);
        });
        return session;
      },

      sendInput: async (text) => {
        const current = get().session;
        if (!current) throw new Error("会话尚未加载");
        const session = await api.inputSession(current.id, { text });
        set((state) => {
          state.session = mergeSession(state.session, session);
        });
        return session;
      },

      setAutoScroll: (value) =>
        set((state) => {
          state.autoScroll = value;
        }),

      clearEvents: () =>
        set((state) => {
          state.events = [];
          state.lastEventSeq = 0;
          state.outline = null;
          state.outlineChapterId = null;
          state.streamError = null;
        }),

      ingestEvent: (event) => {
        const current = get().session;
        // 事件按 seq 单调递增；重放/重连只需和已见最大序号比较，
        // 不能依赖被 EVENT_LIMIT 裁剪过的事件数组。
        if (event.seq > 0 && event.seq <= get().lastEventSeq) return;

        set((state) => {
          if (event.seq > state.lastEventSeq) state.lastEventSeq = event.seq;
          state.events.push(event);
          if (state.events.length > EVENT_LIMIT) {
            state.events.splice(0, state.events.length - EVENT_LIMIT);
          }
          if (!state.session) return;

          const step = typeof event.payload.step === "number" ? event.payload.step : null;
          if (step !== null) {
            state.session.current_step = Math.max(state.session.current_step, step);
          }

          switch (event.event_type) {
            case "session_started":
            case "session_resumed":
              if (state.session.status !== "running") state.session.status = "running";
              break;
            case "session_paused":
              state.session.status = "paused";
              break;
            case "waiting_input":
              state.session.status = "waiting_input";
              break;
            case "finalize_started":
              state.session.status = "finalizing";
              break;
            case "outline_ready": {
              const markdown = event.payload.outline_markdown;
              if (typeof markdown === "string") state.outline = markdown;
              const cid = event.payload.chapter_id;
              if (typeof cid === "string") state.outlineChapterId = cid;
              break;
            }
            case "session_completed":
              state.session.status = "completed";
              break;
            case "session_failed":
              state.session.status = "failed";
              break;
            case "session_aborted":
              state.session.status = "aborted";
              break;
            case "session_interrupted":
              state.session.status = "interrupted";
              state.session.can_recover = true;
              break;
            default:
              break;
          }
        });

        if (!current) return;
        const novelId = current.novel_id;

        if (event.event_type === "outline_ready") {
          const cid = event.payload.chapter_id;
          if (typeof cid === "string") {
            void useOutlineStore.getState().refreshChapter(novelId, cid);
          }
        }

        if (event.event_type === "emotion_updated") {
          const pair = event.payload.pair;
          if (Array.isArray(pair) && pair.length === 2 && typeof pair[0] === "string" && typeof pair[1] === "string") {
            const affinity = Number(event.payload.affinity_delta ?? 0) || 0;
            const trust = Number(event.payload.trust_delta ?? 0) || 0;
            useCharacterStore.getState().applyEmotionDelta([pair[0], pair[1]], affinity, trust);
          }
        }

        if (
          event.event_type === "session_completed" ||
          event.event_type === "session_failed" ||
          event.event_type === "session_aborted"
        ) {
          void get().loadSession(current.id, { silent: true });
        }
      },

      clear: () => {
        get().disconnect(true);
        set((state) => {
          state.sessions = [];
          state.sessionsTotal = 0;
          state.sessionsPage = 1;
          state.session = null;
          state.events = [];
          state.lastEventSeq = 0;
          state.outline = null;
          state.outlineChapterId = null;
          state.connection = "idle";
          state.streamError = null;
          state.snapshotError = null;
          state.sessionsError = null;
        });
      },
    })),
  ),
);

export function isSessionTerminal(status: SessionStatus): boolean {
  return isTerminal(status);
}
