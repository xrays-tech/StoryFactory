import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import type {
  ChapterDto,
  CreateChapterReq,
  CreateVolumeReq,
  UpdateChapterReq,
  UpdateVolumeReq,
  VolumeDto,
} from "../types/domain";
import type { Wire } from "../types/api";
import * as api from "../api/endpoints";
import { userMessage } from "../lib/errors";

export type Volume = Wire<VolumeDto>;
export type Chapter = Wire<ChapterDto>;

type OutlineState = {
  novelId: string | null;
  volumes: Volume[];
  chaptersByVolume: Record<string, Chapter[]>;
  loading: boolean;
  error: string | null;
  selectedChapterId: string | null;
  chapterLoading: boolean;
  chapterError: string | null;
  saving: boolean;
  loadTree: (novelId: string, options?: { silent?: boolean }) => Promise<void>;
  createVolume: (novelId: string, req: Wire<CreateVolumeReq>) => Promise<Volume>;
  updateVolume: (novelId: string, id: string, req: Partial<Wire<UpdateVolumeReq>>) => Promise<Volume>;
  moveVolume: (novelId: string, id: string, direction: "up" | "down") => Promise<void>;
  deleteVolume: (novelId: string, id: string) => Promise<void>;
  createChapter: (novelId: string, volumeId: string, req: Wire<CreateChapterReq>) => Promise<Chapter>;
  updateChapter: (novelId: string, chapterId: string, req: Partial<Wire<UpdateChapterReq>>) => Promise<Chapter>;
  refreshChapter: (novelId: string, chapterId: string) => Promise<Chapter | null>;
  deleteChapter: (novelId: string, chapterId: string) => Promise<void>;
  selectChapter: (id: string | null) => void;
  clear: () => void;
};

let treeSeq = 0;

function sortVolumes(volumes: Volume[]): Volume[] {
  return [...volumes].sort((a, b) => a.volume_index - b.volume_index);
}

function sortChapters(chapters: Chapter[]): Chapter[] {
  return [...chapters].sort((a, b) => a.chapter_index - b.chapter_index);
}

