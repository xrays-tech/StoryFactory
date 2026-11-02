import { useEffect, useState } from "react";
import { Sparkles } from "lucide-react";
import { zh } from "../../copy/zh";
import { useChapterDraftStore, type DraftPrefs } from "../../stores/chapterDraftStore";
import { useNovelStore } from "../../stores/novelStore";
import { userMessage } from "../../lib/errors";
import { resolveStylePayload, STYLE_LIMITS, type DraftPov } from "../../lib/draftStyle";
import {
  DRAFT_TARGET_MAX,
  DRAFT_TARGET_MIN,
  draftTargetWordsForChapter,
  parseTargetWords,
} from "../../lib/targetWords";
import { cn } from "../../lib/cn";
import { Button } from "../common/Button";
import { Dialog } from "../common/Dialog";
import { Field } from "../common/Field";
import { Input, Textarea } from "../common/Input";

const STYLE_NAME_LIMIT = STYLE_LIMITS.name;
const STYLE_SAMPLE_LIMIT = STYLE_LIMITS.sample;
const INSTRUCTION_LIMIT = STYLE_LIMITS.instruction;
const TARGET_MIN = DRAFT_TARGET_MIN;
const TARGET_MAX = DRAFT_TARGET_MAX;

export type ChapterDraftDialogProps = {
  open: boolean;
  onClose: () => void;
  novelId: string;
  chapterId: string;
  /** 章节记录的目标字数；缺失时按 3500 处理。 */
  chapterTargetWords?: number | null;
};

