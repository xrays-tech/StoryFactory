import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DraftJob } from "../types/domain";
import type { Wire } from "../types/api";

const harness = vi.hoisted(() => ({
  createChapterDraft: vi.fn(),
  cancelChapterDraft: vi.fn(),
  getChapterContent: vi.fn(),
  streamConfig: { current: null as StreamConfig | null },
}));

type StreamConfig = {
  onEvent?: (name: string, data: unknown) => void;
  onDone?: (name: string, data: unknown) => void;
};

vi.mock("../api/endpoints", () => ({
  createChapterDraft: harness.createChapterDraft,
  cancelChapterDraft: harness.cancelChapterDraft,
  getChapterContent: harness.getChapterContent,
  draftEventsUrl: (id: string) => `/api/v1/chapter-drafts/${id}/events`,
}));

vi.mock("../api/eventStream", () => ({
  EventStream: class {
    constructor(config: StreamConfig) {
      harness.streamConfig.current = config;
    }
    start() {}
    close() {}
  },
}));

import { useChapterDraftStore, isDraftRunning, type DraftPrefs } from "./chapterDraftStore";
import { useChapterContentStore } from "./chapterContentStore";

type Job = Wire<DraftJob>;

const prefs: DraftPrefs = {
  style_name: null,
  style_sample: "范文",
  instruction: null,
  target_words: 1200,
  pov: "第三人称",
};

function job(overrides: Partial<Job> = {}): Job {
  return {
    id: "d1",
    chapter_id: "c1",
    status: "queued",
    stage: "queued",
    progress: 0,
    style_name: null,
    target_words: 1200,
    message: null,
    error: null,
    created_at: 1,
    updated_at: 1,
    ...overrides,
  };
}

function resetStores() {
  useChapterDraftStore.getState().reset();
  useChapterContentStore.getState().reset();
  useChapterContentStore.setState({ novelId: "n1", chapterId: "c1", loaded: true });
  harness.streamConfig.current = null;
}

function lastStream(): StreamConfig {
  const config = harness.streamConfig.current;
  if (!config) throw new Error("SSE 尚未连接");
  return config;
}

describe("useChapterDraftStore.create", () => {
  beforeEach(() => {
    harness.createChapterDraft.mockReset();
    harness.cancelChapterDraft.mockReset();
    harness.getChapterContent.mockReset();
    resetStores();
  });

  it("submits prefs with an idempotency key and starts streaming", async () => {
    harness.createChapterDraft.mockResolvedValue(job({ status: "generating", stage: "preparing", progress: 5 }));

    await useChapterDraftStore.getState().create("n1", "c1", prefs);

    expect(harness.createChapterDraft).toHaveBeenCalledWith(
      "n1",
      "c1",
      {
        style_name: null,
        style_sample: "范文",
        instruction: null,
        target_words: 1200,
        pov: null,
      },
      expect.stringMatching(/^draft-/),
    );
    const state = useChapterDraftStore.getState();
    expect(state.job?.id).toBe("d1");
    expect(isDraftRunning(state.job)).toBe(true);
    expect(state.creating).toBe(false);
    expect(state.startedAt).not.toBeNull();
    expect(harness.streamConfig.current).not.toBeNull();
  });

  it("uses a fresh idempotency key on retry", async () => {
    harness.createChapterDraft.mockResolvedValue(job({ status: "failed", error: "先失败一次" }));

    await useChapterDraftStore.getState().create("n1", "c1", prefs);
    await useChapterDraftStore.getState().retry();

    const firstKey = harness.createChapterDraft.mock.calls[0]?.[3];
    const secondKey = harness.createChapterDraft.mock.calls[1]?.[3];
    expect(firstKey).toBeTruthy();
    expect(secondKey).toBeTruthy();
    expect(secondKey).not.toBe(firstKey);
  });

  it("records creation failures", async () => {
    harness.createChapterDraft.mockRejectedValue(new Error("创建失败"));

    await expect(useChapterDraftStore.getState().create("n1", "c1", prefs)).rejects.toThrow("创建失败");

    const state = useChapterDraftStore.getState();
    expect(state.error).toBe("创建失败");
    expect(state.job).toBeNull();
    expect(state.creating).toBe(false);
  });

  it("rejects duplicate submission while a job is running", async () => {
    harness.createChapterDraft.mockResolvedValue(job({ status: "generating", progress: 5 }));
    await useChapterDraftStore.getState().create("n1", "c1", prefs);

    await expect(useChapterDraftStore.getState().create("n1", "c2", prefs)).rejects.toThrow(
      "已有生成任务在进行中",
    );
    expect(harness.createChapterDraft).toHaveBeenCalledTimes(1);
  });
});

