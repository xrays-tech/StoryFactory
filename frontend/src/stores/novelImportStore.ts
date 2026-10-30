import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import * as api from "../api/endpoints";
import { apiUrl } from "../api/client";
import { EventStream } from "../api/eventStream";
import type { Wire } from "../types/api";
import type {
  NovelDto,
  NovelImportJob,
  NovelImportPreview,
  NovelImportStatus,
} from "../types/domain";
import { userMessage } from "../lib/errors";

export type ImportJob = Wire<NovelImportJob>;
export type ImportPreview = Wire<NovelImportPreview>;
export type ImportNovel = Wire<NovelDto>;

const MAX_FILE_BYTES = 32 * 1024 * 1024;

const KNOWN_EVENTS = new Set<string>([
  "import.snapshot",
  "import.progress",
  "import.done",
  "import.failed",
]);

const TERMINAL_EVENTS = new Set<string>(["import.done", "import.failed"]);

const IMPORT_STATUSES: ReadonlySet<string> = new Set<NovelImportStatus>([
  "queued",
  "analyzing",
  "ready",
  "failed",
  "cancelled",
]);

type NovelImportState = {
  file: File | null;
  job: ImportJob | null;
  preview: ImportPreview | null;
  loading: boolean;
  uploading: boolean;
  creating: boolean;
  error: string | null;
  connecting: boolean;
  streamError: string | null;
  upload: (file: File) => Promise<void>;
  connect: (jobId: string) => void;
  disconnect: () => void;
  refresh: (jobId: string) => Promise<void>;
  loadPreview: () => Promise<void>;
  createProject: (title?: string) => Promise<ImportNovel>;
  cancel: () => Promise<void>;
  reset: () => void;
};

let stream: EventStream | null = null;
let importSeq = 0;
let jobSeq = 0;
let previewSeq = 0;
let streamSeq = 0;

function closeStream(): void {
  streamSeq += 1;
  stream?.close(true);
  stream = null;
}

export function validateImportFile(file: File): string | null {
  if (!file.name.toLowerCase().endsWith(".txt")) return "只支持 .txt 文本文件。";
  if (file.size === 0) return "文件是空的，请换一个有内容的 txt。";
  if (file.size > MAX_FILE_BYTES) return "文件超过 32MB 上限，请先拆分再导入。";
  return null;
}

function parseJob(data: unknown): ImportJob | null {
  if (!data || typeof data !== "object" || Array.isArray(data)) return null;
  const candidate = data as Partial<ImportJob>;
  if (typeof candidate.id !== "string" || candidate.id === "") return null;
  if (typeof candidate.status !== "string" || !IMPORT_STATUSES.has(candidate.status)) return null;
  if (typeof candidate.stage !== "string") return null;
  return candidate as ImportJob;
}

const initialState = {
  file: null as File | null,
  job: null as ImportJob | null,
  preview: null as ImportPreview | null,
  loading: false,
  uploading: false,
  creating: false,
  error: null as string | null,
  connecting: false,
  streamError: null as string | null,
};

