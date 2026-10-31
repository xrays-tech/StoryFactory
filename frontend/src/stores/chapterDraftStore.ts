import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import type { ChapterContent, DraftJob, DraftStatus } from "../types/domain";
import type { Wire } from "../types/api";
import * as api from "../api/endpoints";
import { EventStream, type StreamState } from "../api/eventStream";
import { newIdempotencyKey } from "../api/client";
import { userMessage } from "../lib/errors";
import type { DraftPov } from "../lib/draftStyle";
import { useUiStore } from "./uiStore";
import { useChapterContentStore } from "./chapterContentStore";
import { zh } from "../copy/zh";

export type DraftPrefs = {
  style_name: string | null;
  style_sample: string | null;
  instruction: string | null;
  target_words: number;
  pov: DraftPov | null;
};

type ChapterDraftState = {
  novelId: string | null;
  chapterId: string | null;
  job: Wire<DraftJob> | null;
  /** POST 请求进行中（SSE 尚未建立）。 */
  creating: boolean;
  /** DELETE 请求进行中。 */
  cancelling: boolean;
  /** 最近一次创建/生成/读取失败的原因。 */
  error: string | null;
  stage: string | null;
  progress: number;
  /** 服务端附带的警示信息，例如“已跳过 1 场”。 */
  message: string | null;
  connection: StreamState;
  streamError: string | null;
  startedAt: number | null;
  /** 任务完成后拉取正文的过渡态。 */
  refreshing: boolean;
  /** 生成成功但正文读取失败时的原因。 */
  refreshError: string | null;
  /** 上一次提交的设置，供失败后原样重试。 */
  prefs: DraftPrefs | null;

  create: (novelId: string, chapterId: string, prefs: DraftPrefs) => Promise<void>;
  retry: () => Promise<void>;
  cancel: () => Promise<void>;
  ingestJob: (job: Wire<DraftJob>) => void;
  clearRefreshError: () => void;
  reset: () => void;
};

const STREAM_EVENTS = ["draft.snapshot", "draft.progress", "draft.done", "draft.failed"] as const;
const TERMINAL_EVENTS = ["draft.done", "draft.failed"] as const;

let stream: EventStream | null = null;
/** 每次创建/取消/重置递增，用来丢弃上一轮 SSE 的回调。 */
let runSeq = 0;

export function isDraftRunning(job: Wire<DraftJob> | null): boolean {
  return !!job && (job.status === "queued" || job.status === "generating");
}

export function draftStatusLabel(status: DraftStatus): string {
  switch (status) {
    case "queued":
      return zh.draft.stageQueued;
    case "generating":
      return zh.draft.stageWriting;
    case "ready":
      return zh.draft.stageFinished;
    case "failed":
      return zh.draft.stageFailed;
    case "cancelled":
      return zh.draft.stageCancelled;
  }
}

/** 后端 stage 是英文枚举，展示前统一转成中文阶段文案。 */
export function draftStageLabel(stage: string | null | undefined): string {
  switch (stage) {
    case "queued":
      return zh.draft.stageQueued;
    case "preparing":
      return zh.draft.stagePreparing;
    case "writing":
      return zh.draft.stageWriting;
    case "finished":
      return zh.draft.stageFinished;
    case "failed":
      return zh.draft.stageFailed;
    case "cancelled":
      return zh.draft.stageCancelled;
    default:
      return zh.draft.stageWriting;
  }
}

function isTerminalStatus(status: DraftStatus): boolean {
  return status === "ready" || status === "failed" || status === "cancelled";
}

function shouldApply(current: Wire<DraftJob> | null, next: Wire<DraftJob>): boolean {
  if (!current || current.id !== next.id) return true;
  if (isTerminalStatus(current.status)) return false;
  if (isTerminalStatus(next.status)) return true;
  return next.progress >= current.progress;
}

function asJob(data: unknown): Wire<DraftJob> | null {
  if (!data || typeof data !== "object" || Array.isArray(data)) return null;
  const candidate = data as Record<string, unknown>;
  if (typeof candidate.id !== "string") return null;
  return candidate as unknown as Wire<DraftJob>;
}

