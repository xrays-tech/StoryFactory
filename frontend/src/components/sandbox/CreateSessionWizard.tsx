import { useEffect, useMemo, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  Clapperboard,
  Plus,
  Repeat,
  ScrollText,
  Sparkles,
  Trash2,
  Users,
} from "lucide-react";
import type { DirectorMode } from "../../types/domain";
import { modeLabel, zh } from "../../copy/zh";
import { useCharacterStore } from "../../stores/characterStore";
import { useOutlineStore } from "../../stores/outlineStore";
import { useSandboxStore, type Session } from "../../stores/sandboxStore";
import { useUiStore } from "../../stores/uiStore";
import { userMessage } from "../../lib/errors";
import { buildCreateSessionPayload } from "../../lib/sandboxSession";
import { newIdempotencyKey } from "../../api/client";
import { cn } from "../../lib/cn";
import { Dialog } from "../common/Dialog";
import { Button } from "../common/Button";
import { Field } from "../common/Field";
import { Input, Select, Textarea } from "../common/Input";
import { RoleBadge } from "../common/StatusBadge";
import { Badge } from "../common/Badge";

type ScriptStepForm = { actor_id: string; beat_hint: string };

const modeCards: Array<{
  value: DirectorMode;
  icon: typeof Repeat;
  title: string;
  hint: string;
}> = [
  {
    value: "director",
    icon: Sparkles,
    title: "导演驱动",
    hint: "导演每步挑选最合适的角色，并给出行动提示。",
  },
  {
    value: "roundrobin",
    icon: Repeat,
    title: "轮转",
    hint: "角色依次行动，稳定但少一点意外。",
  },
  {
    value: "scripted",
    icon: ScrollText,
    title: "剧本驱动",
    hint: "按你写好的步骤与节拍推进，适合预演关键场景。",
  },
];

