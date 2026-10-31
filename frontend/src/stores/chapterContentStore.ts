import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import type { ChapterContent } from "../types/domain";
import type { Wire } from "../types/api";
import * as api from "../api/endpoints";
import { userMessage } from "../lib/errors";
import { countWords } from "../lib/format";
import { useUiStore } from "./uiStore";
import { zh } from "../copy/zh";

type ChapterContentState = {
  novelId: string | null;
  chapterId: string | null;
  /** 服务端已保存的正文快照。 */
  content: string;
  /** 当前编辑中的文本（含未保存改动）。 */
  editable: string;
  wordCount: number;
  loaded: boolean;
  loading: boolean;
  error: string | null;
  saving: boolean;
  dirty: boolean;

  load: (novelId: string, chapterId: string) => Promise<void>;
  setEditable: (text: string) => void;
  save: () => Promise<void>;
  discard: () => void;
  applyContent: (view: Wire<ChapterContent>) => void;
  reset: () => void;
};

let loadSeq = 0;

function isCurrentContext(novelId: string, chapterId: string): boolean {
  const state = useChapterContentStore.getState();
  return state.novelId === novelId && state.chapterId === chapterId;
}

export const useChapterContentStore = create<ChapterContentState>()(
  immer((set, get) => ({
    novelId: null,
    chapterId: null,
    content: "",
    editable: "",
    wordCount: 0,
    loaded: false,
    loading: false,
    error: null,
    saving: false,
    dirty: false,

    load: async (novelId, chapterId) => {
      const seq = ++loadSeq;
      set((state) => {
        state.novelId = novelId;
        state.chapterId = chapterId;
        state.content = "";
        state.editable = "";
        state.wordCount = 0;
        state.loaded = false;
        state.loading = true;
        state.error = null;
        state.saving = false;
        state.dirty = false;
      });

      try {
        const result = await api.getChapterContent(novelId, chapterId);
        if (seq !== loadSeq) return;
        set((state) => {
          state.content = result.content;
          state.editable = result.content;
          state.wordCount = result.word_count;
          state.loaded = true;
          state.loading = false;
        });
      } catch (error) {
        if (seq !== loadSeq) return;
        set((state) => {
          state.loading = false;
          state.loaded = true;
          state.error = userMessage(error);
        });
      }
    },

    setEditable: (text) =>
      set((state) => {
        state.editable = text;
        state.dirty = text !== state.content;
      }),

    save: async () => {
      const { novelId, chapterId, editable, saving } = get();
      if (!novelId || !chapterId || saving) return;

      const toSave = editable;
      if (!toSave.trim()) {
        const message = zh.outline.contentRequired;
        set((state) => {
          state.error = message;
        });
        throw new Error(message);
      }
      const contextSeq = loadSeq;

      set((state) => {
        state.saving = true;
        state.error = null;
      });

      try {
        await api.saveChapterContent(novelId, chapterId, { content: toSave });
        if (!isCurrentContext(novelId, chapterId) || contextSeq !== loadSeq) return;
        const savedWordCount = countWords(toSave);
        set((state) => {
          state.content = toSave;
          state.wordCount = savedWordCount;
          state.saving = false;
          // 保存期间用户继续输入时，不覆盖新内容，只更新服务端快照。
          if (state.editable === toSave) {
            state.editable = toSave;
            state.dirty = false;
          } else {
            state.dirty = state.editable !== toSave;
          }
        });
        useUiStore.getState().toast({ tone: "success", message: zh.outline.contentSaved });
      } catch (error) {
        if (!isCurrentContext(novelId, chapterId)) throw error;
        set((state) => {
          state.saving = false;
          state.error = userMessage(error);
        });
        throw error;
      }
    },

    discard: () =>
      set((state) => {
        state.editable = state.content;
        state.dirty = false;
      }),

    applyContent: (view) => {
      // 服务端刚写入正文：让仍在飞行的旧读取请求失效，避免用旧快照覆盖新正文。
      ++loadSeq;
      set((state) => {
        state.content = view.content;
        state.editable = view.content;
        state.wordCount = view.word_count;
        state.loaded = true;
        state.loading = false;
        state.error = null;
        state.saving = false;
        state.dirty = false;
      });
    },

    reset: () => {
      ++loadSeq;
      set((state) => {
        state.novelId = null;
        state.chapterId = null;
        state.content = "";
        state.editable = "";
        state.wordCount = 0;
        state.loaded = false;
        state.loading = false;
        state.error = null;
        state.saving = false;
        state.dirty = false;
      });
    },
  })),
);