function toBody(prefs: DraftPrefs): api.CreateChapterDraftBody {
  return {
    style_name: prefs.style_name?.trim() || null,
    style_sample: prefs.style_sample?.trim() || null,
    instruction: prefs.instruction?.trim() || null,
    target_words: prefs.target_words,
    // null 表示沿用小说档案（未设置时由后端回退到第三人称）。
    pov: prefs.pov,
  };
}

function closeStream(final = true): void {
  stream?.close(final);
  stream = null;
}

async function refreshGeneratedContent(
  novelId: string,
  job: Wire<DraftJob>,
  seq: number,
  message: string | null,
): Promise<void> {
  const targetChapterId = job.chapter_id;
  if (!targetChapterId) {
    useChapterDraftStore.setState((state) => {
      state.refreshError = zh.draft.failedFallback;
      state.refreshing = false;
    });
    return;
  }

  try {
    const view: Wire<ChapterContent> = await api.getChapterContent(novelId, targetChapterId);
    if (seq !== runSeq) return;
    const contentState = useChapterContentStore.getState();
    if (contentState.novelId === novelId && contentState.chapterId === targetChapterId) {
      contentState.applyContent(view);
    }
    useUiStore.getState().toast({
      tone: message ? "warning" : "success",
      message: message ? zh.draft.generatedWithWarning(message) : zh.draft.doneToast,
    });
  } catch (error) {
    if (seq === runSeq) {
      useChapterDraftStore.setState((state) => {
        state.refreshError = zh.draft.refreshFailed(userMessage(error));
      });
      useUiStore.getState().toast({ tone: "error", message: zh.draft.refreshFailed(userMessage(error)) });
    }
  } finally {
    if (seq === runSeq) {
      useChapterDraftStore.setState((state) => {
        state.refreshing = false;
      });
    }
  }
}

function settleTerminal(
  name: string,
  payload: Wire<DraftJob> | null,
  seq: number,
): void {
  if (seq !== runSeq) return;

  const state = useChapterDraftStore.getState();
  const job = state.job ?? payload;
  const message = job?.message?.trim() || null;

  if (name === "draft.done") {
    const novelId = state.novelId;
    useChapterDraftStore.setState((draft) => {
      draft.connection = "closed";
      draft.startedAt = null;
      draft.stage = "finished";
      draft.progress = 100;
      draft.job = null;
      draft.error = null;
      draft.message = message ?? draft.message;
      draft.refreshing = !!job && !!novelId;
      draft.refreshError = null;
    });
    if (job && novelId) void refreshGeneratedContent(novelId, job, seq, message);
    return;
  }

  const reason = job?.error?.trim() || zh.draft.failedFallback;
  useChapterDraftStore.setState((draft) => {
    draft.connection = "closed";
    draft.startedAt = null;
    draft.stage = "failed";
    draft.job = job?.status === "failed" ? job : null;
    draft.error = reason;
    draft.message = message;
    draft.refreshing = false;
  });
  useUiStore.getState().toast({ tone: "error", message: reason });
}

function connectDraftStream(draftId: string, seq: number): void {
  closeStream(true);
  stream = new EventStream({
    url: () => api.draftEventsUrl(draftId),
    knownEvents: STREAM_EVENTS,
    terminalEvents: TERMINAL_EVENTS,
    onState: (connection) => {
      if (seq !== runSeq) return;
      useChapterDraftStore.setState((state) => {
        state.connection = connection;
        if (connection === "open") state.streamError = null;
      });
    },
    onError: (error) => {
      if (seq !== runSeq) return;
      useChapterDraftStore.setState((state) => {
        state.streamError = userMessage(error);
      });
    },
    onEvent: (name, data) => {
      if (seq !== runSeq) return;
      if (name !== "draft.snapshot" && name !== "draft.progress") return;
      const job = asJob(data);
      if (job) useChapterDraftStore.getState().ingestJob(job);
    },
    onDone: (name, data) => {
      if (seq !== runSeq) return;
      const payload = asJob(data);
      if (payload) useChapterDraftStore.getState().ingestJob(payload);
      closeStream(true);
      settleTerminal(name, payload, seq);
    },
  });
  stream.start();
}

