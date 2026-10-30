import { useState, type FormEvent } from "react";
import type { LoaderFunctionArgs } from "react-router-dom";
import { useParams } from "react-router-dom";
import { Pencil, Plus, Quote, Target, Trash2, UserRound, Users } from "lucide-react";
import type { RoleType } from "../../types/domain";
import { roleLabel, zh } from "../../copy/zh";
import { useCharacterStore, type Character } from "../../stores/characterStore";
import { useUiStore } from "../../stores/uiStore";
import { userMessage } from "../../lib/errors";
import { truncate } from "../../lib/format";
import { PageHeader } from "../../components/common/PageHeader";
import { Button } from "../../components/common/Button";
import { Dialog } from "../../components/common/Dialog";
import { Field } from "../../components/common/Field";
import { Input, Select, Textarea } from "../../components/common/Input";
import { EmptyState, ErrorState } from "../../components/common/EmptyState";
import { LoadingBlock } from "../../components/common/Skeleton";
import { RoleBadge } from "../../components/common/StatusBadge";
import { cn } from "../../lib/cn";

export async function charactersLoader({ params }: LoaderFunctionArgs) {
  if (params.novelId) await useCharacterStore.getState().load(params.novelId);
  return null;
}

type CharacterForm = {
  name: string;
  roleType: RoleType;
  appearance: string;
  personality: string;
  hiddenPersonality: string;
  biography: string;
  currentGoal: string;
  speakingStyle: string;
};

const emptyForm: CharacterForm = {
  name: "",
  roleType: "support",
  appearance: "",
  personality: "",
  hiddenPersonality: "",
  biography: "",
  currentGoal: "",
  speakingStyle: "",
};

const roleOptions: RoleType[] = ["protagonist", "deuteragonist", "antagonist", "support"];

function accentForRole(role: RoleType): string {
  switch (role) {
    case "protagonist":
      return "bg-accent";
    case "deuteragonist":
      return "bg-info";
    case "antagonist":
      return "bg-danger";
    default:
      return "bg-line";
  }
}

