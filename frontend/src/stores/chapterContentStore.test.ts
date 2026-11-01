import { beforeEach, describe, expect, it, vi } from "vitest";

const { getChapterContent, saveChapterContent } = vi.hoisted(() => ({
  getChapterContent: vi.fn(),
  saveChapterContent: vi.fn(),
}));

vi.mock("../api/endpoints", () => ({
  getChapterContent,
  saveChapterContent,
}));

import { useChapterContentStore } from "./chapterContentStore";

function resetStore() {
  useChapterContentStore.getState().reset();
  useChapterContentStore.setState({
    novelId: "n1",
    chapterId: "c1",
  });
}

describe("useChapterContentStore.load", () => {
  beforeEach(() => {
    getChapterContent.mockReset();
    saveChapterContent.mockReset();
    resetStore();
  });

  it("loads content, editable and word count", async () => {
    getChapterContent.mockResolvedValue({ content: "第一段正文", word_count: 5 });

    await useChapterContentStore.getState().load("n1", "c1");

    const state = useChapterContentStore.getState();
    expect(state.content).toBe("第一段正文");
    expect(state.editable).toBe("第一段正文");
    expect(state.wordCount).toBe(5);
    expect(state.loaded).toBe(true);
    expect(state.dirty).toBe(false);
  });

  it("exposes fetch errors to the user", async () => {
    getChapterContent.mockRejectedValue(new Error("网络错误"));

    await useChapterContentStore.getState().load("n1", "c1");

    const state = useChapterContentStore.getState();
    expect(state.loaded).toBe(true);
    expect(state.error).toBe("网络错误");
  });
});

describe("useChapterContentStore editing + save", () => {
  beforeEach(() => {
    getChapterContent.mockReset();
    saveChapterContent.mockReset();
    resetStore();
    useChapterContentStore.setState({ content: "原文", editable: "原文", loaded: true });
  });

  it("tracks dirty state and discards changes", () => {
    const store = useChapterContentStore.getState();
    store.setEditable("改过的");
    expect(useChapterContentStore.getState().dirty).toBe(true);

    useChapterContentStore.getState().discard();
    expect(useChapterContentStore.getState().editable).toBe("原文");
    expect(useChapterContentStore.getState().dirty).toBe(false);
  });

  it("persists on save and clears dirty", async () => {
    saveChapterContent.mockResolvedValue(undefined);
    useChapterContentStore.setState({ editable: "已保存", dirty: true });

    await useChapterContentStore.getState().save();

    expect(saveChapterContent).toHaveBeenCalledWith("n1", "c1", { content: "已保存" });
    const state = useChapterContentStore.getState();
    expect(state.content).toBe("已保存");
    expect(state.dirty).toBe(false);
    expect(state.wordCount).toBe(3);
  });

  it("rejects empty content before calling the API", async () => {
    useChapterContentStore.setState({ editable: "   ", dirty: true });

    await expect(useChapterContentStore.getState().save()).rejects.toThrow("正文不能为空");

    expect(saveChapterContent).not.toHaveBeenCalled();
    expect(useChapterContentStore.getState().error).toBe("正文不能为空。");
  });

  it("keeps edits typed while a save request is in flight", async () => {
    let resolveSave!: () => void;
    const pending = new Promise<void>((resolve) => {
      resolveSave = resolve;
    });
    saveChapterContent.mockReturnValue(pending);
    useChapterContentStore.setState({ editable: "第一次", dirty: true });

    const saving = useChapterContentStore.getState().save();
    useChapterContentStore.getState().setEditable("第一次，又补了一句");
    resolveSave();
    await saving;

    const state = useChapterContentStore.getState();
    expect(state.content).toBe("第一次");
    expect(state.editable).toBe("第一次，又补了一句");
    expect(state.dirty).toBe(true);
  });
});

describe("useChapterContentStore.applyContent", () => {
  beforeEach(() => {
    resetStore();
  });

  it("replaces the editing buffer with generated content", () => {
    useChapterContentStore.setState({ editable: "旧草稿", dirty: true });

    useChapterContentStore.getState().applyContent({
      content: "生成好的正文",
      word_count: 6,
      lint: { before_total: 6, after_total: 1, rewritten: 2, issues: [] },
    });

    const state = useChapterContentStore.getState();
    expect(state.content).toBe("生成好的正文");
    expect(state.editable).toBe("生成好的正文");
    expect(state.wordCount).toBe(6);
    expect(state.lint?.before_total).toBe(6);
    expect(state.dirty).toBe(false);
  });

  it("keeps lint from the server snapshot after reload", async () => {
    await useChapterContentStore.getState().load("novel-1", "chapter-1");
    const state = useChapterContentStore.getState();
    expect(state.lint).toBeNull();
  });
});
