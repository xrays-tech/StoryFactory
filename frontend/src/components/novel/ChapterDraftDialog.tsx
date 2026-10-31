import { useEffect, useState } from "react";
import { Sparkles } from "lucide-react";
import { zh } from "../../copy/zh";
import { useChapterDraftStore, type DraftPrefs } from "../../stores/chapterDraftStore";
import { userMessage } from "../../lib/errors";
import { cn } from "../../lib/cn";
import { Button } from "../common/Button";
import { Dialog } from "../common/Dialog";
import { Field } from "../common/Field";
import { Input, Textarea } from "../common/Input";

const STYLE_SAMPLE_LIMIT = 6000;
const INSTRUCTION_LIMIT = 1000;
const TARGET_MIN = 500;
const TARGET_MAX = 8000;
const TARGET_DEFAULT = 1200;

export type ChapterDraftDialogProps = {
  open: boolean;
  onClose: () => void;
  novelId: string;
  chapterId: string;
};

export function ChapterDraftDialog({ open, onClose, novelId, chapterId }: ChapterDraftDialogProps) {
  const creatingDraft = useChapterDraftStore((s) => s.creating);
  const createDraft = useChapterDraftStore((s) => s.create);

  const [styleSample, setStyleSample] = useState("");
  const [pov, setPov] = useState<DraftPrefs["pov"]>("第三人称");
  const [targetWords, setTargetWords] = useState(String(TARGET_DEFAULT));
  const [instruction, setInstruction] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setStyleSample("");
    setPov("第三人称");
    setTargetWords(String(TARGET_DEFAULT));
    setInstruction("");
    setError(null);
  }, [open, chapterId]);

  const submit = async () => {
    const target = Number(targetWords);
    if (!Number.isInteger(target) || target < TARGET_MIN || target > TARGET_MAX) {
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

    const prefs: DraftPrefs = {
      style_name: null,
      style_sample: styleSample.trim() || null,
      instruction: instruction.trim() || null,
      target_words: target,
      pov,
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
            data-autofocus
          />
          <div className="mt-1 flex justify-end font-mono text-[10px] text-faint">
            {styleSample.length.toLocaleString("zh-CN")} / {STYLE_SAMPLE_LIMIT.toLocaleString("zh-CN")}
          </div>
        </Field>

        <Field label={zh.draft.pov} hint={zh.draft.povHint}>
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
            hint={zh.draft.targetWordsRange(TARGET_MIN, TARGET_MAX)}
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