export function ChapterDraftDialog({
  open,
  onClose,
  novelId,
  chapterId,
  chapterTargetWords,
}: ChapterDraftDialogProps) {
  const creatingDraft = useChapterDraftStore((s) => s.creating);
  const createDraft = useChapterDraftStore((s) => s.create);
  const current = useNovelStore((s) => s.current);
  const profile = current?.id === novelId ? current : null;
  const hasProfile =
    !!profile &&
    [profile.style_name, profile.style_sample, profile.style_instruction, profile.pov].some(
      (value) => !!value?.trim(),
    );

  const defaultTargetWords = draftTargetWordsForChapter(chapterTargetWords);

  const [styleName, setStyleName] = useState("");
  const [styleSample, setStyleSample] = useState("");
  const [pov, setPov] = useState<DraftPov>("第三人称");
  const [targetWords, setTargetWords] = useState(() => String(defaultTargetWords));
  const [instruction, setInstruction] = useState("");
  const [deslop, setDeslop] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const profileId = profile?.id ?? null;

  useEffect(() => {
    if (!open) return;
    const source = useNovelStore.getState().current;
    const novel = source?.id === novelId ? source : null;
    setStyleName(novel?.style_name ?? "");
    setStyleSample(novel?.style_sample ?? "");
    setPov(novel?.pov === "第一人称" ? "第一人称" : "第三人称");
    setTargetWords(String(defaultTargetWords));
    setInstruction(novel?.style_instruction ?? "");
    setDeslop(true);
    setError(null);
  }, [open, chapterId, novelId, profileId, defaultTargetWords]);

  const submit = async () => {
    const target = parseTargetWords(targetWords, TARGET_MIN, TARGET_MAX);
    if (target === null) {
      setError(zh.draft.targetWordsInvalid(TARGET_MIN, TARGET_MAX));
      return;
    }
    if (styleSample.length > STYLE_SAMPLE_LIMIT) {
      setError(zh.draft.styleSampleHint(STYLE_SAMPLE_LIMIT));
      return;
    }
    if (instruction.length > INSTRUCTION_LIMIT) {
      setError(zh.draft.instructionHint(INSTRUCTION_LIMIT));
      return;
    }

    const style = resolveStylePayload(profile, {
      style_name: styleName,
      style_sample: styleSample,
      instruction,
      pov,
    });

    const prefs: DraftPrefs = {
      ...style,
      target_words: target,
      deslop,
    };

    try {
      await createDraft(novelId, chapterId, prefs);
      onClose();
    } catch (err) {
      setError(userMessage(err));
    }
  };

  return (
    <Dialog
      open={open}
      onClose={creatingDraft ? () => {} : onClose}
      title={zh.draft.title}
      description={zh.draft.subtitle}
      size="lg"
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={creatingDraft}>
            {zh.common.cancel}
          </Button>
          <Button
            variant="primary"
            leftIcon={<Sparkles className="h-4 w-4" />}
            loading={creatingDraft}
            onClick={() => void submit()}
          >
            {zh.draft.create}
          </Button>
        </>
      }
    >
      <form
        className="space-y-5"
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        {hasProfile ? (
          <p className="rounded-lg border border-accent/20 bg-accent-soft/40 px-3 py-2 text-xs leading-5 text-accent-strong">
            {zh.draft.profilePrefilled}
          </p>
        ) : null}

        <Field label={zh.draft.styleName} htmlFor="draft-style-name">
          <Input
            id="draft-style-name"
            value={styleName}
            onChange={(event) => {
              setStyleName(event.target.value.slice(0, STYLE_NAME_LIMIT));
              setError(null);
            }}
            placeholder={zh.draft.styleNamePlaceholder}
            maxLength={STYLE_NAME_LIMIT}
            data-autofocus
          />
        </Field>

        <Field
          label={zh.draft.styleSample}
          htmlFor="draft-style-sample"
          hint={zh.draft.styleSampleHint(STYLE_SAMPLE_LIMIT)}
        >
          <Textarea
            id="draft-style-sample"
            rows={6}
            value={styleSample}
            onChange={(event) => {
              setStyleSample(event.target.value.slice(0, STYLE_SAMPLE_LIMIT));
              setError(null);
            }}
            placeholder={zh.draft.styleSamplePlaceholder}
          />
          <div className="mt-1 flex justify-end font-mono text-[10px] text-faint">
            {styleSample.length.toLocaleString("zh-CN")} / {STYLE_SAMPLE_LIMIT.toLocaleString("zh-CN")}
          </div>
        </Field>

        <Field
          label={zh.draft.pov}
          hint={profile?.pov ? `项目默认：${profile.pov}。` : zh.draft.povHint}
        >
          <div
            className="inline-flex items-center gap-1 rounded-lg border border-line bg-paper p-1"
            role="radiogroup"
            aria-label={zh.draft.pov}
          >
            {(["第三人称", "第一人称"] as const).map((option) => {
              const active = pov === option;
              return (
                <button
                  key={option}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  onClick={() => {
                    setPov(option);
                    setError(null);
                  }}
                  className={cn(
                    "focus-ring rounded-md px-3 py-1.5 text-xs font-medium transition-colors",
                    active
                      ? "bg-accent text-white shadow-sm dark:text-[#201612]"
                      : "text-muted hover:bg-line/40 hover:text-ink",
                  )}
                >
                  {option === "第三人称" ? zh.draft.povThird : zh.draft.povFirst}
                </button>
              );
            })}
          </div>
        </Field>

        <div className="grid gap-5 sm:grid-cols-2">
          <Field
            label={zh.draft.targetWords}
            htmlFor="draft-target-words"
            hint={zh.draft.targetWordsFromChapter(TARGET_MIN, TARGET_MAX)}
          >
            <Input
              id="draft-target-words"
              type="number"
              min={TARGET_MIN}
              max={TARGET_MAX}
              step={100}
              value={targetWords}
              onChange={(event) => {
                setTargetWords(event.target.value);
                setError(null);
              }}
              inputMode="numeric"
            />
          </Field>
          <div className="flex items-end">
            <p className="text-xs leading-5 text-faint">{zh.draft.targetWordsHint}</p>
          </div>
        </div>

        <label className="flex cursor-pointer items-start gap-3 rounded-xl border border-line/70 bg-raised px-3 py-3">
          <input
            type="checkbox"
            checked={deslop}
            onChange={(event) => {
              setDeslop(event.target.checked);
              setError(null);
            }}
            className="mt-0.5 h-4 w-4 rounded border-line text-accent focus:ring-accent"
          />
          <span>
            <span className="block text-sm font-medium text-ink">{zh.draft.deslop}</span>
            <span className="mt-0.5 block text-[11px] leading-5 text-muted">
              {zh.draft.deslopHint}
            </span>
          </span>
        </label>

        <Field
          label={zh.draft.instruction}
          htmlFor="draft-instruction"
          hint={zh.draft.instructionHint(INSTRUCTION_LIMIT)}
        >
          <Textarea
            id="draft-instruction"
            rows={4}
            value={instruction}
            onChange={(event) => {
              setInstruction(event.target.value.slice(0, INSTRUCTION_LIMIT));
              setError(null);
            }}
            placeholder={zh.draft.instructionPlaceholder}
          />
          <div className="mt-1 flex justify-end font-mono text-[10px] text-faint">
            {instruction.length.toLocaleString("zh-CN")} / {INSTRUCTION_LIMIT.toLocaleString("zh-CN")}
          </div>
        </Field>

        {error ? (
          <p
            className="rounded-lg border border-danger/25 bg-danger-soft/50 px-3 py-2 text-sm text-danger"
            role="alert"
          >
            {error}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}