export function CharactersPage() {
  const { novelId } = useParams<{ novelId: string }>();
  const items = useCharacterStore((s) => s.items);
  const loading = useCharacterStore((s) => s.loading);
  const error = useCharacterStore((s) => s.error);
  const load = useCharacterStore((s) => s.load);
  const create = useCharacterStore((s) => s.create);
  const update = useCharacterStore((s) => s.update);
  const remove = useCharacterStore((s) => s.remove);
  const toast = useUiStore((s) => s.toast);
  const askConfirm = useUiStore((s) => s.askConfirm);

  const [dialogOpen, setDialogOpen] = useState(false);
  const [editing, setEditing] = useState<Character | null>(null);
  const [form, setForm] = useState<CharacterForm>(emptyForm);
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  if (!novelId) return null;

  const openCreate = () => {
    setEditing(null);
    setForm(emptyForm);
    setFormError(null);
    setFieldErrors({});
    setDialogOpen(true);
  };

  const openEdit = (character: Character) => {
    setEditing(character);
    setForm({
      name: character.name,
      roleType: character.role_type,
      appearance: character.appearance ?? "",
      personality: character.personality ?? "",
      hiddenPersonality: character.hidden_personality ?? "",
      biography: character.biography ?? "",
      currentGoal: character.current_goal ?? "",
      speakingStyle: character.speaking_style ?? "",
    });
    setFormError(null);
    setFieldErrors({});
    setDialogOpen(true);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!form.name.trim()) {
      setFieldErrors({ name: "姓名不能为空" });
      return;
    }
    setSaving(true);
    setFormError(null);
    setFieldErrors({});
    const payload = {
      name: form.name.trim(),
      role_type: form.roleType,
      appearance: form.appearance.trim() || null,
      personality: form.personality.trim() || null,
      hidden_personality: form.hiddenPersonality.trim() || null,
      biography: form.biography.trim() || null,
      current_goal: form.currentGoal.trim() || null,
      speaking_style: form.speakingStyle.trim() || null,
    };
    try {
      if (editing) {
        await update(novelId, editing.id, payload);
        toast({ tone: "success", message: "角色已更新" });
      } else {
        await create(novelId, payload);
        toast({ tone: "success", message: "角色已创建" });
      }
      setDialogOpen(false);
    } catch (err) {
      setFormError(userMessage(err));
      setFieldErrors((err as { fieldErrors?: Record<string, string> }).fieldErrors ?? {});
    } finally {
      setSaving(false);
    }
  };

  const deleteCharacter = async (character: Character) => {
    const ok = await askConfirm({
      title: "删除角色？",
      message: zh.character.deleteConfirm,
      confirmLabel: zh.common.delete,
      tone: "danger",
    });
    if (!ok) return;
    try {
      await remove(novelId, character.id);
      toast({ tone: "success", message: "角色已移入回收站" });
    } catch (err) {
      toast({ tone: "error", message: userMessage(err) });
    }
  };

  return (
    <div className="space-y-5">
      <PageHeader
        title={zh.character.title}
        description={zh.character.subtitle}
        actions={
          <Button variant="primary" leftIcon={<Plus className="h-4 w-4" />} onClick={openCreate}>
            {zh.character.newCharacter}
          </Button>
        }
      />

      {loading && items.length === 0 ? (
        <LoadingBlock label="正在读取角色…" />
      ) : error && items.length === 0 ? (
        <ErrorState message={error} onRetry={() => void load(novelId)} />
      ) : items.length === 0 ? (
        <EmptyState
          icon={<Users className="h-5 w-5" />}
          title={zh.character.empty}
          description={zh.character.emptyHint}
          action={
            <Button variant="primary" leftIcon={<Plus className="h-4 w-4" />} onClick={openCreate}>
              {zh.character.newCharacter}
            </Button>
          }
        />
      ) : (
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {items.map((character) => (
            <article
              key={character.id}
              className="group relative flex min-h-[190px] flex-col overflow-hidden rounded-2xl border border-line/80 bg-surface/80 p-4 shadow-card transition-all duration-200 hover:-translate-y-0.5 hover:border-accent/30 hover:bg-raised hover:shadow-float"
            >
              <span className={cn("absolute inset-y-4 left-0 w-0.5 rounded-full opacity-70", accentForRole(character.role_type))} />
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <h2 className="truncate font-display text-lg font-semibold text-ink">{character.name}</h2>
                  <div className="mt-1">
                    <RoleBadge role={character.role_type} />
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-0.5 opacity-100 transition-opacity sm:opacity-0 sm:group-hover:opacity-100 sm:focus-within:opacity-100">
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7"
                    onClick={() => openEdit(character)}
                    aria-label={`编辑 ${character.name}`}
                  >
                    <Pencil className="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 hover:text-danger"
                    onClick={() => void deleteCharacter(character)}
                    aria-label={`删除 ${character.name}`}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>

              <div className="mt-3 space-y-2 text-sm">
                {character.current_goal ? (
                  <p className="flex items-start gap-1.5 leading-6 text-ink/85">
                    <Target className="mt-1 h-3.5 w-3.5 shrink-0 text-accent" />
                    <span>{truncate(character.current_goal, 80)}</span>
                  </p>
                ) : null}
                {character.personality ? (
                  <p className="flex items-start gap-1.5 leading-6 text-muted">
                    <Quote className="mt-1 h-3.5 w-3.5 shrink-0 text-faint" />
                    <span>{truncate(character.personality, 90)}</span>
                  </p>
                ) : null}
                {!character.current_goal && !character.personality ? (
                  <p className="flex items-center gap-1.5 text-xs text-faint">
                    <UserRound className="h-3.5 w-3.5" /> 还没有填写性格与目标
                  </p>
                ) : null}
              </div>

              <button
                type="button"
                onClick={() => openEdit(character)}
                className="mt-auto pt-3 text-left text-[11px] text-faint transition-colors hover:text-accent-strong"
              >
                打开人物卡 →
              </button>
            </article>
          ))}
        </div>
      )}

      <Dialog
        open={dialogOpen}
        onClose={() => (saving ? undefined : setDialogOpen(false))}
        title={editing ? `编辑角色 · ${editing.name}` : zh.character.newCharacter}
        description="人物越具体，沙盒里的行动就越像他自己。"
        size="xl"
        footer={
          <>
            <Button variant="ghost" onClick={() => setDialogOpen(false)} disabled={saving}>
              {zh.common.cancel}
            </Button>
            <Button variant="primary" onClick={submit} loading={saving}>
              {zh.common.save}
            </Button>
          </>
        }
      >
        <form onSubmit={submit} className="space-y-4">
          <div className="grid gap-4 sm:grid-cols-[1fr_180px]">
            <Field label={zh.character.name} htmlFor="char-name" required error={fieldErrors.name}>
              <Input
                id="char-name"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                invalid={!!fieldErrors.name}
                autoFocus
                maxLength={50}
              />
            </Field>
            <Field label={zh.character.role} htmlFor="char-role">
              <Select
                id="char-role"
                value={form.roleType}
                onChange={(e) => setForm({ ...form, roleType: e.target.value as RoleType })}
              >
                {roleOptions.map((role) => (
                  <option key={role} value={role}>
                    {roleLabel[role]}
                  </option>
                ))}
              </Select>
            </Field>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label={zh.character.personality} htmlFor="char-personality" error={fieldErrors.personality}>
              <Textarea
                id="char-personality"
                rows={3}
                value={form.personality}
                onChange={(e) => setForm({ ...form, personality: e.target.value })}
                placeholder="外在呈现出来的性格…"
                invalid={!!fieldErrors.personality}
              />
            </Field>
            <Field label={zh.character.hiddenPersonality} htmlFor="char-hidden" error={fieldErrors.hidden_personality}>
              <Textarea
                id="char-hidden"
                rows={3}
                value={form.hiddenPersonality}
                onChange={(e) => setForm({ ...form, hiddenPersonality: e.target.value })}
                placeholder="只有读者或导演知道的另一面…"
                invalid={!!fieldErrors.hidden_personality}
              />
            </Field>
            <Field label={zh.character.currentGoal} htmlFor="char-goal" error={fieldErrors.current_goal}>
              <Textarea
                id="char-goal"
                rows={3}
                value={form.currentGoal}
                onChange={(e) => setForm({ ...form, currentGoal: e.target.value })}
                placeholder="此刻他最想得到什么？"
                invalid={!!fieldErrors.current_goal}
              />
            </Field>
            <Field label={zh.character.speakingStyle} htmlFor="char-speaking" error={fieldErrors.speaking_style}>
              <Textarea
                id="char-speaking"
                rows={3}
                value={form.speakingStyle}
                onChange={(e) => setForm({ ...form, speakingStyle: e.target.value })}
                placeholder="句子长短、用词习惯、口头禅…"
                invalid={!!fieldErrors.speaking_style}
              />
            </Field>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label={zh.character.appearance} htmlFor="char-appearance" error={fieldErrors.appearance}>
              <Textarea
                id="char-appearance"
                rows={4}
                value={form.appearance}
                onChange={(e) => setForm({ ...form, appearance: e.target.value })}
                placeholder="外貌、衣着、姿态…"
                invalid={!!fieldErrors.appearance}
              />
            </Field>
            <Field label={zh.character.biography} htmlFor="char-bio" error={fieldErrors.biography}>
              <Textarea
                id="char-bio"
                rows={4}
                value={form.biography}
                onChange={(e) => setForm({ ...form, biography: e.target.value })}
                placeholder="他走到今天之前，经历过什么？"
                invalid={!!fieldErrors.biography}
              />
            </Field>
          </div>
          {formError ? (
            <p className="rounded-lg border border-danger/25 bg-danger-soft/50 px-3 py-2 text-sm text-danger" role="alert">
              {formError}
            </p>
          ) : null}
        </form>
      </Dialog>
    </div>
  );
}
