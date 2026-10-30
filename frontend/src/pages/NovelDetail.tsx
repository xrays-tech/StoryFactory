import { useState, type FormEvent } from "react";
import { Link, Outlet, useNavigate, useParams } from "react-router-dom";
import {
  ArrowLeft,
  BookOpenText,
  Boxes,
  HeartHandshake,
  ListTree,
  Package,
  Pencil,
  Trash2,
  Users,
} from "lucide-react";
import { useNovelStore } from "../stores/novelStore";
import { useUiStore } from "../stores/uiStore";
import { zh, novelStatusLabel } from "../copy/zh";
import { formatDateTime, truncate } from "../lib/format";
import { userMessage } from "../lib/errors";
import { Badge } from "../components/common/Badge";
import { Button } from "../components/common/Button";
import { Dialog } from "../components/common/Dialog";
import { Field } from "../components/common/Field";
import { Input, Select, Textarea } from "../components/common/Input";
import { EmptyState, ErrorState } from "../components/common/EmptyState";
import { LoadingBlock } from "../components/common/Skeleton";
import { NavTabs } from "../components/common/Tabs";

export function NovelDetail() {
  const { novelId } = useParams<{ novelId: string }>();
  const navigate = useNavigate();
  const toast = useUiStore((s) => s.toast);
  const askConfirm = useUiStore((s) => s.askConfirm);
  const current = useNovelStore((s) => s.current);
  const currentId = useNovelStore((s) => s.currentId);
  const currentLoading = useNovelStore((s) => s.currentLoading);
  const currentError = useNovelStore((s) => s.currentError);
  const loadOne = useNovelStore((s) => s.loadOne);
  const update = useNovelStore((s) => s.update);
  const remove = useNovelStore((s) => s.remove);

  const [editOpen, setEditOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [form, setForm] = useState({
    title: "",
    status: "active" as "active" | "archived",
    worldView: "",
    background: "",
    summary: "",
  });

  const novel = current && current.id === novelId ? current : null;

  if (!novelId) return null;

  if (currentLoading && (!novel || currentId !== novelId)) {
    return <LoadingBlock label="正在打开项目…" />;
  }

  if (currentError && !novel) {
    return (
      <ErrorState
        title="项目打不开"
        message={currentError}
        onRetry={() => void loadOne(novelId)}
      />
    );
  }

  if (!novel) {
    return (
      <EmptyState
        icon={<BookOpenText className="h-5 w-5" />}
        title="项目不存在"
        description="它可能已经被删除，或在另一个项目里。"
        action={
          <Button variant="outline" onClick={() => navigate("/")}>
            {zh.nav.backHome}
          </Button>
        }
      />
    );
  }

  const base = `/novel/${novel.id}`;

  const openEdit = () => {
    setForm({
      title: novel.title,
      status: novel.status,
      worldView: novel.world_view ?? "",
      background: novel.background ?? "",
      summary: novel.summary ?? "",
    });
    setFormError(null);
    setFieldErrors({});
    setEditOpen(true);
  };

  const submitEdit = async (event: FormEvent) => {
    event.preventDefault();
    if (!form.title.trim()) {
      setFieldErrors({ title: "标题不能为空" });
      return;
    }
    setSaving(true);
    setFormError(null);
    setFieldErrors({});
    try {
      await update(novel.id, {
        title: form.title.trim(),
        status: form.status,
        world_view: form.worldView.trim() || null,
        background: form.background.trim() || null,
        summary: form.summary.trim() || null,
      });
      setEditOpen(false);
      toast({ tone: "success", message: "项目信息已更新" });
    } catch (error) {
      setFormError(userMessage(error));
      setFieldErrors((error as { fieldErrors?: Record<string, string> }).fieldErrors ?? {});
    } finally {
      setSaving(false);
    }
  };

  const removeNovel = async () => {
    const ok = await askConfirm({
      title: "移入回收站？",
      message: zh.novel.deleteConfirm,
      confirmLabel: zh.common.delete,
      tone: "danger",
    });
    if (!ok) return;
    try {
      await remove(novel.id);
      toast({ tone: "success", message: "项目已移入回收站" });
      navigate("/");
    } catch (error) {
      toast({ tone: "error", message: userMessage(error) });
    }
  };

  return (
    <div className="space-y-0">
      <div className="pb-5">
        <Link
          to="/"
          className="focus-ring inline-flex items-center gap-1.5 rounded text-xs text-muted transition-colors hover:text-ink"
        >
          <ArrowLeft className="h-3.5 w-3.5" /> 返回项目
        </Link>

        <div className="mt-3 flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <Badge tone={novel.status === "active" ? "accent" : "muted"}>
                {novelStatusLabel[novel.status]}
              </Badge>
              <span className="text-[11px] text-faint">
                更新于 {formatDateTime(novel.updated_at)} · 创建于 {formatDateTime(novel.created_at)}
              </span>
            </div>
            <h1 className="mt-2 font-display text-3xl font-semibold tracking-tight text-ink text-balance sm:text-4xl">
              {novel.title}
            </h1>
            <p className="mt-2 max-w-3xl text-sm leading-6 text-muted text-pretty">
              {truncate(novel.summary || novel.world_view || novel.background || "还没有摘要。可以在编辑项目信息里写下这本书的核。", 220)}
            </p>
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-2">
            <Button variant="outline" leftIcon={<Pencil className="h-4 w-4" />} onClick={openEdit}>
              {zh.novel.editInfo}
            </Button>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => void removeNovel()}
              title={zh.novel.deleteNovel}
              aria-label={zh.novel.deleteNovel}
              className="text-faint hover:text-danger"
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </div>

      <div className="sticky top-14 z-30 -mx-4 bg-paper/90 px-4 backdrop-blur-md sm:-mx-6 sm:px-6 lg:-mx-8 lg:px-8">
        <NavTabs
          items={[
            { to: base, label: zh.novel.overview, end: true, icon: <BookOpenText className="h-4 w-4" /> },
            { to: `${base}/characters`, label: zh.novel.characters, icon: <Users className="h-4 w-4" /> },
            { to: `${base}/relations`, label: zh.novel.relations, icon: <HeartHandshake className="h-4 w-4" /> },
            { to: `${base}/outline`, label: zh.novel.outline, icon: <ListTree className="h-4 w-4" /> },
            { to: `${base}/items`, label: zh.novel.items, icon: <Package className="h-4 w-4" /> },
            { to: `${base}/sandbox`, label: zh.novel.sandbox, icon: <Boxes className="h-4 w-4" /> },
          ]}
        />
      </div>

      <div className="pt-6">
        <Outlet />
      </div>

      <Dialog
        open={editOpen}
        onClose={() => (saving ? undefined : setEditOpen(false))}
        title={zh.novel.editInfo}
        size="lg"
        footer={
          <>
            <Button variant="ghost" onClick={() => setEditOpen(false)} disabled={saving}>
              {zh.common.cancel}
            </Button>
            <Button variant="primary" onClick={submitEdit} loading={saving}>
              {zh.common.save}
            </Button>
          </>
        }
      >
        <form onSubmit={submitEdit} className="space-y-4">
          <div className="grid gap-4 sm:grid-cols-[1fr_180px]">
            <Field label={zh.novel.title} htmlFor="edit-title" required error={fieldErrors.title}>
              <Input
                id="edit-title"
                value={form.title}
                onChange={(e) => setForm({ ...form, title: e.target.value })}
                invalid={!!fieldErrors.title}
                autoFocus
                maxLength={200}
              />
            </Field>
            <Field label={zh.novel.status} htmlFor="edit-status">
              <Select
                id="edit-status"
                value={form.status}
                onChange={(e) => setForm({ ...form, status: e.target.value as "active" | "archived" })}
              >
                <option value="active">{novelStatusLabel.active}</option>
                <option value="archived">{novelStatusLabel.archived}</option>
              </Select>
            </Field>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label={zh.novel.worldView} htmlFor="edit-world" error={fieldErrors.world_view}>
              <Textarea
                id="edit-world"
                rows={5}
                value={form.worldView}
                onChange={(e) => setForm({ ...form, worldView: e.target.value })}
                invalid={!!fieldErrors.world_view}
              />
            </Field>
            <Field label={zh.novel.background} htmlFor="edit-background" error={fieldErrors.background}>
              <Textarea
                id="edit-background"
                rows={5}
                value={form.background}
                onChange={(e) => setForm({ ...form, background: e.target.value })}
                invalid={!!fieldErrors.background}
              />
            </Field>
          </div>
          <Field label={zh.novel.summary} htmlFor="edit-summary" error={fieldErrors.summary}>
            <Textarea
              id="edit-summary"
              rows={4}
              value={form.summary}
              onChange={(e) => setForm({ ...form, summary: e.target.value })}
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
    </div>
  );
}