export const useChapterDraftStore = create<ChapterDraftState>()(
  immer((set, get) => ({
    novelId: null,
    chapterId: null,
    job: null,
    creating: false,
    cancelling: false,
    error: null,
    stage: null,
    progress: 0,
    message: null,
    connection: "idle",
    streamError: null,
    startedAt: null,
    refreshing: false,
    refreshError: null,
    prefs: null,

    create: async (novelId, chapterId, prefs) => {
      const current = get();
      if (current.creating || isDraftRunning(current.job)) {
        throw new Error(zh.draft.busy);
      }

      const seq = ++runSeq;
      closeStream(true);
      const idempotencyKey = newIdempotencyKey("draft");

      set((state) => {
        state.novelId = novelId;
        state.chapterId = chapterId;
        state.prefs = prefs;
        state.creating = true;
        state.cancelling = false;
        state.error = null;
        state.stage = "queued";
        state.progress = 0;
        state.message = null;
        state.streamError = null;
        state.startedAt = null;
        state.job = null;
        state.connection = "connecting";
        state.refreshing = false;
        state.refreshError = null;
      });

      try {
        const job = await api.createChapterDraft(novelId, chapterId, toBody(prefs), idempotencyKey);
        if (seq !== runSeq) return;
        set((state) => {
          state.creating = false;
          state.job = job;
          state.stage = job.stage;
          state.progress = job.progress;
          state.message = job.message?.trim() || null;
          state.startedAt = Date.now();
        });
        connectDraftStream(job.id, seq);
      } catch (error) {
        if (seq !== runSeq) throw error;
        set((state) => {
          state.creating = false;
          state.error = userMessage(error);
          state.stage = null;
          state.progress = 0;
          state.startedAt = null;
          state.connection = "idle";
        });
        throw error;
      }
    },

    retry: async () => {
      const { novelId, chapterId, prefs } = get();
      if (!novelId || !chapterId || !prefs) throw new Error(zh.draft.retryUnavailable);
      await get().create(novelId, chapterId, prefs);
    },

    cancel: async () => {
      const job = get().job;
      if (!job) return;

      const seq = ++runSeq;
      closeStream(true);
      set((state) => {
        state.cancelling = true;
        state.connection = "closed";
      });

      try {
        await api.cancelChapterDraft(job.id);
        useUiStore.getState().toast({ tone: "info", message: zh.draft.cancelled });
      } catch (error) {
        useUiStore.getState().toast({ tone: "error", message: userMessage(error) });
      } finally {
        if (seq === runSeq) {
          set((state) => {
            state.cancelling = false;
            state.job = null;
            state.stage = null;
            state.progress = 0;
            state.message = null;
            state.error = null;
            state.startedAt = null;
            state.connection = "idle";
            state.refreshing = false;
            state.refreshError = null;
          });
        }
      }
    },

    ingestJob: (job) =>
      set((state) => {
        if (!shouldApply(state.job, job)) return;
        state.job = job;
        state.stage = job.stage;
        state.progress = job.progress;
        state.message = job.message?.trim() || state.message;
        if (job.status === "failed") {
          state.error = job.error?.trim() || zh.draft.failedFallback;
        }
      }),

    clearRefreshError: () =>
      set((state) => {
        state.refreshError = null;
      }),

    reset: () => {
      ++runSeq;
      closeStream(true);
      set((state) => {
        state.novelId = null;
        state.chapterId = null;
        state.job = null;
        state.creating = false;
        state.cancelling = false;
        state.error = null;
        state.stage = null;
        state.progress = 0;
        state.message = null;
        state.connection = "idle";
        state.streamError = null;
        state.startedAt = null;
        state.refreshing = false;
        state.refreshError = null;
        state.prefs = null;
      });
    },
  })),
);
