import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NovelDto } from "../types/domain";
import type { Wire, WirePage } from "../types/api";

const { listNovels, createNovel } = vi.hoisted(() => ({
  listNovels: vi.fn(),
  createNovel: vi.fn(),
}));

vi.mock("../api/endpoints", () => ({
  listNovels,
  createNovel,
}));

import { useNovelStore, type NovelQuery } from "./novelStore";

type Novel = Wire<NovelDto>;

function novel(overrides: Partial<Novel> = {}): Novel {
  return {
    id: "n1",
    title: "新小说",
    background: null,
    world_view: null,
    summary: null,
    style_name: null,
    style_sample: null,
    style_instruction: null,
    pov: null,
    status: "active",
    created_at: 1,
    updated_at: 1,
    ...overrides,
  };
}

function emptyPage(): WirePage<NovelDto> {
  return { items: [], total: 0, page: 1, page_size: 24 };
}

function resetStore(query: Partial<NovelQuery> = {}) {
  useNovelStore.setState({
    items: [],
    total: 0,
    page: 1,
    pageSize: 24,
    query: { q: "", status: "", page: 1, pageSize: 24, ...query },
    loading: false,
    error: null,
    currentId: null,
    current: null,
    currentLoading: false,
    currentError: null,
  });
}

const createReq: Wire<import("../types/domain").CreateNovelReq> = {
  title: "新小说",
  background: null,
  world_view: null,
  summary: null,
};

describe("useNovelStore.create", () => {
  beforeEach(() => {
    listNovels.mockReset();
    createNovel.mockReset();
    listNovels.mockResolvedValue(emptyPage());
    resetStore();
  });

  it("inserts a matching novel on the first page", async () => {
    const created = novel();
    createNovel.mockResolvedValue(created);

    await useNovelStore.getState().create(createReq);

    expect(useNovelStore.getState().items).toEqual([created]);
    expect(useNovelStore.getState().total).toBe(1);
    expect(listNovels).not.toHaveBeenCalled();
  });

  it("does not insert when the novel misses the status filter", async () => {
    resetStore({ status: "archived" });
    createNovel.mockResolvedValue(novel({ status: "active" }));

    await useNovelStore.getState().create(createReq);

    expect(useNovelStore.getState().items).toEqual([]);
    expect(useNovelStore.getState().total).toBe(0);
    expect(listNovels).toHaveBeenCalledWith({
      q: "",
      status: "archived",
      page: 1,
      pageSize: 24,
    });
  });

  it("does not insert when the novel misses the text query", async () => {
    resetStore({ q: "目标" });
    createNovel.mockResolvedValue(novel({ title: "无关标题" }));

    await useNovelStore.getState().create(createReq);

    expect(useNovelStore.getState().items).toEqual([]);
    expect(listNovels).toHaveBeenCalledWith({
      q: "目标",
      status: "",
      page: 1,
      pageSize: 24,
    });
  });

  it("does not insert while browsing a non-first page", async () => {
    resetStore();
    useNovelStore.setState({ page: 2, query: { q: "", status: "", page: 2, pageSize: 24 } });
    createNovel.mockResolvedValue(novel());

    await useNovelStore.getState().create(createReq);

    expect(useNovelStore.getState().items).toEqual([]);
    expect(useNovelStore.getState().total).toBe(0);
    expect(listNovels).toHaveBeenCalledWith({
      q: "",
      status: "",
      page: 2,
      pageSize: 24,
    });
  });
});
