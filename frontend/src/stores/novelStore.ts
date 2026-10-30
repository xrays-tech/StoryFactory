import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import type { CreateNovelReq, NovelDto, NovelStatus, UpdateNovelReq } from "../types/domain";
import type { NovelListQuery, Wire } from "../types/api";
import * as api from "../api/endpoints";
import { userMessage } from "../lib/errors";

export type Novel = Wire<NovelDto>;

export type NovelQuery = {
  q: string;
  status: NovelStatus | "";
  page: number;
  pageSize: number;
};

type NovelState = {
  items: Novel[];
  total: number;
  page: number;
  pageSize: number;
  query: NovelQuery;
  loading: boolean;
  error: string | null;
  currentId: string | null;
  current: Novel | null;
  currentLoading: boolean;
  currentError: string | null;
  load: (query?: Partial<NovelListQuery>) => Promise<void>;
  loadOne: (id: string, options?: { silent?: boolean }) => Promise<Novel | null>;
  setQuery: (query: Partial<NovelQuery>) => void;
  create: (req: Wire<CreateNovelReq>) => Promise<Novel>;
  update: (id: string, req: Partial<Wire<UpdateNovelReq>>) => Promise<Novel>;
  remove: (id: string) => Promise<void>;
  clearCurrent: () => void;
};

let listSeq = 0;

export const useNovelStore = create<NovelState>()(
  immer((set, get) => ({
    items: [],
    total: 0,
    page: 1,
    pageSize: 24,
    query: { q: "", status: "", page: 1, pageSize: 24 },
    loading: false,
    error: null,
    currentId: null,
    current: null,
    currentLoading: false,
    currentError: null,

    load: async (patch = {}) => {
      const next: NovelQuery = {
        ...get().query,
        ...(patch.q !== undefined ? { q: patch.q } : {}),
        ...(patch.status !== undefined ? { status: patch.status } : {}),
        ...(patch.page !== undefined ? { page: patch.page } : {}),
        ...(patch.pageSize !== undefined ? { pageSize: patch.pageSize } : {}),
      };
      const seq = ++listSeq;
      set((state) => {
        state.query = next;
        state.loading = true;
        state.error = null;
      });
      try {
        const page = await api.listNovels({
          q: next.q,
          status: next.status,
          page: next.page,
          pageSize: next.pageSize,
        });
        if (seq !== listSeq) return;
        set((state) => {
          state.items = page.items;
          state.total = page.total;
          state.page = page.page;
          state.pageSize = page.page_size;
          state.loading = false;
        });
      } catch (error) {
        if (seq !== listSeq) return;
        set((state) => {
          state.loading = false;
          state.error = userMessage(error);
        });
      }
    },

    setQuery: (patch) =>
      set((state) => {
        state.query = { ...state.query, ...patch };
      }),

    loadOne: async (id, options = {}) => {
      set((state) => {
        state.currentId = id;
        if (!options.silent) {
          state.currentLoading = true;
          state.currentError = null;
        }
      });
      try {
        const novel = await api.getNovel(id);
        set((state) => {
          state.current = novel;
          state.currentId = novel.id;
          state.currentLoading = false;
          state.currentError = null;
          const idx = state.items.findIndex((n) => n.id === novel.id);
          if (idx >= 0) state.items[idx] = novel;
        });
        return novel;
      } catch (error) {
        set((state) => {
          state.currentLoading = false;
          state.currentError = userMessage(error);
        });
        return null;
      }
    },

    create: async (req) => {
      const novel = await api.createNovel(req);
      set((state) => {
        state.total += 1;
        state.items.unshift(novel);
      });
      return novel;
    },

    update: async (id, req) => {
      const novel = await api.updateNovel(id, req);
      set((state) => {
        if (state.current?.id === id) state.current = novel;
        const idx = state.items.findIndex((n) => n.id === id);
        if (idx >= 0) state.items[idx] = novel;
      });
      return novel;
    },

    remove: async (id) => {
      await api.deleteNovel(id);
      set((state) => {
        state.items = state.items.filter((n) => n.id !== id);
        state.total = Math.max(0, state.total - 1);
        if (state.currentId === id) {
          state.currentId = null;
          state.current = null;
        }
      });
    },

    clearCurrent: () =>
      set((state) => {
        state.currentId = null;
        state.current = null;
        state.currentError = null;
      }),
  })),
);