export const useNovelImportStore = create<NovelImportState>()(
  immer((set, get) => ({
    ...initialState,

    upload: async (file) => {
      const problem = validateImportFile(file);
      if (problem) {
        set((state) => {
          state.file = null;
          state.error = problem;
        });
        return;
      }

      closeStream();
      const seq = ++importSeq;
      set((state) => {
        state.file = file;
        state.job = null;
        state.preview = null;
        state.uploading = true;
        state.creating = false;
        state.error = null;
        state.connecting = false;
        state.streamError = null;
      });

      try {
        const job = await api.createNovelImport(file);
        if (seq !== importSeq) return;
        set((state) => {
          state.job = job;
          state.uploading = false;
        });
        get().connect(job.id);
      } catch (error) {
        if (seq !== importSeq) return;
        set((state) => {
          state.uploading = false;
          state.error = userMessage(error);
        });
      }
    },

    connect: (jobId) => {
      closeStream();
      const seq = ++streamSeq;
      set((state) => {
        state.connecting = true;
        state.streamError = null;
      });

      const next = new EventStream({
        url: () => apiUrl(api.paths.novelImportEvents(jobId)),
        knownEvents: KNOWN_EVENTS,
        terminalEvents: TERMINAL_EVENTS,
        onState: (connection) => {
          if (seq !== streamSeq) return;
          set((state) => {
            state.connecting = connection === "connecting" || connection === "reconnecting" || connection === "idle";
          });
        },
        onOpen: () => {
          if (seq !== streamSeq) return;
          set((state) => {
            state.connecting = false;
            state.streamError = null;
          });
        },
        onReconnect: () => {
          if (seq !== streamSeq) return;
          set((state) => {
            state.connecting = true;
          });
        },
        onError: (error) => {
          if (seq !== streamSeq) return;
          set((state) => {
            state.streamError = userMessage(error);
          });
        },
        onEvent: (_name, data) => {
          if (seq !== streamSeq) return;
          const job = parseJob(data);
          if (!job) return;
          const isTerminalJob =
            job.status === "ready" || job.status === "failed" || job.status === "cancelled";
          set((state) => {
            state.job = job;
            state.streamError = null;
            if (job.status === "failed" || job.status === "cancelled") {
              state.error =
                job.error ?? job.message ?? "分析未完成，请重新上传。";
            }
            if (isTerminalJob) state.connecting = false;
          });
          if (!isTerminalJob) return;
          // 终态快照之后服务端可能直接关闭连接，主动收流避免无意义重连。
          closeStream();
          if (job.status === "ready") void get().loadPreview();
        },
        onDone: (name, data) => {
          if (seq !== streamSeq) return;
          const job = parseJob(data);
          const finalJob = job ?? get().job;
          set((state) => {
            state.connecting = false;
            if (job) state.job = job;
            const failed =
              name === "import.failed" ||
              finalJob?.status === "failed" ||
              finalJob?.status === "cancelled";
            if (failed) {
              state.error =
                finalJob?.error ?? finalJob?.message ?? "分析未完成，请重新上传。";
            }
          });
          if (finalJob?.status === "ready") void get().loadPreview();
        },
      });

      stream = next;
      next.start();
    },

    disconnect: () => {
      closeStream();
      set((state) => {
        state.connecting = false;
      });
    },

    refresh: async (jobId) => {
      const seq = ++jobSeq;
      try {
        const job = await api.getNovelImport(jobId);
        if (seq !== jobSeq) return;
        set((state) => {
          state.job = job;
          state.error = null;
        });
        if (job.status === "ready") void get().loadPreview();
      } catch (error) {
        if (seq !== jobSeq) return;
        set((state) => {
          state.error = userMessage(error);
        });
      }
    },

    loadPreview: async () => {
      const job = get().job;
      if (!job || get().loading) return;
      const seq = ++previewSeq;
      set((state) => {
        state.loading = true;
        state.error = null;
      });
      try {
        const preview = await api.getNovelImportPreview(job.id);
        if (seq !== previewSeq) return;
        set((state) => {
          state.preview = preview;
          state.loading = false;
        });
      } catch (error) {
        if (seq !== previewSeq) return;
        set((state) => {
          state.loading = false;
          state.error = userMessage(error);
        });
      }
    },

    createProject: async (title) => {
      const job = get().job;
      if (!job) throw new Error("还没有可生成的分析任务。");
      if (job.status !== "ready") throw new Error("分析尚未完成，暂时不能生成项目。");

      set((state) => {
        state.creating = true;
        state.error = null;
      });
      try {
        const novel = await api.createNovelFromImport(job.id, {
          title: title?.trim() || null,
        });
        set((state) => {
          state.creating = false;
          if (state.job) state.job.novel_id = novel.id;
        });
        return novel;
      } catch (error) {
        set((state) => {
          state.creating = false;
          state.error = userMessage(error);
        });
        throw error;
      }
    },

    cancel: async () => {
      const job = get().job;
      if (!job) return;
      try {
        await api.cancelNovelImport(job.id);
        closeStream();
        set((state) => {
          state.connecting = false;
          if (state.job) {
            state.job.status = "cancelled";
            state.job.stage = "failed";
            state.job.message = "已取消";
          }
        });
      } catch (error) {
        set((state) => {
          state.error = userMessage(error);
        });
        throw error;
      }
    },

    reset: () => {
      importSeq += 1;
      jobSeq += 1;
      previewSeq += 1;
      closeStream();
      set(() => ({ ...initialState }));
    },
  })),
);
