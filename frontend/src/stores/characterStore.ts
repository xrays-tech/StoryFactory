import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import type {
  CharacterDto,
  CreateCharacterReq,
  RelationDto,
  UpdateCharacterReq,
} from "../types/domain";
import type { Wire } from "../types/api";
import * as api from "../api/endpoints";
import { userMessage } from "../lib/errors";

export type Character = Wire<CharacterDto>;
export type Relation = Wire<RelationDto>;

export function orderedPair(a: string, b: string): [string, string] {
  return a < b ? [a, b] : [b, a];
}

export function relationKey(a: string, b: string): string {
  const [x, y] = orderedPair(a, b);
  return `${x}::${y}`;
}

function buildMatrix(characters: Character[], source: Relation[]): Relation[] {
  const byKey = new Map<string, Relation>();
  for (const rel of source) {
    byKey.set(relationKey(rel.char_a_id, rel.char_b_id), rel);
  }
  const out: Relation[] = [];
  for (let i = 0; i < characters.length; i += 1) {
    for (let j = i + 1; j < characters.length; j += 1) {
      const [a, b] = orderedPair(characters[i].id, characters[j].id);
      const key = relationKey(a, b);
      const existing = byKey.get(key);
      out.push({
        char_a_id: a,
        char_b_id: b,
        affinity: existing?.affinity ?? 0,
        trust: existing?.trust ?? 0,
        conflict_desc: existing?.conflict_desc ?? null,
      });
    }
  }
  return out;
}

export type RelationField = "affinity" | "trust" | "conflict_desc";

type CharacterState = {
  novelId: string | null;
  items: Character[];
  total: number;
  loading: boolean;
  error: string | null;
  relationsSaved: Relation[];
  relationsDraft: Relation[];
  relationsLoaded: boolean;
  relationsLoading: boolean;
  relationsSaving: boolean;
  relationsError: string | null;
  relationsDirty: boolean;
  load: (novelId: string, options?: { page?: number; pageSize?: number; silent?: boolean }) => Promise<void>;
  loadRelations: (novelId: string) => Promise<void>;
  create: (novelId: string, req: Wire<CreateCharacterReq>) => Promise<Character>;
  update: (novelId: string, id: string, req: Partial<Wire<UpdateCharacterReq>>) => Promise<Character>;
  remove: (novelId: string, id: string) => Promise<void>;
  setRelationField: (a: string, b: string, field: RelationField, value: number | string | null) => void;
  saveRelations: () => Promise<void>;
  resetRelations: () => void;
  applyEmotionDelta: (pair: [string, string], affinityDelta: number, trustDelta: number) => void;
  syncMatrix: () => void;
  clear: () => void;
};

let loadSeq = 0;

