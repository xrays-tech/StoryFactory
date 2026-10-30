import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  CheckCircle2,
  ChevronRight,
  FileText,
  FileUp,
  ListTree,
  Link2,
  Package,
  RefreshCw,
  Square,
  Upload,
  Users,
} from "lucide-react";
import type { NovelImportStatus, RoleType } from "../../types/domain";
import { importStageLabel, importStatusLabel, roleLabel, zh } from "../../copy/zh";
import {
  useNovelImportStore,
  validateImportFile,
  type ImportJob,
} from "../../stores/novelImportStore";
import { useUiStore } from "../../stores/uiStore";
import { bytesToSize, truncate } from "../../lib/format";
import { userMessage } from "../../lib/errors";
import { cn } from "../../lib/cn";
import { PageHeader } from "../../components/common/PageHeader";
import { Button } from "../../components/common/Button";
import { Badge, type BadgeTone } from "../../components/common/Badge";
import { ProgressBar } from "../../components/common/Progress";
import { Field } from "../../components/common/Field";
import { Input } from "../../components/common/Input";
import { Tabs } from "../../components/common/Tabs";
import { EmptyState } from "../../components/common/EmptyState";

type Step = 1 | 2 | 3;
type PreviewTab = "volumes" | "characters" | "relations" | "items";

const statusTone: Record<NovelImportStatus, BadgeTone> = {
  queued: "neutral",
  analyzing: "accent",
  ready: "success",
  failed: "danger",
  cancelled: "muted",
};

function clampPercent(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, Math.round(value)));
}

function jobPercent(job: ImportJob): number {
  if (job.chunks_total > 0) {
    return clampPercent((job.chunks_done / job.chunks_total) * 100);
  }
  return clampPercent(job.progress);
}

function roleText(roleType: string): string {
  return roleLabel[roleType as RoleType] ?? roleType;
}

/** 后端可能原样回显 percent-encoded 的 X-Filename，展示时尽量还原。 */
function displayFilename(filename: string): string {
  try {
    return decodeURIComponent(filename);
  } catch {
    return filename;
  }
}

function StepRail({ current }: { current: Step }) {
  const steps: Array<{ id: Step; label: string }> = [
    { id: 1, label: zh.importNovel.stepUpload },
    { id: 2, label: zh.importNovel.stepAnalyzing },
    { id: 3, label: zh.importNovel.stepPreview },
  ];
  return (
    <ol className="flex items-center gap-3" aria-label="导入步骤">
      {steps.map((step, index) => {
        const active = step.id === current;
        const done = step.id < current;
        return (
          <li key={step.id} className={cn("flex items-center gap-3", index > 0 && "flex-1")}>
            {index > 0 ? (
              <span
                className={cn(
                  "hidden h-px flex-1 sm:block",
                  done || active ? "bg-accent/50" : "bg-line/70",
                )}
                aria-hidden
              />
            ) : null}
            <span
              className={cn(
                "inline-flex items-center gap-2 rounded-full border px-2.5 py-1 text-[11px] font-medium transition-colors",
                active
                  ? "border-accent/30 bg-accent-soft text-accent-strong dark:text-accent"
                  : done
                    ? "border-success/25 bg-success-soft text-success"
                    : "border-line/70 bg-surface/60 text-faint",
              )}
            >
              <span className="font-mono text-[10px] tracking-widest">
                {done ? <CheckCircle2 className="h-3 w-3" /> : `0${step.id}`}
              </span>
              {step.label}
            </span>
          </li>
        );
      })}
    </ol>
  );
}

function Alert({ message }: { message: string }) {
  return (
    <p
      role="alert"
      className="rounded-lg border border-danger/25 bg-danger-soft/50 px-3 py-2 text-sm leading-6 text-danger"
    >
      {message}
    </p>
  );
}

