import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import type { LoaderFunctionArgs } from "react-router-dom";
import { useNavigate, useParams } from "react-router-dom";
import {
  BookPlus,
  ChevronDown,
  ChevronUp,
  FilePlus2,
  ListTree,
  Pencil,
  Plus,
  Save,
  Sparkles,
  Trash2,
} from "lucide-react";
import type { ChapterStatus } from "../../types/domain";
import { chapterStatusLabel, zh } from "../../copy/zh";
import { useOutlineStore, type Chapter, type Volume } from "../../stores/outlineStore";
import { useUiStore } from "../../stores/uiStore";
import { useChapterContentStore } from "../../stores/chapterContentStore";
import { useChapterDraftStore } from "../../stores/chapterDraftStore";
import { userMessage } from "../../lib/errors";
import { formatDateTime, truncate } from "../../lib/format";
import {
  CHAPTER_TARGET_MAX,
  CHAPTER_TARGET_MIN,
  DEFAULT_CHAPTER_TARGET_WORDS,
  chapterTargetWords,
  parseTargetWords,
} from "../../lib/targetWords";
import { PageHeader } from "../../components/common/PageHeader";
import { Button } from "../../components/common/Button";
import { Dialog } from "../../components/common/Dialog";
import { Field } from "../../components/common/Field";
import { Input, Select, Textarea } from "../../components/common/Input";
import { EmptyState, ErrorState } from "../../components/common/EmptyState";
import { LoadingBlock, SkeletonLines } from "../../components/common/Skeleton";
import { ChapterStatusBadge } from "../../components/common/StatusBadge";
import { Tabs } from "../../components/common/Tabs";
import { MarkdownPreview } from "../../components/common/MarkdownPreview";
import { Badge } from "../../components/common/Badge";
import { cn } from "../../lib/cn";
import { ChapterContentPanel } from "../../components/novel/ChapterContentPanel";
import { ChapterDraftDialog } from "../../components/novel/ChapterDraftDialog";

export async function outlineLoader({ params }: LoaderFunctionArgs) {
  if (params.novelId) await useOutlineStore.getState().loadTree(params.novelId);
  return null;
}

const statuses: ChapterStatus[] = ["draft", "outlined", "completed", "archived"];

type VolumeDialogState = { mode: "create" } | { mode: "edit"; volume: Volume };

