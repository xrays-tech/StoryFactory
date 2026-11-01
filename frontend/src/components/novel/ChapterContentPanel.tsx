import { useEffect, useMemo, useState } from "react";
import { Eye, FileText, Loader2, Pencil, RefreshCw, Save, Sparkles } from "lucide-react";
import { zh } from "../../copy/zh";
import {
  draftLintChips,
  draftLintSummary,
  draftStageLabel,
  draftStatusLabel,
  isDraftRunning,
  useChapterDraftStore,
} from "../../stores/chapterDraftStore";
import { useChapterContentStore } from "../../stores/chapterContentStore";
import { useUiStore } from "../../stores/uiStore";
import { userMessage } from "../../lib/errors";
import { countWords, formatNumber } from "../../lib/format";
import { Button } from "../common/Button";
import { Badge } from "../common/Badge";
import { Textarea } from "../common/Input";
import { ProgressBar } from "../common/Progress";
import { EmptyState, ErrorState } from "../common/EmptyState";
import { LoadingBlock } from "../common/Skeleton";

export type ChapterContentPanelProps = {
  novelId: string;
  chapterId: string;
  disabled?: boolean;
  onGenerate: () => void;
};

export function ChapterContentPanel({ novelId, chapterId, disabled, onGenerate }: ChapterContentPanelProps) {
  const toast = useUiStore((s) => s.toast);
  const askConfirm = useUiStore((s) => s.askConfirm);

  const content = useChapterContentStore((s) => s.content);
  const editable = useChapterContentStore((s) => s.editable);
  const loaded = useChapterContentStore((s) => s.loaded);
  const loading = useChapterContentStore((s) => s.loading);
  const contentError = useChapterContentStore((s) => s.error);
  const saving = useChapterContentStore((s) => s.saving);
  const dirty = useChapterContentStore((s) => s.dirty);
  const setEditable = useChapterContentStore((s) => s.setEditable);
  const save = useChapterContentStore((s) => s.save);
  const load = useChapterContentStore((s) => s.load);
  const discard = useChapterContentStore((s) => s.discard);

  const job = useChapterDraftStore((s) => s.job);
  const draftLint = useChapterDraftStore((s) => s.lint);
  const draftChapterId = useChapterDraftStore((s) => s.chapterId);
  const creating = useChapterDraftStore((s) => s.creating);
  const cancelling = useChapterDraftStore((s) => s.cancelling);
  const draftError = useChapterDraftStore((s) => s.error);
  const draftStage = useChapterDraftStore((s) => s.stage);
  const draftProgress = useChapterDraftStore((s) => s.progress);
  const draftMessage = useChapterDraftStore((s) => s.message);
  const connection = useChapterDraftStore((s) => s.connection);
  const streamError = useChapterDraftStore((s) => s.streamError);
  const startedAt = useChapterDraftStore((s) => s.startedAt);
  const refreshing = useChapterDraftStore((s) => s.refreshing);
  const refreshError = useChapterDraftStore((s) => s.refreshError);
  const prefs = useChapterDraftStore((s) => s.prefs);
  const retry = useChapterDraftStore((s) => s.retry);
  const cancelDraft = useChapterDraftStore((s) => s.cancel);
  const clearRefreshError = useChapterDraftStore((s) => s.clearRefreshError);

  const contextMatches = draftChapterId === chapterId;
  const currentJob = contextMatches ? job : null;
  const running = contextMatches && (creating || isDraftRunning(currentJob));
  const anyRunning = creating || isDraftRunning(job);
  const blockedByOther = anyRunning && !contextMatches;
  const isRefreshing = contextMatches && refreshing;
  const currentError = contextMatches ? draftError : null;
  const currentRefreshError = contextMatches ? refreshError : null;
  const currentMessage = contextMatches ? draftMessage : null;
  const currentStage = contextMatches ? draftStage : null;
  const currentProgress = contextMatches ? draftProgress : 0;
  const currentStartedAt = contextMatches ? startedAt : null;
  const currentLint = contextMatches && !running ? draftLint : null;
  const lintChips = useMemo(
    () => (currentLint ? draftLintChips(currentLint) : { chips: [], overflow: 0 }),
    [currentLint],
  );
  const streamUnstable =
    running && (connection === "reconnecting" || connection === "error" || !!streamError);
  const stageText =
    creating
      ? zh.draft.creating
      : currentStage
        ? draftStageLabel(currentStage)
        : currentJob
          ? draftStatusLabel(currentJob.status)
          : zh.draft.stageQueued;

  const [now, setNow] = useState(() => Date.now());
  const [editing, setEditing] = useState(true);

  useEffect(() => {
    if (!running || !currentStartedAt) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [running, currentStartedAt]);

  // 章节切换时回到编辑态；生成期间强制只读，避免进度与编辑互相打断。
  useEffect(() => {
    setEditing(true);
  }, [chapterId]);

  useEffect(() => {
    if (running) setEditing(false);
  }, [running]);

  const elapsed = currentStartedAt ? Math.max(0, Math.floor((now - currentStartedAt) / 1000)) : 0;
  const liveWordCount = useMemo(() => countWords(editable), [editable]);

  const handleSave = async () => {
    try {
      await save();
    } catch (error) {
      toast({ tone: "error", message: userMessage(error) });
    }
  };

  const handleDiscard = async () => {
    const ok = await askConfirm({
      title: zh.outline.discardContentTitle,
      message: zh.outline.discardContentHint,
      confirmLabel: zh.outline.discardContentConfirm,
    });
    if (!ok) return;
    discard();
  };

  const handleRegenerate = async () => {
    if (dirty) {
      const ok = await askConfirm({
        title: zh.outline.regenerateTitle,
        message: zh.outline.regenerateHint,
        confirmLabel: zh.outline.regenerateConfirm,
      });
      if (!ok) return;
    }
    onGenerate();
  };

  const handleCancelDraft = async () => {
    const ok = await askConfirm({
      title: zh.draft.cancelTitle,
      message: zh.draft.cancelHint,
      confirmLabel: zh.draft.cancel,
    });
    if (!ok) return;
    void cancelDraft();
  };

  const handleRetry = async () => {
    try {
      await retry();
    } catch (error) {
      // create/retry 已写入 store.error，这里只补一条即时反馈。
      toast({ tone: "error", message: userMessage(error) });
    }
  };

  const handleReloadContent = () => {
    clearRefreshError();
    void load(novelId, chapterId);
  };

  const showContentError = !!contentError && !content;
  const showContent = loaded && !!content;
  const showEmpty =
    loaded && !loading && !showContent && !running && !currentError && !currentRefreshError && !isRefreshing;
  const showDraftFailure = !!currentError && !running;
  const showRefreshFailure = !!currentRefreshError && !showContent && !running;
  const canRetry = !!prefs;

  return (
    <section className="rounded-2xl border border-line/70 bg-surface">
      <header className="flex flex-wrap items-center justify-between gap-2 border-b border-line/70 px-4 py-3">
        <div className="flex min-w-0 items-center gap-2">
          <FileText className="h-4 w-4 shrink-0 text-accent" aria-hidden />
          <h3 className="font-display text-sm font-semibold text-ink">{zh.outline.contentTitle}</h3>
          {showContent ? (
            <span className="text-[11px] text-faint">{formatNumber(liveWordCount)} 字</span>
          ) : null}
          {dirty ? (
            <Badge tone="warning" dot>
              {zh.outline.contentUnsaved}
            </Badge>
          ) : null}
          {running ? (
            <Badge tone="accent" dot pulse>
              {zh.draft.generating}
            </Badge>
          ) : null}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {showContent && !running ? (
            <>
              <Button
                variant="ghost"
                size="sm"
                leftIcon={editing ? <Eye className="h-3.5 w-3.5" /> : <Pencil className="h-3.5 w-3.5" />}
                onClick={() => setEditing((value) => !value)}
              >
                {editing ? zh.outline.contentReading : zh.outline.contentEditing}
              </Button>
              <Button
                variant="outline"
                size="sm"
                leftIcon={<RefreshCw className="h-3.5 w-3.5" />}
                onClick={() => void handleRegenerate()}
                disabled={disabled || blockedByOther}
                title={blockedByOther ? zh.draft.otherChapterRunning : undefined}
              >
                {zh.outline.regenerate}
              </Button>
            </>
          ) : null}
          {running && currentJob ? (
            <Button
              variant="ghost"
              size="sm"
              className="text-faint hover:text-danger"
              loading={cancelling}
              onClick={() => void handleCancelDraft()}
            >
              {zh.draft.cancel}
            </Button>
          ) : null}
        </div>
      </header>

      <div className="space-y-3 p-4">
        {running ? (
          <div className="space-y-3">
            <LiveProgress
              stage={stageText}
              progress={currentProgress}
              elapsed={elapsed}
              creating={creating}
            />
            {streamUnstable ? (
              <p className="text-[11px] leading-5 text-warning" role="status">
                {zh.draft.streamReconnecting}
              </p>
            ) : null}
            {currentMessage ? <WarningBanner message={currentMessage} /> : null}
          </div>
        ) : null}

        {isRefreshing ? (
          <p className="flex items-center gap-2 text-[11px] leading-5 text-faint" role="status">
            <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden />
            {zh.draft.refreshing}
          </p>
        ) : null}

        {loading && !loaded ? <LoadingBlock label="正在读取正文…" /> : null}

        {showContentError ? (
          <ErrorState
            title={zh.outline.contentLoadFailed}
            message={contentError}
            onRetry={() => void load(novelId, chapterId)}
            compact
          />
        ) : null}

        {showEmpty ? (
          <div className="space-y-2">
            {blockedByOther ? (
              <p
                className="rounded-lg border border-warning/30 bg-warning-soft/50 px-3 py-2 text-xs leading-5 text-warning"
                role="status"
              >
                {zh.draft.otherChapterRunning}
              </p>
            ) : null}
            <EmptyState
              icon={<Sparkles className="h-5 w-5" />}
              title={zh.outline.contentEmpty}
              description={zh.outline.contentEmptyHint}
              compact
              action={
                <Button
                  variant="primary"
                  leftIcon={<Sparkles className="h-4 w-4" />}
                  onClick={onGenerate}
                  disabled={disabled || blockedByOther}
                >
                  {zh.outline.generateContent}
                </Button>
              }
            />
          </div>
        ) : null}

        {showDraftFailure && !showContent ? (
          <ErrorState
            title={zh.draft.failed}
            message={currentError ?? zh.draft.failedFallback}
            onRetry={canRetry ? () => void handleRetry() : onGenerate}
            compact
          />
        ) : null}

        {showRefreshFailure ? (
          <ErrorState
            title={zh.outline.contentLoadFailed}
            message={currentRefreshError ?? zh.draft.failedFallback}
            onRetry={handleReloadContent}
            compact
          />
        ) : null}

        {showContent ? (
          <>
            {currentLint ? (
              <div
                role="status"
                className="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-lg border border-info/20 bg-info-soft/40 px-3 py-2 text-xs leading-5 text-info"
              >
                <span>{draftLintSummary(currentLint)}</span>
                {lintChips.chips.length > 0 ? (
                  <div className="flex flex-wrap items-center gap-1.5">
                    {lintChips.chips.map((chip) => (
                      <Badge key={chip.key} tone="info">
                        {chip.label} · {chip.count}
                      </Badge>
                    ))}
                    {lintChips.overflow > 0 ? (
                      <Badge tone="muted">{zh.draft.lintMore(lintChips.overflow)}</Badge>
                    ) : null}
                  </div>
                ) : null}
              </div>
            ) : null}

            {editing && !running ? (
              <Textarea
                value={editable}
                onChange={(event) => setEditable(event.target.value)}
                rows={20}
                disabled={disabled}
                placeholder={zh.outline.contentPlaceholder}
                className="min-h-[360px] text-sm leading-7"
                aria-label={zh.outline.contentEdit}
              />
            ) : (
              <div
                role="article"
                tabIndex={0}
                aria-label={zh.outline.contentRead}
                className="focus-ring min-h-[240px] whitespace-pre-wrap break-words rounded-xl border border-line/60 bg-paper/40 px-4 py-3.5 text-sm leading-8 text-ink"
              >
                {editable}
              </div>
            )}

            {currentMessage ? <WarningBanner message={currentMessage} /> : null}

            {currentError ? (
              <div
                role="alert"
                className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-danger/30 bg-danger-soft/50 px-3 py-2 text-xs leading-5 text-danger"
              >
                <span>
                  {zh.draft.failed}：{currentError}
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 px-2 text-danger hover:bg-danger-soft"
                  onClick={canRetry ? () => void handleRetry() : onGenerate}
                >
                  {zh.draft.retry}
                </Button>
              </div>
            ) : null}

            {currentRefreshError ? (
              <div
                role="alert"
                className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-danger/30 bg-danger-soft/50 px-3 py-2 text-xs leading-5 text-danger"
              >
                <span>{currentRefreshError}</span>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 px-2 text-danger hover:bg-danger-soft"
                  onClick={handleReloadContent}
                >
                  {zh.draft.reloadContent}
                </Button>
              </div>
            ) : null}

            <div className="flex flex-wrap items-center justify-between gap-2 border-t border-line/60 pt-3">
              <div className="flex items-center gap-2 text-[11px] text-faint">
                <span>{formatNumber(liveWordCount)} 字</span>
                {dirty ? (
                  <span className="text-warning">{zh.outline.contentUnsavedState}</span>
                ) : (
                  <span>{zh.outline.contentSavedState}</span>
                )}
              </div>
              <div className="flex items-center gap-2">
                {dirty && !running ? (
                  <Button variant="ghost" size="sm" onClick={() => void handleDiscard()}>
                    {zh.outline.discardContent}
                  </Button>
                ) : null}
                <Button
                  variant="primary"
                  size="sm"
                  leftIcon={<Save className="h-3.5 w-3.5" />}
                  loading={saving}
                  disabled={!dirty || disabled || running}
                  onClick={() => void handleSave()}
                >
                  {zh.outline.saveContent}
                </Button>
              </div>
            </div>
          </>
        ) : null}
      </div>
    </section>
  );
}

function LiveProgress({
  stage,
  progress,
  elapsed,
  creating,
}: {
  stage: string;
  progress: number;
  elapsed: number;
  creating: boolean;
}) {
  return (
    <div
      className="rounded-xl border border-line/60 bg-paper/50 p-3.5"
      role="status"
      aria-live="polite"
      aria-label={zh.draft.progressLabel}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="inline-flex items-center gap-1.5 text-xs font-medium text-accent-strong dark:text-accent">
          <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden />
          {stage}
        </span>
        <span className="font-mono text-[11px] text-faint">{zh.draft.elapsed(elapsed)}</span>
      </div>
      <ProgressBar
        value={creating ? 0 : progress}
        max={100}
        className="mt-2.5"
        tone="accent"
        label={zh.draft.progressLabel}
        valueText={`${Math.round(creating ? 0 : progress)}%`}
      />
      <p className="mt-2 text-[11px] text-faint">
        {creating ? zh.draft.creating : zh.draft.progressHint}
      </p>
    </div>
  );
}

function WarningBanner({ message }: { message: string }) {
  return (
    <p
      role="alert"
      className="rounded-lg border border-warning/30 bg-warning-soft/50 px-3 py-2 text-xs leading-5 text-warning"
    >
      {message}
    </p>
  );
}