export const useCharacterStore = create<CharacterState>()(
  immer((set, get) => ({
    novelId: null,
    items: [],
    total: 0,
    loading: false,
    error: null,
    relationsSaved: [],
    relationsDraft: [],
    relationsLoaded: false,
    relationsLoading: false,
    relationsSaving: false,
    relationsError: null,
    relationsDirty: false,

    load: async (novelId, options = {}) => {
      const seq = ++loadSeq;
      set((state) => {
        if (state.novelId !== novelId) {
          state.items = [];
          state.relationsSaved = [];
          state.relationsDraft = [];
          state.relationsLoaded = false;
        }
        state.novelId = novelId;
        if (!options.silent) {
          state.loading = true;
          state.error = null;
        }
      });
      try {
        const page = await api.listCharacters(novelId, {
          page: options.page ?? 1,
          pageSize: options.pageSize ?? 100,
        });
        if (seq !== loadSeq) return;
        set((state) => {
          state.items = page.items;
          state.total = page.total;
          state.loading = false;
          if (state.relationsLoaded) {
            state.relationsDraft = buildMatrix(state.items, state.relationsSaved);
          }
        });
      } catch (error) {
        if (seq !== loadSeq) return;
        set((state) => {
          state.loading = false;
          state.error = userMessage(error);
        });
      }
    },

    loadRelations: async (novelId) => {
      set((state) => {
        state.novelId = novelId;
        state.relationsLoading = true;
        state.relationsError = null;
        state.relationsDirty = false;
      });
      try {
        const saved = await api.getRelations(novelId);
        set((state) => {
          state.relationsSaved = saved;
          state.relationsDraft = buildMatrix(state.items, saved);
          state.relationsLoaded = true;
          state.relationsLoading = false;
        });
      } catch (error) {
        set((state) => {
          state.relationsLoading = false;
          state.relationsError = userMessage(error);
        });
      }
    },

    create: async (novelId, req) => {
      const character = await api.createCharacter(novelId, req);
      set((state) => {
        state.items.push(character);
        state.total += 1;
        if (state.relationsLoaded) {
          state.relationsDraft = buildMatrix(state.items, state.relationsSaved);
        }
      });
      return character;
    },

    update: async (novelId, id, req) => {
      const character = await api.updateCharacter(novelId, id, req);
      set((state) => {
        const idx = state.items.findIndex((c) => c.id === id);
        if (idx >= 0) state.items[idx] = character;
      });
      return character;
    },

    remove: async (novelId, id) => {
      await api.deleteCharacter(novelId, id);
      set((state) => {
        state.items = state.items.filter((c) => c.id !== id);
        state.total = Math.max(0, state.total - 1);
        if (state.relationsLoaded) {
          state.relationsDraft = buildMatrix(state.items, state.relationsSaved);
        }
      });
    },

    setRelationField: (a, b, field, value) => {
      const key = relationKey(a, b);
      set((state) => {
        let relation = state.relationsDraft.find(
          (r) => relationKey(r.char_a_id, r.char_b_id) === key,
        );
        if (!relation) {
          const [x, y] = orderedPair(a, b);
          relation = { char_a_id: x, char_b_id: y, affinity: 0, trust: 0, conflict_desc: null };
          state.relationsDraft.push(relation);
        }
        if (field === "conflict_desc") {
          relation.conflict_desc = value == null ? null : String(value);
        } else {
          const numeric = typeof value === "number" ? value : Number(value);
          const next = Number.isFinite(numeric) ? Math.round(numeric) : 0;
          relation[field] = field === "affinity" ? Math.max(-100, Math.min(100, next)) : Math.max(0, Math.min(100, next));
        }
        state.relationsDirty = true;
      });
    },

    saveRelations: async () => {
      const { novelId, relationsDraft } = get();
      if (!novelId) return;
      set((state) => {
        state.relationsSaving = true;
        state.relationsError = null;
      });
      try {
        const saved = await api.putRelations(novelId, {
          relations: relationsDraft.map((r) => ({
            char_a_id: r.char_a_id,
            char_b_id: r.char_b_id,
            affinity: r.affinity,
            trust: r.trust,
            conflict_desc: r.conflict_desc,
          })),
        });
        set((state) => {
          state.relationsSaved = saved;
          state.relationsDraft = buildMatrix(state.items, saved);
          state.relationsSaving = false;
          state.relationsDirty = false;
        });
      } catch (error) {
        set((state) => {
          state.relationsSaving = false;
          state.relationsError = userMessage(error);
        });
        throw error;
      }
    },

    resetRelations: () =>
      set((state) => {
        state.relationsDraft = state.relationsSaved.map((r) => ({ ...r }));
        state.relationsDirty = false;
        state.relationsError = null;
      }),

    applyEmotionDelta: (pair, affinityDelta, trustDelta) => {
      const state = get();
      if (
        !state.novelId ||
        !state.items.some((character) => character.id === pair[0]) ||
        !state.items.some((character) => character.id === pair[1])
      ) {
        return;
      }
      const key = relationKey(pair[0], pair[1]);
      const [a, b] = orderedPair(pair[0], pair[1]);
      const updateOne = (list: Relation[]) => {
        let rel = list.find((r) => relationKey(r.char_a_id, r.char_b_id) === key);
        if (!rel) {
          rel = { char_a_id: a, char_b_id: b, affinity: 0, trust: 0, conflict_desc: null };
          list.push(rel);
        }
        rel.affinity = Math.max(-100, Math.min(100, rel.affinity + affinityDelta));
        rel.trust = Math.max(0, Math.min(100, rel.trust + trustDelta));
      };
      set((state) => {
        updateOne(state.relationsSaved);
        updateOne(state.relationsDraft);
      });
    },

    syncMatrix: () =>
      set((state) => {
        state.relationsDraft = buildMatrix(state.items, state.relationsSaved);
        state.relationsDirty = false;
      }),

    clear: () =>
      set((state) => {
        state.novelId = null;
        state.items = [];
        state.total = 0;
        state.error = null;
        state.relationsSaved = [];
        state.relationsDraft = [];
        state.relationsLoaded = false;
        state.relationsDirty = false;
        state.relationsError = null;
      }),
  })),
);