export const useOutlineStore = create<OutlineState>()(
  immer((set, get) => ({
    novelId: null,
    volumes: [],
    chaptersByVolume: {},
    loading: false,
    error: null,
    selectedChapterId: null,
    chapterLoading: false,
    chapterError: null,
    saving: false,

    loadTree: async (novelId, options = {}) => {
      const seq = ++treeSeq;
      set((state) => {
        if (state.novelId !== novelId) {
          state.volumes = [];
          state.chaptersByVolume = {};
          state.selectedChapterId = null;
        }
        state.novelId = novelId;
        if (!options.silent) {
          state.loading = true;
          state.error = null;
        }
      });
      try {
        const volumes = await api.listVolumes(novelId);
        const chapterPages = await Promise.all(
          volumes.map((volume) =>
            api.listChapters(novelId, volume.id, { page: 1, pageSize: 100 }),
          ),
        );
        if (seq !== treeSeq) return;
        const map: Record<string, Chapter[]> = {};
        volumes.forEach((volume, index) => {
          map[volume.id] = chapterPages[index].items;
        });
        set((state) => {
          state.volumes = volumes;
          state.chaptersByVolume = map;
          state.loading = false;
          if (
            state.selectedChapterId &&
            !Object.values(map).some((list) =>
              list.some((chapter) => chapter.id === state.selectedChapterId),
            )
          ) {
            state.selectedChapterId = null;
          }
        });
      } catch (error) {
        if (seq !== treeSeq) return;
        set((state) => {
          state.loading = false;
          state.error = userMessage(error);
        });
      }
    },

    createVolume: async (novelId, req) => {
      const volume = await api.createVolume(novelId, req);
      set((state) => {
        state.volumes = sortVolumes([...state.volumes, volume]);
        state.chaptersByVolume[volume.id] = [];
      });
      return volume;
    },

    updateVolume: async (novelId, id, req) => {
      const volume = await api.updateVolume(novelId, id, req);
      set((state) => {
        const idx = state.volumes.findIndex((v) => v.id === id);
        if (idx >= 0) state.volumes[idx] = volume;
        state.volumes = sortVolumes(state.volumes);
      });
      return volume;
    },

    moveVolume: async (novelId, id, direction) => {
      const volumes = await api.moveVolume(novelId, id, { direction });
      set((state) => {
        state.volumes = sortVolumes(volumes);
      });
    },

    deleteVolume: async (novelId, id) => {
      await api.deleteVolume(novelId, id);
      set((state) => {
        state.volumes = state.volumes.filter((v) => v.id !== id);
        delete state.chaptersByVolume[id];
        if (
          state.selectedChapterId &&
          !Object.values(state.chaptersByVolume).some((list) =>
            list.some((chapter) => chapter.id === state.selectedChapterId),
          )
        ) {
          state.selectedChapterId = null;
        }
      });
    },

    createChapter: async (novelId, volumeId, req) => {
      const chapter = await api.createChapter(novelId, volumeId, req);
      set((state) => {
        const list = state.chaptersByVolume[volumeId] ?? [];
        state.chaptersByVolume[volumeId] = sortChapters([...list, chapter]);
        state.selectedChapterId = chapter.id;
      });
      return chapter;
    },

    updateChapter: async (novelId, chapterId, req) => {
      set((state) => {
        state.saving = true;
        state.chapterError = null;
      });
      try {
        const chapter = await api.updateChapter(novelId, chapterId, req);
        set((state) => {
          for (const [volumeId, list] of Object.entries(state.chaptersByVolume)) {
            const idx = list.findIndex((c) => c.id === chapterId);
            if (idx >= 0) {
              if (chapter.volume_id !== volumeId) {
                list.splice(idx, 1);
              } else {
                list[idx] = chapter;
              }
            }
          }
          const target = state.chaptersByVolume[chapter.volume_id] ?? [];
          if (!target.some((c) => c.id === chapter.id)) target.push(chapter);
          state.chaptersByVolume[chapter.volume_id] = sortChapters(target);
          state.saving = false;
        });
        return chapter;
      } catch (error) {
        set((state) => {
          state.saving = false;
          state.chapterError = userMessage(error);
        });
        throw error;
      }
    },

    refreshChapter: async (novelId, chapterId) => {
      set((state) => {
        state.chapterLoading = true;
        state.chapterError = null;
      });
      try {
        const chapter = await api.getChapter(novelId, chapterId);
        set((state) => {
          const list = state.chaptersByVolume[chapter.volume_id] ?? [];
          const idx = list.findIndex((c) => c.id === chapter.id);
          if (idx >= 0) list[idx] = chapter;
          else list.push(chapter);
          state.chaptersByVolume[chapter.volume_id] = sortChapters(list);
          state.chapterLoading = false;
        });
        return chapter;
      } catch (error) {
        set((state) => {
          state.chapterLoading = false;
          state.chapterError = userMessage(error);
        });
        return null;
      }
    },

    deleteChapter: async (novelId, chapterId) => {
      await api.deleteChapter(novelId, chapterId);
      set((state) => {
        for (const [volumeId, list] of Object.entries(state.chaptersByVolume)) {
          state.chaptersByVolume[volumeId] = list.filter((c) => c.id !== chapterId);
        }
        if (state.selectedChapterId === chapterId) state.selectedChapterId = null;
      });
    },

    selectChapter: (id) =>
      set((state) => {
        state.selectedChapterId = id;
        state.chapterError = null;
      }),

    clear: () =>
      set((state) => {
        state.novelId = null;
        state.volumes = [];
        state.chaptersByVolume = {};
        state.error = null;
        state.selectedChapterId = null;
      }),
  })),
);