export function ImportNovel() {
  const navigate = useNavigate();
  const toast = useUiStore((s) => s.toast);
  const askConfirm = useUiStore((s) => s.askConfirm);

  const file = useNovelImportStore((s) => s.file);
  const job = useNovelImportStore((s) => s.job);
  const preview = useNovelImportStore((s) => s.preview);
  const loading = useNovelImportStore((s) => s.loading);
  const uploading = useNovelImportStore((s) => s.uploading);
  const creating = useNovelImportStore((s) => s.creating);
  const error = useNovelImportStore((s) => s.error);
  const connecting = useNovelImportStore((s) => s.connecting);
  const streamError = useNovelImportStore((s) => s.streamError);
  const upload = useNovelImportStore((s) => s.upload);
  const connect = useNovelImportStore((s) => s.connect);
  const refresh = useNovelImportStore((s) => s.refresh);
  const loadPreview = useNovelImportStore((s) => s.loadPreview);
  const createProject = useNovelImportStore((s) => s.createProject);
  const cancel = useNovelImportStore((s) => s.cancel);
  const reset = useNovelImportStore((s) => s.reset);

  const inputRef = useRef<HTMLInputElement>(null);
  const previewRequested = useRef<string | null>(null);

  const [selected, setSelected] = useState<File | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [title, setTitle] = useState("");
  const [tab, setTab] = useState<PreviewTab>("volumes");

  useEffect(() => {
    const state = useNovelImportStore.getState();
    if (state.job) {
      if (state.job.status === "queued" || state.job.status === "analyzing") {
        state.connect(state.job.id);
      } else if (state.job.status === "ready" && !state.preview) {
        void state.loadPreview();
      }
    }
    return () => {
      useNovelImportStore.getState().disconnect();
    };
  }, []);

  useEffect(() => {
    if (job?.status !== "ready" || preview) return;
    if (previewRequested.current === job.id) return;
    previewRequested.current = job.id;
    void loadPreview();
  }, [job?.id, job?.status, preview, loadPreview]);

  useEffect(() => {
    if (preview) setTitle(preview.title);
  }, [preview?.job_id]);

  const currentStep: Step = preview || job?.status === "ready" ? 3 : job ? 2 : 1;

  const pickFile = (next: File | null) => {
    if (!next) return;
    const problem = validateImportFile(next);
    if (problem) {
      setSelected(null);
      setFileError(problem);
      return;
    }
    setFileError(null);
    setSelected(next);
  };

  const start = async () => {
    if (!selected) return;
    await upload(selected);
  };

  const restart = () => {
    reset();
    setSelected(null);
    setFileError(null);
    setTitle("");
    setTab("volumes");
    previewRequested.current = null;
  };

  const retryPreview = () => {
    previewRequested.current = null;
    void loadPreview();
  };

  const stop = async () => {
    const ok = await askConfirm({
      title: zh.importNovel.cancelTitle,
      message: zh.importNovel.cancelConfirm,
      confirmLabel: zh.importNovel.cancelAnalyze,
      cancelLabel: zh.common.back,
      tone: "danger",
    });
    if (!ok) return;
    try {
      await cancel();
      toast({ tone: "info", message: zh.importNovel.cancelDone });
      restart();
    } catch (err) {
      toast({ tone: "error", message: userMessage(err) });
    }
  };

  const generate = async () => {
    try {
      const novel = await createProject(title);
      toast({
        tone: "success",
        title: zh.importNovel.generated,
        message: `《${novel.title}》已创建，可以继续补完设定。`,
      });
      reset();
      navigate(`/novel/${novel.id}`);
    } catch {
      // store 已写入 error，页面统一展示。
    }
  };

  return (
    <div className="space-y-7">
      <PageHeader
        eyebrow={zh.importNovel.eyebrow}
        title={zh.importNovel.title}
        description={zh.importNovel.subtitle}
        actions={
          currentStep === 3 ? (
            <Button variant="ghost" size="sm" onClick={restart}>
              {zh.importNovel.reupload}
            </Button>
          ) : null
        }
      />

      <StepRail current={currentStep} />

      {currentStep === 1 ? (
        <section className="surface-card p-5 sm:p-6">
          <input
            ref={inputRef}
            type="file"
            accept=".txt,text/plain"
            className="sr-only"
            onChange={(event) => {
              pickFile(event.target.files?.[0] ?? null);
              event.target.value = "";
            }}
          />
          <div
            className={cn(
              "flex flex-col items-center justify-center rounded-xl border border-dashed px-5 py-12 text-center transition-colors",
              dragging
                ? "border-accent/60 bg-accent-soft/35"
                : "border-line/90 bg-paper/40",
            )}
            onDragOver={(event) => {
              event.preventDefault();
              setDragging(true);
            }}
            onDragLeave={() => setDragging(false)}
            onDrop={(event) => {
              event.preventDefault();
              setDragging(false);
              pickFile(event.dataTransfer.files?.[0] ?? null);
            }}
            onClick={() => inputRef.current?.click()}
            role="button"
            tabIndex={0}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                inputRef.current?.click();
              }
            }}
          >
            <span className="flex h-12 w-12 items-center justify-center rounded-xl border border-line/80 bg-raised text-accent">
              <Upload className="h-5 w-5" />
            </span>
            <p className="mt-4 font-display text-base font-semibold text-ink">
              {zh.importNovel.dropTitle}
            </p>
            <p className="mt-1.5 max-w-md text-sm leading-6 text-muted">
              {zh.importNovel.dropHint}
            </p>
            <Button
              variant="outline"
              size="sm"
              className="mt-4"
              leftIcon={<FileText className="h-3.5 w-3.5" />}
              onClick={(event) => {
                event.stopPropagation();
                inputRef.current?.click();
              }}
            >
              {zh.importNovel.chooseFile}
            </Button>
          </div>

          {selected ? (
            <div className="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-line/80 bg-raised/70 px-4 py-3">
              <div className="flex min-w-0 items-center gap-3">
                <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent-strong dark:text-accent">
                  <FileUp className="h-4 w-4" />
                </span>
                <div className="min-w-0">
                  <p className="truncate text-sm font-medium text-ink">{selected.name}</p>
                  <p className="font-mono text-[11px] text-faint">
                    {zh.importNovel.fileSelected} · {bytesToSize(selected.size)}
                  </p>
                </div>
              </div>
              <Button
                variant="primary"
                loading={uploading}
                leftIcon={<FileUp className="h-4 w-4" />}
                onClick={() => void start()}
              >
                {uploading ? zh.importNovel.uploading : zh.importNovel.startAnalyze}
              </Button>
            </div>
          ) : null}

          {fileError ? (
            <div className="mt-4">
              <Alert message={fileError} />
            </div>
          ) : null}
          {error && !job ? (
            <div className="mt-4">
              <Alert message={error} />
            </div>
          ) : null}
          {file && !selected ? (
            <p className="mt-4 text-xs text-faint">
              上一次上传的 {file.name} 尚未完成，重新选择文件会开始新的分析。
            </p>
          ) : null}
        </section>
      ) : null}

      {currentStep === 2 && job ? (
        <section className="surface-card p-5 sm:p-6">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <p className="flex items-center gap-2 text-[11px] font-medium tracking-wide text-muted">
                <FileText className="h-3.5 w-3.5 text-faint" />
                <span className="truncate">{displayFilename(job.filename)}</span>
                <span className="font-mono text-[10px] text-faint">{bytesToSize(job.size_bytes)}</span>
              </p>
              <h2 className="mt-2 font-display text-xl font-semibold tracking-tight text-ink">
                {job.status === "failed" || job.status === "cancelled"
                  ? zh.importNovel.failedTitle
                  : zh.importNovel.analyzingTitle}
              </h2>
              <p className="mt-1.5 text-sm leading-6 text-muted">{zh.importNovel.analyzingHint}</p>
            </div>
            <Badge tone={statusTone[job.status]} dot pulse={job.status === "analyzing"}>
              {importStatusLabel[job.status]}
            </Badge>
          </div>

          {job.status !== "failed" && job.status !== "cancelled" ? (
            <>
              <div className="mt-6 space-y-2.5">
                <div className="flex items-center justify-between text-xs">
                  <span className="text-muted">{importStageLabel[job.stage]}</span>
                  <span className="font-mono text-faint">{jobPercent(job)}%</span>
                </div>
                <ProgressBar value={jobPercent(job)} max={100} />
                <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-faint">
                  <span>{zh.importNovel.chunks(job.chunks_done, job.chunks_total)}</span>
                  <span className="inline-flex items-center gap-1.5">
                    <span
                      className={cn(
                        "h-1.5 w-1.5 rounded-full",
                        streamError
                          ? "bg-warning"
                          : connecting
                            ? "animate-pulse-soft bg-accent"
                            : "bg-success",
                      )}
                      aria-hidden
                    />
                    {zh.importNovel.connection}：
                    {streamError
                      ? zh.importNovel.reconnecting
                      : connecting
                        ? zh.importNovel.connecting
                        : zh.importNovel.connected}
                  </span>
                </div>
              </div>

              {job.message ? (
                <p className="mt-4 rounded-lg border border-line/70 bg-paper/50 px-3 py-2 text-xs leading-5 text-muted">
                  {job.message}
                </p>
              ) : null}

              {streamError ? (
                <div className="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-lg border border-warning/25 bg-warning-soft/50 px-3 py-2">
                  <p className="text-xs leading-5 text-warning">{streamError}</p>
                  <Button
                    size="sm"
                    variant="outline"
                    leftIcon={<RefreshCw className="h-3.5 w-3.5" />}
                    onClick={() => {
                      connect(job.id);
                      void refresh(job.id);
                    }}
                  >
                    {zh.common.retry}
                  </Button>
                </div>
              ) : null}

              <div className="mt-6 flex flex-wrap items-center justify-between gap-3 border-t border-line/60 pt-4">
                <p className="text-[11px] text-faint">
                  任务编号 <span className="font-mono text-muted">{job.id.slice(0, 8)}</span>
                </p>
                <Button
                  variant="outline"
                  size="sm"
                  leftIcon={<Square className="h-3.5 w-3.5" />}
                  onClick={() => void stop()}
                >
                  {zh.importNovel.cancelAnalyze}
                </Button>
              </div>
            </>
          ) : (
            <div className="mt-5 space-y-4">
              <Alert message={job.error ?? job.message ?? error ?? "分析未完成，请重新上传。"} />
              <Button variant="primary" leftIcon={<FileUp className="h-4 w-4" />} onClick={restart}>
                {zh.importNovel.reupload}
              </Button>
            </div>
          )}
        </section>
      ) : null}

      {currentStep === 3 ? (
        <section className="surface-card p-5 sm:p-6">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h2 className="font-display text-xl font-semibold tracking-tight text-ink">
                {zh.importNovel.previewTitle}
              </h2>
              <p className="mt-1.5 max-w-xl text-sm leading-6 text-muted">
                {zh.importNovel.previewHint}
              </p>
            </div>
            <Badge tone="success" dot>
              {importStatusLabel.ready}
            </Badge>
          </div>

          {!preview ? (
            <div className="mt-6 space-y-4">
              {loading ? (
                <div className="flex items-center justify-center rounded-xl border border-line/70 bg-paper/40 py-12">
                  <RefreshCw className="mr-2 h-4 w-4 animate-spin text-muted" />
                  <span className="text-sm text-muted">{zh.common.loading}</span>
                </div>
              ) : (
                <>
                  <Alert message={error ?? "预览还没有准备好，请重试。"} />
                  <Button
                    variant="outline"
                    size="sm"
                    leftIcon={<RefreshCw className="h-3.5 w-3.5" />}
                    onClick={retryPreview}
                  >
                    {zh.importNovel.retryPreview}
                  </Button>
                </>
              )}
            </div>
          ) : (
            <div className="mt-6 space-y-6">
              <Field label={zh.importNovel.titleLabel} htmlFor="import-title">
                <Input
                  id="import-title"
                  value={title}
                  onChange={(event) => setTitle(event.target.value)}
                  placeholder={preview.title}
                  maxLength={200}
                />
              </Field>

              {preview.summary || preview.world_view || preview.background ? (
                <div className="grid gap-3 rounded-xl border border-line/70 bg-paper/40 p-4 sm:grid-cols-3">
                  {[
                    { label: zh.novel.worldView, value: preview.world_view },
                    { label: zh.novel.background, value: preview.background },
                    { label: zh.novel.summary, value: preview.summary },
                  ].map((item) => (
                    <div key={item.label} className="min-w-0">
                      <p className="text-[11px] font-medium tracking-wide text-faint">{item.label}</p>
                      <p className="mt-1.5 line-clamp-5 text-sm leading-6 text-muted">
                        {item.value || "—"}
                      </p>
                    </div>
                  ))}
                </div>
              ) : null}

              <Tabs<PreviewTab>
                value={tab}
                onChange={setTab}
                items={[
                  {
                    value: "volumes",
                    label: zh.importNovel.volumes,
                    badge: <Badge tone="muted">{preview.volumes.length}</Badge>,
                  },
                  {
                    value: "characters",
                    label: zh.importNovel.characters,
                    badge: <Badge tone="muted">{preview.characters.length}</Badge>,
                  },
                  {
                    value: "relations",
                    label: zh.importNovel.relations,
                    badge: <Badge tone="muted">{preview.relations.length}</Badge>,
                  },
                  {
                    value: "items",
                    label: zh.importNovel.items,
                    badge: <Badge tone="muted">{preview.items.length}</Badge>,
                  },
                ]}
              />

              {tab === "volumes" ? (
                preview.volumes.length === 0 ? (
                  <EmptyState compact icon={<ListTree className="h-5 w-5" />} title={zh.common.empty} />
                ) : (
                  <div className="space-y-3">
                    {preview.volumes.map((volume, index) => (
                      <details
                        key={`${volume.title}-${index}`}
                        className="group rounded-xl border border-line/80 bg-raised/60"
                        open={index === 0}
                      >
                        <summary className="flex cursor-pointer list-none items-center gap-3 px-4 py-3 [&::-webkit-details-marker]:hidden">
                          <ChevronRight className="h-4 w-4 shrink-0 text-faint transition-transform group-open:rotate-90" />
                          <span className="min-w-0 flex-1">
                            <span className="block truncate font-display text-sm font-semibold text-ink">
                              {volume.title || zh.importNovel.volumeFallback}
                            </span>
                            {volume.summary ? (
                              <span className="mt-0.5 block truncate text-[11px] text-faint">
                                {volume.summary}
                              </span>
                            ) : null}
                          </span>
                          <Badge tone="muted">{zh.importNovel.chapterCount(volume.chapters.length)}</Badge>
                        </summary>
                        <div className="border-t border-line/60 px-4 py-3">
                          {volume.chapters.length === 0 ? (
                            <p className="text-xs text-faint">{zh.importNovel.noChapters}</p>
                          ) : (
                            <ol className="space-y-2.5">
                              {volume.chapters.map((chapter, chapterIndex) => (
                                <li key={`${chapter.title}-${chapterIndex}`} className="flex gap-3">
                                  <span className="mt-0.5 font-mono text-[10px] tracking-widest text-faint">
                                    {String(chapterIndex + 1).padStart(2, "0")}
                                  </span>
                                  <div className="min-w-0">
                                    <p className="truncate text-sm text-ink">{chapter.title}</p>
                                    {chapter.outline ? (
                                      <p className="mt-0.5 text-[11px] leading-5 text-muted">
                                        {truncate(chapter.outline, 160)}
                                      </p>
                                    ) : null}
                                  </div>
                                </li>
                              ))}
                            </ol>
                          )}
                        </div>
                      </details>
                    ))}
                  </div>
                )
              ) : null}

              {tab === "characters" ? (
                preview.characters.length === 0 ? (
                  <EmptyState compact icon={<Users className="h-5 w-5" />} title={zh.importNovel.noCharacters} />
                ) : (
                  <div className="grid gap-3 sm:grid-cols-2">
                    {preview.characters.map((character, index) => (
                      <article
                        key={`${character.name}-${index}`}
                        className="rounded-xl border border-line/80 bg-raised/60 p-4"
                      >
                        <div className="flex items-center justify-between gap-2">
                          <h3 className="min-w-0 truncate font-display text-sm font-semibold text-ink">
                            {character.name}
                          </h3>
                          <Badge tone="info">{roleText(character.role_type)}</Badge>
                        </div>
                        <p className="mt-2 line-clamp-3 text-xs leading-5 text-muted">
                          {character.biography ||
                            character.personality ||
                            character.current_goal ||
                            character.appearance ||
                            "—"}
                        </p>
                        {character.current_goal ? (
                          <p className="mt-2 text-[11px] leading-5 text-faint">
                            {zh.character.currentGoal}：{character.current_goal}
                          </p>
                        ) : null}
                      </article>
                    ))}
                  </div>
                )
              ) : null}

              {tab === "relations" ? (
                preview.relations.length === 0 ? (
                  <EmptyState compact icon={<Link2 className="h-5 w-5" />} title={zh.importNovel.noRelations} />
                ) : (
                  <ul className="divide-y divide-line/60 rounded-xl border border-line/80 bg-raised/60">
                    {preview.relations.map((relation, index) => (
                      <li
                        key={`${relation.a}-${relation.b}-${index}`}
                        className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3"
                      >
                        <p className="min-w-0 flex-1 text-sm text-ink">
                          <span className="font-medium">{relation.a}</span>
                          <span className="mx-1.5 text-faint">—</span>
                          <span className="font-medium">{relation.b}</span>
                        </p>
                        <div className="flex items-center gap-2">
                          <Badge tone={relation.affinity >= 0 ? "accent" : "danger"}>
                            {zh.importNovel.affinity} {relation.affinity}
                          </Badge>
                          <Badge tone="info">
                            {zh.importNovel.trust} {relation.trust}
                          </Badge>
                        </div>
                        {relation.conflict_desc ? (
                          <p className="w-full text-[11px] leading-5 text-muted">
                            {relation.conflict_desc}
                          </p>
                        ) : null}
                      </li>
                    ))}
                  </ul>
                )
              ) : null}

              {tab === "items" ? (
                preview.items.length === 0 ? (
                  <EmptyState compact icon={<Package className="h-5 w-5" />} title={zh.importNovel.noItems} />
                ) : (
                  <ul className="divide-y divide-line/60 rounded-xl border border-line/80 bg-raised/60">
                    {preview.items.map((item, index) => (
                      <li key={`${item.name}-${index}`} className="flex flex-wrap items-start gap-3 px-4 py-3">
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-sm font-medium text-ink">{item.name}</p>
                          <p className="mt-0.5 line-clamp-2 text-[11px] leading-5 text-muted">
                            {item.description || item.features || "—"}
                          </p>
                        </div>
                        <div className="flex items-center gap-2">
                          {item.owner ? <Badge tone="muted">{item.owner}</Badge> : null}
                          <Badge tone={item.importance >= 4 ? "warning" : "neutral"}>
                            {zh.importNovel.importance} {item.importance}
                          </Badge>
                        </div>
                      </li>
                    ))}
                  </ul>
                )
              ) : null}

              {error && preview ? (
                <div className="space-y-3">
                  <Alert message={error} />
                </div>
              ) : null}

              <div className="flex flex-wrap items-center justify-between gap-3 border-t border-line/60 pt-4">
                <p className="text-[11px] text-faint">
                  共 {preview.volumes.length} 卷 · {preview.characters.length} 人物 ·{" "}
                  {preview.items.length} 道具
                </p>
                <div className="flex items-center gap-2">
                  <Button variant="ghost" onClick={restart}>
                    {zh.importNovel.reupload}
                  </Button>
                  <Button
                    variant="primary"
                    loading={creating}
                    leftIcon={<CheckCircle2 className="h-4 w-4" />}
                    onClick={() => void generate()}
                  >
                    {creating ? zh.importNovel.generating : zh.importNovel.generate}
                  </Button>
                </div>
              </div>
            </div>
          )}
        </section>
      ) : null}
    </div>
  );
}