export function OutlinePage() {
  const { novelId } = useParams<{ novelId: string }>();
  const navigate = useNavigate();
  const toast = useUiStore((s) => s.toast);
  const askConfirm = useUiStore((s) => s.askConfirm);

  const volumes = useOutlineStore((s) => s.volumes);
  const chaptersByVolume = useOutlineStore((s) => s.chaptersByVolume);
  const loading = useOutlineStore((s) => s.loading);
  const error = useOutlineStore((s) => s.error);
  const selectedId = useOutlineStore((s) => s.selectedChapterId);
  const saving = useOutlineStore((s) => s.saving);
  const chapterError = useOutlineStore((s) => s.chapterError);
  const loadTree = useOutlineStore((s) => s.loadTree);
  const selectChapter = useOutlineStore((s) => s.selectChapter);
  const createVolume = useOutlineStore((s) => s.createVolume);
  const updateVolume = useOutlineStore((s) => s.updateVolume);
  const moveVolume = useOutlineStore((s) => s.moveVolume);
  const deleteVolume = useOutlineStore((s) => s.deleteVolume);
  const createChapter = useOutlineStore((s) => s.createChapter);
  const updateChapter = useOutlineStore((s) => s.updateChapter);
  const deleteChapter = useOutlineStore((s) => s.deleteChapter);

  const [volumeDialog, setVolumeDialog] = useState<VolumeDialogState | null>(null);
  const [chapterDialogVolume, setChapterDialogVolume] = useState<string | null>(null);
  const [view, setView] = useState<"write" | "preview">("write");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [volumeForm, setVolumeForm] = useState({ title: "", summary: "" });
  const [chapterForm, setChapterForm] = useState({
    title: "",
    scenePrompt: "",
    status: "draft" as ChapterStatus,
    targetWords: String(DEFAULT_CHAPTER_TARGET_WORDS),
  });

  const selected = useMemo(() => {
    if (!selectedId) return null;
    for (const list of Object.values(chaptersByVolume)) {
      const found = list.find((chapter) => chapter.id === selectedId);
      if (found) return found;
    }
    return null;
  }, [selectedId, chaptersByVolume]);

  const [draft, setDraft] = useState({
    title: "",
    scenePrompt: "",
    finalOutline: "",
    status: "draft" as ChapterStatus,
    targetWords: String(DEFAULT_CHAPTER_TARGET_WORDS),
  });
  const [dirty, setDirty] = useState(false);
  const [targetWordsError, setTargetWordsError] = useState<string | null>(null);

  const [draftDialogOpen, setDraftDialogOpen] = useState(false);
  const contentDirty = useChapterContentStore((s) => s.dirty);
  const loadContent = useChapterContentStore((s) => s.load);
  const resetContent = useChapterContentStore((s) => s.reset);
  const resetDraft = useChapterDraftStore((s) => s.reset);

  // 章节变化时加载该章正文；组件卸载时清空正文状态、断开任务流。
  useEffect(() => {
    if (selected && novelId) void loadContent(novelId, selected.id);
  }, [selected?.id, novelId, loadContent]);

  useEffect(
    () => () => {
      resetContent();
      resetDraft();
    },
    [resetContent, resetDraft],
  );

  const handleSelectChapter = async (chapterId: string) => {
    if (chapterId === selectedId) return;
    if (contentDirty) {
      const ok = await askConfirm({
        title: zh.outline.leaveUnsavedTitle,
        message: zh.outline.leaveUnsavedContent,
        confirmLabel: zh.outline.leaveUnsavedConfirm,
      });
      if (!ok) return;
    }
    selectChapter(chapterId);
  };

  const openDraftDialog = () => {
    if (!selected) return;
    setDraftDialogOpen(true);
  };

  useEffect(() => {
    if (!selected) return;
    setDraft({
      title: selected.title,
      scenePrompt: selected.scene_prompt ?? "",
      finalOutline: selected.final_outline ?? "",
      status: selected.status,
      targetWords: String(chapterTargetWords(selected.target_words)),
    });
    setDirty(false);
    setTargetWordsError(null);
    setView("write");
  }, [selected?.id, selected?.updated_at]);

  if (!novelId) return null;

  const reload = () => void loadTree(novelId);

  const openCreateVolume = () => {
    setVolumeForm({ title: "", summary: "" });
    setFormError(null);
    setFieldErrors({});
    setVolumeDialog({ mode: "create" });
  };

  const openEditVolume = (volume: Volume) => {
    setVolumeForm({ title: volume.title, summary: volume.summary ?? "" });
    setFormError(null);
    setFieldErrors({});
    setVolumeDialog({ mode: "edit", volume });
  };

  const submitVolume = async (event: FormEvent) => {
    event.preventDefault();
    if (!volumeDialog) return;
    setBusy(true);
    setFormError(null);
    setFieldErrors({});
    try {
      if (volumeDialog.mode === "create") {
        await createVolume(novelId, {
          title: volumeForm.title.trim() || null,
          summary: volumeForm.summary.trim() || null,
        });
        toast({ tone: "success", message: "卷已创建" });
      } else {
        await updateVolume(novelId, volumeDialog.volume.id, {
          title: volumeForm.title.trim() || null,
          summary: volumeForm.summary.trim() || null,
        });
        toast({ tone: "success", message: "卷已更新" });
      }
      setVolumeDialog(null);
    } catch (err) {
      setFormError(userMessage(err));
      setFieldErrors((err as { fieldErrors?: Record<string, string> }).fieldErrors ?? {});
    } finally {
      setBusy(false);
    }
  };

  const openCreateChapter = (volumeId: string) => {
    setChapterForm({
      title: "",
      scenePrompt: "",
      status: "draft",
      targetWords: String(DEFAULT_CHAPTER_TARGET_WORDS),
    });
    setFormError(null);
    setFieldErrors({});
    setChapterDialogVolume(volumeId);
  };

  const submitChapter = async (event: FormEvent) => {
    event.preventDefault();
    if (!chapterDialogVolume) return;
    const targetWords = parseTargetWords(
      chapterForm.targetWords,
      CHAPTER_TARGET_MIN,
      CHAPTER_TARGET_MAX,
    );
    if (targetWords === null) {
      setFieldErrors({
        target_words: zh.outline.targetWordsInvalid(CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX),
      });
      return;
    }
    setBusy(true);
    setFormError(null);
    setFieldErrors({});
    try {
      await createChapter(novelId, chapterDialogVolume, {
        title: chapterForm.title.trim() || null,
        scene_prompt: chapterForm.scenePrompt.trim() || null,
        target_words: targetWords,
        status: chapterForm.status,
      });
      setChapterDialogVolume(null);
      toast({ tone: "success", message: "章节已创建" });
    } catch (err) {
      setFormError(userMessage(err));
      setFieldErrors((err as { fieldErrors?: Record<string, string> }).fieldErrors ?? {});
    } finally {
      setBusy(false);
    }
  };

  const submitChapterEdit = async () => {
    if (!selected) return;
    const targetWords = parseTargetWords(
      draft.targetWords,
      CHAPTER_TARGET_MIN,
      CHAPTER_TARGET_MAX,
    );
    if (targetWords === null) {
      setTargetWordsError(zh.outline.targetWordsInvalid(CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX));
      return;
    }
    try {
      await updateChapter(novelId, selected.id, {
        title: draft.title.trim() || null,
        scene_prompt: draft.scenePrompt.trim() || null,
        final_outline: draft.finalOutline.trim() || null,
        target_words: targetWords,
        status: draft.status,
      });
      setDirty(false);
      setTargetWordsError(null);
      toast({ tone: "success", message: zh.outline.saved });
    } catch (err) {
      toast({ tone: "error", message: userMessage(err) });
    }
  };

  // 监听器只安装一次；通过 ref 读取最新草稿/选中状态，避免闭包读到旧值。
  const saveShortcutRef = useRef<() => void>(() => {});
  saveShortcutRef.current = () => {
    if (selected && dirty && !saving) void submitChapterEdit();
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        saveShortcutRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const removeVolume = async (volume: Volume) => {
    const ok = await askConfirm({
      title: "删除卷？",
      message: zh.outline.deleteVolumeConfirm,
      confirmLabel: zh.common.delete,
      tone: "danger",
    });
    if (!ok) return;
    try {
      await deleteVolume(novelId, volume.id);
      toast({ tone: "success", message: "卷已移入回收站" });
    } catch (err) {
      toast({ tone: "error", message: userMessage(err) });
    }
  };

  const removeChapter = async (chapter: Chapter) => {
    const ok = await askConfirm({
      title: "删除章节？",
      message: zh.outline.deleteChapterConfirm,
      confirmLabel: zh.common.delete,
      tone: "danger",
    });
    if (!ok) return;
    try {
      await deleteChapter(novelId, chapter.id);
      toast({ tone: "success", message: "章节已移入回收站" });
    } catch (err) {
      toast({ tone: "error", message: userMessage(err) });
    }
  };

  const move = async (volume: Volume, direction: "up" | "down") => {
    try {
      await moveVolume(novelId, volume.id, direction);
    } catch (err) {
      toast({ tone: "error", message: userMessage(err) });
    }
  };

  return (
    <div className="space-y-5">
      <PageHeader
        title={zh.outline.title}
        description={zh.outline.subtitle}
        actions={
          <Button variant="primary" leftIcon={<BookPlus className="h-4 w-4" />} onClick={openCreateVolume}>
            {zh.outline.newVolume}
          </Button>
        }
      />

      {loading && volumes.length === 0 ? (
        <LoadingBlock label="正在读取卷章大纲…" />
      ) : error && volumes.length === 0 ? (
        <ErrorState message={error} onRetry={reload} />
      ) : volumes.length === 0 ? (
        <EmptyState
          icon={<ListTree className="h-5 w-5" />}
          title={zh.outline.empty}
          description={zh.outline.emptyHint}
          action={
            <Button variant="primary" leftIcon={<Plus className="h-4 w-4" />} onClick={openCreateVolume}>
              {zh.outline.newVolume}
            </Button>
          }
        />
      ) : (
        <div className="grid gap-4 xl:grid-cols-[minmax(300px,380px)_minmax(0,1fr)]">
          <aside className="space-y-3 xl:sticky xl:top-[7.2rem] xl:max-h-[calc(100dvh-8.5rem)] xl:overflow-y-auto xl:pr-1">
            {volumes.map((volume, volumeIndex) => {
              const chapters = chaptersByVolume[volume.id] ?? [];
              return (
                <section key={volume.id} className="surface-card overflow-hidden">
                  <div className="border-b border-line/60 bg-paper/40 px-3 py-2.5">
                    <div className="flex items-start gap-2">
                      <span className="mt-0.5 font-mono text-[11px] text-faint">
                        {String(volume.volume_index).padStart(2, "0")}
                      </span>
                      <div className="min-w-0 flex-1">
                        <h3 className="truncate font-display text-sm font-semibold text-ink">
                          {volume.title || "未命名卷"}
                        </h3>
                        {volume.summary ? (
                          <p className="mt-0.5 line-clamp-1 text-[11px] text-faint">{volume.summary}</p>
                        ) : null}
                      </div>
                      <div className="flex shrink-0 items-center gap-0.5">
                        <button
                          type="button"
                          className="focus-ring rounded p-1 text-faint hover:bg-line/40 hover:text-ink disabled:opacity-30"
                          onClick={() => void move(volume, "up")}
                          disabled={volumeIndex === 0}
                          aria-label="卷上移"
                          title={zh.outline.moveUp}
                        >
                          <ChevronUp className="h-3.5 w-3.5" />
                        </button>
                        <button
                          type="button"
                          className="focus-ring rounded p-1 text-faint hover:bg-line/40 hover:text-ink disabled:opacity-30"
                          onClick={() => void move(volume, "down")}
                          disabled={volumeIndex === volumes.length - 1}
                          aria-label="卷下移"
                          title={zh.outline.moveDown}
                        >
                          <ChevronDown className="h-3.5 w-3.5" />
                        </button>
                        <button
                          type="button"
                          className="focus-ring rounded p-1 text-faint hover:bg-line/40 hover:text-ink"
                          onClick={() => openEditVolume(volume)}
                          aria-label="编辑卷"
                        >
                          <Pencil className="h-3.5 w-3.5" />
                        </button>
                        <button
                          type="button"
                          className="focus-ring rounded p-1 text-faint hover:bg-danger-soft hover:text-danger"
                          onClick={() => void removeVolume(volume)}
                          aria-label="删除卷"
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </button>
                      </div>
                    </div>
                  </div>

                  <div className="p-1.5">
                    {chapters.length === 0 ? (
                      <p className="px-2 py-3 text-center text-[11px] text-faint">{zh.outline.emptyChapters}</p>
                    ) : (
                      <ul className="space-y-0.5">
                        {chapters.map((chapter) => (
                          <li key={chapter.id}>
                            <div
                              className={cn(
                                "group flex items-center gap-1 rounded-lg transition-colors",
                                selectedId === chapter.id
                                  ? "bg-accent-soft/70"
                                  : "hover:bg-line/30",
                              )}
                            >
                              <button
                                type="button"
                                onClick={() => void handleSelectChapter(chapter.id)}
                                className="focus-ring flex min-w-0 flex-1 items-center gap-2 rounded-lg px-2 py-2 text-left"
                              >
                                <span className="w-6 shrink-0 text-right font-mono text-[10px] text-faint">
                                  {chapter.chapter_index}
                                </span>
                                <span className="min-w-0 flex-1">
                                  <span className="flex items-baseline gap-1.5">
                                    <span
                                      className={cn(
                                        "min-w-0 flex-1 truncate text-xs",
                                        selectedId === chapter.id ? "font-medium text-accent-strong dark:text-accent" : "text-ink",
                                      )}
                                    >
                                      {chapter.title || "未命名章节"}
                                    </span>
                                    <span className="shrink-0 font-mono text-[10px] text-faint">
                                      {zh.outline.targetWordsValue(chapter.target_words)}
                                    </span>
                                  </span>
                                  <span className="mt-0.5 flex items-center gap-1.5">
                                    <ChapterStatusBadge status={chapter.status} />
                                  </span>
                                </span>
                              </button>
                              <button
                                type="button"
                                onClick={() => void removeChapter(chapter)}
                                className="focus-ring mr-1 rounded p-1 text-faint opacity-100 transition-opacity hover:text-danger sm:opacity-0 sm:group-hover:opacity-100 sm:group-focus-within:opacity-100"
                                aria-label={`删除章节 ${chapter.title}`}
                              >
                                <Trash2 className="h-3 w-3" />
                              </button>
                            </div>
                          </li>
                        ))}
                      </ul>
                    )}
                    <button
                      type="button"
                      onClick={() => openCreateChapter(volume.id)}
                      className="focus-ring mt-1 flex w-full items-center justify-center gap-1.5 rounded-lg border border-dashed border-line/80 px-2 py-2 text-[11px] text-faint transition-colors hover:border-accent/40 hover:text-accent-strong"
                    >
                      <FilePlus2 className="h-3.5 w-3.5" /> {zh.outline.newChapter}
                    </button>
                  </div>
                </section>
              );
            })}
          </aside>

          <section className="min-w-0">
            {!selected ? (
              <EmptyState
                icon={<ListTree className="h-5 w-5" />}
                title={zh.outline.selectChapter}
                description={zh.outline.selectChapterHint}
                className="xl:min-h-[420px]"
              />
            ) : (
              <div className="surface-card overflow-hidden">
                <div className="flex flex-wrap items-center justify-between gap-2 border-b border-line/70 px-4 py-3">
                  <div className="flex min-w-0 items-center gap-2">
                    <ChapterStatusBadge status={draft.status} />
                    <span className="text-[11px] text-faint">更新于 {formatDateTime(selected.updated_at)}</span>
                    <span className="text-[11px] text-faint">
                      {zh.outline.targetWordsDetail(chapterTargetWords(selected.target_words))}
                    </span>
                    {dirty ? <Badge tone="warning" dot>{zh.outline.unsaved}</Badge> : null}
                  </div>
                  <div className="flex flex-wrap items-center gap-2">
                    <Tabs
                      value={view}
                      onChange={setView}
                      items={[
                        { value: "write", label: zh.outline.write },
                        { value: "preview", label: zh.outline.preview },
                      ]}
                    />
                    <Button
                      variant="primary"
                      size="sm"
                      leftIcon={<Save className="h-3.5 w-3.5" />}
                      loading={saving}
                      disabled={!dirty}
                      onClick={() => void submitChapterEdit()}
                    >
                      {zh.common.save}
                    </Button>
                  </div>
                </div>

                <div className="space-y-4 p-4">
                  <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_140px_160px]">
                    <Field label={zh.outline.chapterTitle} htmlFor="chapter-title">
                      <Input
                        id="chapter-title"
                        value={draft.title}
                        onChange={(e) => {
                          setDraft({ ...draft, title: e.target.value });
                          setDirty(true);
                        }}
                        maxLength={200}
                        placeholder="给这一章一个能被记住的名字"
                      />
                    </Field>
                    <Field
                      label={zh.outline.targetWords}
                      htmlFor="chapter-target-words"
                      hint={zh.outline.targetWordsHint}
                      error={targetWordsError}
                    >
                      <Input
                        id="chapter-target-words"
                        type="number"
                        min={CHAPTER_TARGET_MIN}
                        max={CHAPTER_TARGET_MAX}
                        step={100}
                        value={draft.targetWords}
                        invalid={!!targetWordsError}
                        onChange={(e) => {
                          setDraft({ ...draft, targetWords: e.target.value });
                          setTargetWordsError(null);
                          setDirty(true);
                        }}
                        inputMode="numeric"
                      />
                    </Field>
                    <Field label={zh.outline.status} htmlFor="chapter-status">
                      <Select
                        id="chapter-status"
                        value={draft.status}
                        onChange={(e) => {
                          setDraft({ ...draft, status: e.target.value as ChapterStatus });
                          setDirty(true);
                        }}
                      >
                        {statuses.map((status) => (
                          <option key={status} value={status}>
                            {chapterStatusLabel[status]}
                          </option>
                        ))}
                      </Select>
                    </Field>
                  </div>

                  <Field label={zh.outline.scenePrompt} htmlFor="chapter-scene" hint="沙盒会用这段提示开始推演。">
                    <Textarea
                      id="chapter-scene"
                      rows={3}
                      value={draft.scenePrompt}
                      onChange={(e) => {
                        setDraft({ ...draft, scenePrompt: e.target.value });
                        setDirty(true);
                      }}
                      placeholder="时间、地点、在场人物、正在发生的冲突…"
                    />
                  </Field>

                  <Field
                    label={zh.outline.finalOutline}
                    htmlFor="chapter-final"
                    hint="支持 Markdown。保存后章节会保留当前状态。"
                  >
                    {view === "write" ? (
                      <Textarea
                        id="chapter-final"
                        rows={18}
                        value={draft.finalOutline}
                        onChange={(e) => {
                          setDraft({ ...draft, finalOutline: e.target.value });
                          setDirty(true);
                        }}
                        placeholder="## 章节标题&#10;&#10;- 剧情梗概…&#10;- 关键场景…"
                        className="min-h-[360px] font-mono text-xs leading-6"
                      />
                    ) : (
                      <div className="min-h-[360px] rounded-lg border border-line/70 bg-raised p-4">
                        <MarkdownPreview source={draft.finalOutline} empty="还没有定稿大纲，切回编辑开始写。" />
                      </div>
                    )}
                  </Field>

                  {chapterError ? (
                    <p className="rounded-lg border border-danger/25 bg-danger-soft/40 px-3 py-2 text-sm text-danger" role="alert">
                      {chapterError}
                    </p>
                  ) : null}

                  <div className="!mt-5 border-t border-line/60 pt-5">
                    <ChapterContentPanel
                      novelId={novelId}
                      chapterId={selected.id}
                      disabled={busy}
                      onGenerate={openDraftDialog}
                    />
                  </div>

                  <div className="flex flex-wrap items-center justify-between gap-2 border-t border-line/60 pt-3">
                    <button
                      type="button"
                      onClick={() => navigate(`/novel/${novelId}/sandbox?chapter=${selected.id}`)}
                      className="focus-ring inline-flex items-center gap-1.5 rounded text-xs font-medium text-accent-strong hover:underline"
                    >
                      <Sparkles className="h-3.5 w-3.5" /> 用这一章发起推演
                    </button>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="text-faint hover:text-danger"
                      leftIcon={<Trash2 className="h-3.5 w-3.5" />}
                      onClick={() => void removeChapter(selected)}
                    >
                      删除章节
                    </Button>
                  </div>
                </div>
              </div>
            )}
          </section>
        </div>
      )}

      <Dialog
        open={!!volumeDialog}
        onClose={() => (busy ? undefined : setVolumeDialog(null))}
        title={volumeDialog?.mode === "edit" ? "编辑卷" : zh.outline.newVolume}
        size="md"
        footer={
          <>
            <Button variant="ghost" onClick={() => setVolumeDialog(null)} disabled={busy}>
              {zh.common.cancel}
            </Button>
            <Button variant="primary" onClick={submitVolume} loading={busy}>
              {zh.common.save}
            </Button>
          </>
        }
      >
        <form onSubmit={submitVolume} className="space-y-4">
          <Field label={zh.outline.volumeTitle} htmlFor="volume-title" error={fieldErrors.title}>
            <Input
              id="volume-title"
              value={volumeForm.title}
              onChange={(e) => setVolumeForm({ ...volumeForm, title: e.target.value })}
              invalid={!!fieldErrors.title}
              autoFocus
              maxLength={200}
            />
          </Field>
          <Field label={zh.outline.volumeSummary} htmlFor="volume-summary" error={fieldErrors.summary}>
            <Textarea
              id="volume-summary"
              rows={5}
              value={volumeForm.summary}
              onChange={(e) => setVolumeForm({ ...volumeForm, summary: e.target.value })}
              invalid={!!fieldErrors.summary}
            />
          </Field>
          {formError ? (
            <p className="rounded-lg border border-danger/25 bg-danger-soft/50 px-3 py-2 text-sm text-danger" role="alert">
              {formError}
            </p>
          ) : null}
        </form>
      </Dialog>

      <Dialog
        open={!!chapterDialogVolume}
        onClose={() => (busy ? undefined : setChapterDialogVolume(null))}
        title={zh.outline.newChapter}
        description="先给章节一个位置，正文大纲可以之后补。"
        size="lg"
        footer={
          <>
            <Button variant="ghost" onClick={() => setChapterDialogVolume(null)} disabled={busy}>
              {zh.common.cancel}
            </Button>
            <Button variant="primary" onClick={submitChapter} loading={busy}>
              {zh.common.create}
            </Button>
          </>
        }
      >
        <form onSubmit={submitChapter} className="space-y-4">
          <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_140px_160px]">
            <Field label={zh.outline.chapterTitle} htmlFor="new-chapter-title" error={fieldErrors.title}>
              <Input
                id="new-chapter-title"
                value={chapterForm.title}
                onChange={(e) => setChapterForm({ ...chapterForm, title: e.target.value })}
                invalid={!!fieldErrors.title}
                autoFocus
                maxLength={200}
              />
            </Field>
            <Field
              label={zh.outline.targetWords}
              htmlFor="new-chapter-target-words"
              hint={zh.outline.targetWordsHint}
              error={fieldErrors.target_words}
            >
              <Input
                id="new-chapter-target-words"
                type="number"
                min={CHAPTER_TARGET_MIN}
                max={CHAPTER_TARGET_MAX}
                step={100}
                value={chapterForm.targetWords}
                invalid={!!fieldErrors.target_words}
                onChange={(e) => setChapterForm({ ...chapterForm, targetWords: e.target.value })}
                inputMode="numeric"
              />
            </Field>
            <Field label={zh.outline.status} htmlFor="new-chapter-status">
              <Select
                id="new-chapter-status"
                value={chapterForm.status}
                onChange={(e) => setChapterForm({ ...chapterForm, status: e.target.value as ChapterStatus })}
              >
                {statuses.map((status) => (
                  <option key={status} value={status}>
                    {chapterStatusLabel[status]}
                  </option>
                ))}
              </Select>
            </Field>
          </div>
          <Field label={zh.outline.scenePrompt} htmlFor="new-chapter-scene" error={fieldErrors.scene_prompt}>
            <Textarea
              id="new-chapter-scene"
              rows={5}
              value={chapterForm.scenePrompt}
              onChange={(e) => setChapterForm({ ...chapterForm, scenePrompt: e.target.value })}
              invalid={!!fieldErrors.scene_prompt}
              placeholder="这一章开始时，读者应该看到什么？"
            />
          </Field>
          {formError ? (
            <p className="rounded-lg border border-danger/25 bg-danger-soft/50 px-3 py-2 text-sm text-danger" role="alert">
              {formError}
            </p>
          ) : null}
        </form>
      </Dialog>

      {selected && novelId ? (
        <ChapterDraftDialog
          open={draftDialogOpen}
          onClose={() => setDraftDialogOpen(false)}
          novelId={novelId}
          chapterId={selected.id}
          chapterTargetWords={selected.target_words}
        />
      ) : null}
    </div>
  );
}