describe("useChapterDraftStore.ingestJob", () => {
  beforeEach(() => {
    harness.createChapterDraft.mockReset();
    resetStores();
  });

  it("ignores stale progress and never leaves a terminal state", async () => {
    harness.createChapterDraft.mockResolvedValue(job({ status: "generating", progress: 5 }));
    await useChapterDraftStore.getState().create("n1", "c1", prefs);

    const stream = lastStream();
    stream.onEvent?.("draft.progress", job({ status: "generating", progress: 40 }));
    stream.onEvent?.("draft.progress", job({ status: "generating", progress: 20 }));
    expect(useChapterDraftStore.getState().progress).toBe(40);

    stream.onEvent?.("draft.progress", job({ status: "ready", progress: 100 }));
    stream.onEvent?.("draft.progress", job({ status: "generating", progress: 60 }));
    expect(useChapterDraftStore.getState().job?.status).toBe("ready");
    expect(useChapterDraftStore.getState().progress).toBe(100);
  });
});

describe("useChapterDraftStore terminal events", () => {
  beforeEach(() => {
    harness.createChapterDraft.mockReset();
    harness.getChapterContent.mockReset();
    resetStores();
  });

  it("pulls generated content and keeps a warning message", async () => {
    harness.createChapterDraft.mockResolvedValue(job({ status: "generating", progress: 5 }));
    harness.getChapterContent.mockResolvedValue({ content: "生成好的正文", word_count: 6 });
    await useChapterDraftStore.getState().create("n1", "c1", prefs);

    lastStream().onDone?.(
      "draft.done",
      job({ status: "ready", stage: "finished", progress: 100, message: "已跳过 1 场" }),
    );

    await vi.waitFor(() => {
      expect(useChapterContentStore.getState().content).toBe("生成好的正文");
    });
    const state = useChapterDraftStore.getState();
    expect(state.job).toBeNull();
    expect(state.message).toBe("已跳过 1 场");
    expect(state.refreshing).toBe(false);
  });

  it("surfaces the failure reason for retry", async () => {
    harness.createChapterDraft.mockResolvedValue(job({ status: "generating", progress: 5 }));
    await useChapterDraftStore.getState().create("n1", "c1", prefs);

    lastStream().onDone?.("draft.failed", job({ status: "failed", stage: "failed", error: "模型超时" }));

    const state = useChapterDraftStore.getState();
    expect(state.error).toBe("模型超时");
    expect(state.job?.status).toBe("failed");
    expect(state.startedAt).toBeNull();
  });

  it("keeps a content read failure separate from generation failure", async () => {
    harness.createChapterDraft.mockResolvedValue(job({ status: "generating", progress: 5 }));
    harness.getChapterContent.mockRejectedValue(new Error("网络错误"));
    await useChapterDraftStore.getState().create("n1", "c1", prefs);

    lastStream().onDone?.("draft.done", job({ status: "ready", progress: 100 }));

    await vi.waitFor(() => {
      expect(useChapterDraftStore.getState().refreshError).toContain("网络错误");
    });
    const state = useChapterDraftStore.getState();
    expect(state.error).toBeNull();
    expect(state.refreshing).toBe(false);

    useChapterDraftStore.getState().clearRefreshError();
    expect(useChapterDraftStore.getState().refreshError).toBeNull();
  });
});

describe("useChapterDraftStore.cancel", () => {
  beforeEach(() => {
    harness.createChapterDraft.mockReset();
    harness.cancelChapterDraft.mockReset();
    resetStores();
  });

  it("calls the cancel endpoint and clears the job", async () => {
    harness.createChapterDraft.mockResolvedValue(job({ status: "generating", progress: 5 }));
    harness.cancelChapterDraft.mockResolvedValue(undefined);
    await useChapterDraftStore.getState().create("n1", "c1", prefs);

    await useChapterDraftStore.getState().cancel();

    expect(harness.cancelChapterDraft).toHaveBeenCalledWith("d1");
    const state = useChapterDraftStore.getState();
    expect(state.job).toBeNull();
    expect(state.startedAt).toBeNull();
  });
});