export function CreateSessionWizard({
  open,
  onClose,
  novelId,
  defaultChapterId,
  onCreated,
}: {
  open: boolean;
  onClose: () => void;
  novelId: string;
  defaultChapterId?: string | null;
  onCreated: (session: Session) => void;
}) {
  const characters = useCharacterStore((s) => s.items);
  const volumes = useOutlineStore((s) => s.volumes);
  const chaptersByVolume = useOutlineStore((s) => s.chaptersByVolume);
  const outlineNovelId = useOutlineStore((s) => s.novelId);
  const outlineLoading = useOutlineStore((s) => s.loading);
  const outlineError = useOutlineStore((s) => s.error);
  const loadTree = useOutlineStore((s) => s.loadTree);
  const createSession = useSandboxStore((s) => s.createSession);
  const toast = useUiStore((s) => s.toast);

  const [step, setStep] = useState<1 | 2>(1);
  const [chapterId, setChapterId] = useState(defaultChapterId ?? "");
  const [actorIds, setActorIds] = useState<string[]>([]);
  const [mode, setMode] = useState<DirectorMode>("director");
  const [scenePrompt, setScenePrompt] = useState("");
  const [stepLimit, setStepLimit] = useState(20);
  const [directive, setDirective] = useState("");
  const [autostart, setAutostart] = useState(true);
  const [autoProse, setAutoProse] = useState(true);
  const [script, setScript] = useState<ScriptStepForm[]>([]);
  const [idemKey, setIdemKey] = useState(newIdempotencyKey("session"));
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    if (!open) return;
    setStep(1);
    setChapterId(defaultChapterId ?? "");
    setActorIds([]);
    setMode("director");
    setScenePrompt("");
    setStepLimit(20);
    setDirective("");
    setAutostart(true);
    setAutoProse(true);
    setScript([]);
    setIdemKey(newIdempotencyKey("session"));
    setSaving(false);
    setFormError(null);
    setFieldErrors({});
  }, [open, defaultChapterId]);

  // 沙盒页 loader 通常已经加载好大纲树；从其他入口打开时再补一次，失败也不阻塞提交。
  useEffect(() => {
    if (!open || outlineNovelId === novelId) return;
    void loadTree(novelId, { silent: true });
  }, [open, novelId, outlineNovelId, loadTree]);

  // 上下文预填的章节可能已被删除；树加载完成后找不到就退回整本书级。
  useEffect(() => {
    if (!open || !chapterId) return;
    if (outlineNovelId !== novelId || outlineLoading) return;
    if (outlineError) {
      setChapterId("");
      return;
    }
    const exists = volumes.some((volume) =>
      (chaptersByVolume[volume.id] ?? []).some((chapter) => chapter.id === chapterId),
    );
    if (!exists) setChapterId("");
  }, [
    open,
    novelId,
    chapterId,
    outlineNovelId,
    outlineLoading,
    outlineError,
    volumes,
    chaptersByVolume,
  ]);

  const selectedSet = useMemo(() => new Set(actorIds), [actorIds]);

  const toggleActor = (id: string) => {
    if (actorIds.includes(id)) {
      const next = actorIds.filter((item) => item !== id);
      setActorIds(next);
      setScript((steps) => steps.filter((item) => next.includes(item.actor_id)));
      return;
    }
    if (actorIds.length >= 8) {
      toast({ tone: "warning", message: "一次最多选择 8 位角色" });
      return;
    }
    setActorIds([...actorIds, id]);
  };

  const changeMode = (next: DirectorMode) => {
    setMode(next);
    if (next === "scripted" && script.length === 0 && actorIds[0]) {
      setScript([{ actor_id: actorIds[0], beat_hint: "" }]);
    }
  };

  const validateStepOne = (): boolean => {
    const errors: Record<string, string> = {};
    if (actorIds.length === 0) errors.actor_ids = "至少选择 1 位角色";
    if (actorIds.length > 8) errors.actor_ids = "最多选择 8 位角色";
    setFieldErrors(errors);
    if (Object.keys(errors).length > 0) return false;
    return true;
  };

  const validateAll = (): boolean => {
    const errors: Record<string, string> = {};
    if (actorIds.length === 0) errors.actor_ids = "至少选择 1 位角色";
    const prompt = scenePrompt.trim();
    if (prompt.length < 10) errors.scene_prompt = "场景提示至少 10 个字符";
    if (prompt.length > 4000) errors.scene_prompt = "场景提示不能超过 4000 个字符";
    if (stepLimit < 1 || stepLimit > 60) errors.step_limit = "步数上限必须在 1–60 之间";
    if (directive.length > 500) errors.user_directive = "全局指令不能超过 500 个字符";
    if (mode === "scripted") {
      const validSteps = script.filter((item) => item.actor_id);
      if (validSteps.length === 0) errors.director_config = "剧本模式至少需要一个有效步骤";
    }
    setFieldErrors(errors);
    return Object.keys(errors).length === 0;
  };

  const submit = async () => {
    if (!validateAll()) {
      if (actorIds.length === 0) setStep(1);
      return;
    }
    setSaving(true);
    setFormError(null);
    try {
      const payload = buildCreateSessionPayload({
        actorIds,
        mode,
        scenePrompt: scenePrompt.trim(),
        stepLimit,
        directorConfig:
          mode === "scripted"
            ? {
                steps: script
                  .filter((item) => item.actor_id)
                  .map((item) => ({
                    actor_id: item.actor_id,
                    beat_hint: item.beat_hint.trim() || null,
                  })),
              }
            : null,
        userDirective: directive.trim() || null,
        autostart,
        chapterId: chapterId || null,
        autoProse,
      });
      const session = await createSession(novelId, chapterId || null, payload, idemKey);
      toast({ tone: "success", message: zh.sandbox.created });
      onCreated(session);
    } catch (error) {
      const message = userMessage(error);
      const details = (error as { fieldErrors?: Record<string, string> }).fieldErrors ?? {};
      setFormError(message);
      setFieldErrors(details);
      if (details.actor_ids) setStep(1);
      // 失败响应可能被幂等中间件记录；重试必须使用新的键，否则会回放旧错误。
      setIdemKey(newIdempotencyKey("session"));
    } finally {
      setSaving(false);
    }
  };

  const chapterOptions = volumes.map((volume) => ({
    volume,
    chapters: chaptersByVolume[volume.id] ?? [],
  }));

  const chapterHint = outlineError
    ? "章节列表加载失败，本次将按整本书推演。"
    : outlineLoading
      ? "正在读取章节…"
      : "关联章节后，收束时会生成大纲并可直接定稿。";

  return (
    <Dialog
      open={open}
      onClose={() => (saving ? undefined : onClose())}
      title={zh.sandbox.newSession}
      description={step === 1 ? "第一步：选择参与的人与导演方式。" : "第二步：给出场景，让推演有起点。"}
      size="xl"
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={saving}>
            {zh.common.cancel}
          </Button>
          {step === 2 ? (
            <Button
              variant="outline"
              leftIcon={<ArrowLeft className="h-4 w-4" />}
              onClick={() => setStep(1)}
              disabled={saving}
            >
              上一步
            </Button>
          ) : null}
          {step === 1 ? (
            <Button
              variant="primary"
              onClick={() => {
                if (validateStepOne()) setStep(2);
              }}
            >
              下一步 <ArrowRight className="ml-1 h-4 w-4" />
            </Button>
          ) : (
            <Button variant="primary" onClick={() => void submit()} loading={saving}>
              创建并进入
            </Button>
          )}
        </>
      }
    >
      <div className="space-y-5">
        <div className="flex items-center gap-2" aria-label="向导步骤">
          {[1, 2].map((value) => (
            <div key={value} className="flex flex-1 items-center gap-2">
              <span
                className={cn(
                  "flex h-6 w-6 items-center justify-center rounded-full border text-[11px] font-semibold",
                  step === value
                    ? "border-accent bg-accent text-white dark:text-[#201612]"
                    : step > value
                      ? "border-success bg-success-soft text-success"
                      : "border-line text-faint",
                )}
              >
                {step > value ? <Check className="h-3.5 w-3.5" /> : value}
              </span>
              <span className={cn("text-xs", step === value ? "font-medium text-ink" : "text-faint")}>
                {value === 1 ? "角色与模式" : "场景与参数"}
              </span>
              {value === 1 ? <span className="h-px flex-1 bg-line/70" /> : null}
            </div>
          ))}
        </div>

        {step === 1 ? (
          <div className="space-y-5">
            <Field label={`${zh.sandbox.chapter}（${zh.common.optional}）`} htmlFor="session-chapter" hint={chapterHint}>
              <Select
                id="session-chapter"
                value={chapterId}
                onChange={(e) => setChapterId(e.target.value)}
              >
                <option value="">{zh.sandbox.noChapter}</option>
                {chapterOptions.map(({ volume, chapters }) =>
                  chapters.length > 0 ? (
                    <optgroup key={volume.id} label={`${volume.volume_index}. ${volume.title || "未命名卷"}`}>
                      {chapters.map((chapter) => (
                        <option key={chapter.id} value={chapter.id}>
                          {chapter.chapter_index}. {chapter.title || "未命名章节"}
                        </option>
                      ))}
                    </optgroup>
                  ) : null,
                )}
              </Select>
            </Field>

            <label
              className={cn(
                "flex items-start gap-3 rounded-xl border border-line/70 bg-raised px-3 py-3",
                chapterId ? "cursor-pointer" : "cursor-not-allowed opacity-60",
              )}
            >
              <input
                type="checkbox"
                checked={autoProse}
                disabled={!chapterId}
                onChange={(e) => setAutoProse(e.target.checked)}
                className="mt-0.5 h-4 w-4 rounded border-line text-accent focus:ring-accent"
              />
              <span>
                <span className="block text-sm font-medium text-ink">{zh.sandbox.autoProse}</span>
                <span className="mt-0.5 block text-[11px] leading-5 text-muted">
                  {chapterId ? zh.sandbox.autoProseHint : zh.sandbox.autoProseDisabled}
                </span>
              </span>
            </label>

            <Field
              label={
                <span className="flex items-center gap-2">
                  <Users className="h-3.5 w-3.5" /> {zh.sandbox.actors}
                  <Badge tone={actorIds.length > 0 ? "accent" : "warning"}>{actorIds.length} / 8</Badge>
                </span>
              }
              hint={zh.sandbox.actorsHint}
              error={fieldErrors.actor_ids}
            >
              {characters.length === 0 ? (
                <div className="rounded-xl border border-dashed border-line/90 px-4 py-6 text-center text-sm text-muted">
                  这个项目还没有角色。先去角色页创建人物，再回来发起推演。
                </div>
              ) : (
                <div className="grid max-h-[280px] gap-2 overflow-y-auto pr-1 sm:grid-cols-2 lg:grid-cols-3">
                  {characters.map((character) => {
                    const checked = selectedSet.has(character.id);
                    const disabled = !checked && actorIds.length >= 8;
                    return (
                      <label
                        key={character.id}
                        className={cn(
                          "flex cursor-pointer items-start gap-2.5 rounded-xl border px-3 py-2.5 transition-all duration-150",
                          checked
                            ? "border-accent/45 bg-accent-soft/60"
                            : "border-line/70 bg-raised hover:border-faint/70",
                          disabled && "cursor-not-allowed opacity-50",
                        )}
                      >
                        <input
                          type="checkbox"
                          className="mt-1 h-4 w-4 rounded border-line text-accent focus:ring-accent"
                          checked={checked}
                          disabled={disabled}
                          onChange={() => toggleActor(character.id)}
                        />
                        <span className="min-w-0 flex-1">
                          <span className="flex items-center gap-1.5">
                            <span className="truncate text-sm font-medium text-ink">{character.name}</span>
                            <RoleBadge role={character.role_type} />
                          </span>
                          <span className="mt-0.5 line-clamp-1 block text-[11px] text-faint">
                            {character.current_goal || character.personality || "暂无目标"}
                          </span>
                        </span>
                      </label>
                    );
                  })}
                </div>
              )}
            </Field>

            <Field label={zh.sandbox.mode} error={fieldErrors.mode}>
              <div className="grid gap-2 sm:grid-cols-3">
                {modeCards.map((card) => {
                  const Icon = card.icon;
                  const active = mode === card.value;
                  return (
                    <button
                      key={card.value}
                      type="button"
                      onClick={() => changeMode(card.value)}
                      className={cn(
                        "focus-ring rounded-xl border p-3 text-left transition-all duration-150",
                        active
                          ? "border-accent/50 bg-accent-soft/60 shadow-sm"
                          : "border-line/70 bg-raised hover:border-faint/70",
                      )}
                      aria-pressed={active}
                    >
                      <span className="flex items-center gap-2">
                        <Icon className={cn("h-4 w-4", active ? "text-accent-strong dark:text-accent" : "text-faint")} />
                        <span className="text-sm font-medium text-ink">{card.title}</span>
                      </span>
                      <span className="mt-1.5 block text-[11px] leading-5 text-muted">{card.hint}</span>
                    </button>
                  );
                })}
              </div>
            </Field>
          </div>
        ) : (
          <div className="space-y-5">
            <Field
              label={zh.sandbox.scenePrompt}
              htmlFor="session-scene"
              required
              error={fieldErrors.scene_prompt}
              hint={`${scenePrompt.trim().length} / 10–4000 字`}
            >
              <Textarea
                id="session-scene"
                rows={5}
                value={scenePrompt}
                onChange={(e) => setScenePrompt(e.target.value)}
                invalid={!!fieldErrors.scene_prompt}
                placeholder={zh.sandbox.scenePromptPlaceholder}
                data-autofocus
              />
            </Field>

            {mode === "scripted" ? (
              <Field
                label={zh.sandbox.script}
                error={fieldErrors.director_config}
                hint="按顺序指定每一步的行动角色，可附带节拍提示。"
              >
                <div className="space-y-2">
                  {script.map((item, index) => (
                    <div key={index} className="flex flex-col gap-2 rounded-xl border border-line/70 bg-raised p-2.5 sm:flex-row sm:items-center">
                      <span className="w-6 shrink-0 text-center font-mono text-[11px] text-faint">{index + 1}</span>
                      <Select
                        value={item.actor_id}
                        onChange={(e) => {
                          const next = [...script];
                          next[index] = { ...next[index], actor_id: e.target.value };
                          setScript(next);
                        }}
                        className="sm:w-40"
                        aria-label={`第 ${index + 1} 步行动角色`}
                      >
                        <option value="">选择角色</option>
                        {characters
                          .filter((character) => actorIds.includes(character.id))
                          .map((character) => (
                            <option key={character.id} value={character.id}>
                              {character.name}
                            </option>
                          ))}
                      </Select>
                      <Input
                        value={item.beat_hint}
                        onChange={(e) => {
                          const next = [...script];
                          next[index] = { ...next[index], beat_hint: e.target.value };
                          setScript(next);
                        }}
                        placeholder={zh.sandbox.beatHint}
                        aria-label={`第 ${index + 1} 步节拍提示`}
                        className="flex-1"
                      />
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8 shrink-0 hover:text-danger"
                        onClick={() => setScript(script.filter((_, i) => i !== index))}
                        aria-label={`删除第 ${index + 1} 步`}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  ))}
                  <Button
                    variant="outline"
                    size="sm"
                    leftIcon={<Plus className="h-3.5 w-3.5" />}
                    onClick={() =>
                      setScript([
                        ...script,
                        { actor_id: actorIds[0] ?? "", beat_hint: "" },
                      ])
                    }
                  >
                    {zh.sandbox.addStep}
                  </Button>
                </div>
              </Field>
            ) : null}

            <div className="grid gap-4 sm:grid-cols-[180px_1fr]">
              <Field label={zh.sandbox.stepLimit} htmlFor="session-steps" error={fieldErrors.step_limit}>
                <Input
                  id="session-steps"
                  type="number"
                  min={1}
                  max={60}
                  value={stepLimit}
                  onChange={(e) => setStepLimit(Number(e.target.value))}
                  invalid={!!fieldErrors.step_limit}
                />
              </Field>
              <Field
                label={zh.sandbox.directive}
                htmlFor="session-directive"
                error={fieldErrors.user_directive}
                hint={`${directive.length} / 500`}
              >
                <Input
                  id="session-directive"
                  value={directive}
                  onChange={(e) => setDirective(e.target.value)}
                  placeholder={zh.sandbox.directivePlaceholder}
                  maxLength={500}
                  invalid={!!fieldErrors.user_directive}
                />
              </Field>
            </div>

            <label className="flex cursor-pointer items-start gap-3 rounded-xl border border-line/70 bg-raised px-3 py-3">
              <input
                type="checkbox"
                checked={autostart}
                onChange={(e) => setAutostart(e.target.checked)}
                className="mt-0.5 h-4 w-4 rounded border-line text-accent focus:ring-accent"
              />
              <span>
                <span className="block text-sm font-medium text-ink">{zh.sandbox.autostart}</span>
                <span className="mt-0.5 block text-[11px] leading-5 text-muted">
                  关闭时只创建会话，进入控制台后手动点「开始」。
                </span>
              </span>
            </label>

            <div className="rounded-xl border border-line/60 bg-paper/50 px-3 py-2.5 text-[11px] leading-5 text-muted">
              <span className="font-medium text-ink">{modeLabel[mode]}</span> · {actorIds.length} 位角色 · 最多 {stepLimit} 步
              {chapterId ? " · 生成章节大纲" : " · 探索会话"}
              {chapterId && autoProse ? ` · ${zh.sandbox.autoProseSummary}` : ""}
            </div>
          </div>
        )}

        {formError ? (
          <p className="rounded-lg border border-danger/25 bg-danger-soft/50 px-3 py-2 text-sm text-danger" role="alert">
            {formError}
          </p>
        ) : null}
      </div>
    </Dialog>
  );
}
