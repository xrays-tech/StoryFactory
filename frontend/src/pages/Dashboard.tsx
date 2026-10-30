import { useEffect, useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router-dom";
import {
  ArrowUpRight,
  BookOpenText,
  ChevronLeft,
  ChevronRight,
  Feather,
  Plus,
  Search,
  Sparkles,
  Trash2,
} from "lucide-react";
import { useNovelStore } from "../stores/novelStore";
import { useUiStore } from "../stores/uiStore";
import { zh } from "../copy/zh";
import { formatDateTime, truncate } from "../lib/format";
import { userMessage } from "../lib/errors";
import { Button } from "../components/common/Button";
import { Input, Select, Textarea } from "../components/common/Input";
import { Field } from "../components/common/Field";
import { Dialog } from "../components/common/Dialog";
import { Badge } from "../components/common/Badge";
import { EmptyState, ErrorState } from "../components/common/EmptyState";
import { SkeletonCard } from "../components/common/Skeleton";
import { PageHeader } from "../components/common/PageHeader";
import { novelStatusLabel } from "../copy/zh";

export function Dashboard() {
  const navigate = useNavigate();
  const toast = useUiStore((s) => s.toast);
  const items = useNovelStore((s) => s.items);
  const total = useNovelStore((s) => s.total);
  const page = useNovelStore((s) => s.page);
  const pageSize = useNovelStore((s) => s.pageSize);
  const query = useNovelStore((s) => s.query);
  const loading = useNovelStore((s) => s.loading);
  const error = useNovelStore((s) => s.error);
  const load = useNovelStore((s) => s.load);
  const create = useNovelStore((s) => s.create);

  const [search, setSearch] = useState(query.q);
  const [status, setStatus] = useState(query.status);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [form, setForm] = useState({ title: "", background: "", worldView: "", summary: "" });
  const [formError, setFormError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    const timer = window.setTimeout(() => {
      const state = useNovelStore.getState();
      if (state.query.q !== search || state.query.status !== status) {
        void load({ q: search, status, page: 1 });
      }
    }, 280);
    return () => window.clearTimeout(timer);
  }, [search, status, load]);

  const openCreate = () => {
    setForm({ title: "", background: "", worldView: "", summary: "" });
    setFormError(null);
    setFieldErrors({});
    setDialogOpen(true);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!form.title.trim()) {
      setFieldErrors({ title: "请给项目起一个标题" });
      return;
    }
    setSaving(true);
    setFormError(null);
    setFieldErrors({});
    try {
      const novel = await create({
        title: form.title.trim(),
        background: form.background.trim() || null,
        world_view: form.worldView.trim() || null,
        summary: form.summary.trim() || null,
      });
      setDialogOpen(false);
      toast({ tone: "success", message: "项目已创建" });
      navigate(`/novel/${novel.id}`);
    } catch (err) {
      setFormError(userMessage(err));
      const details = (err as { fieldErrors?: Record<string, string> }).fieldErrors ?? {};
      setFieldErrors(details);
    } finally {
      setSaving(false);
    }
  };

  const pageCount = Math.max(1, Math.ceil(total / pageSize));

  return (
    <div className="space-y-7">
      <section className="relative overflow-hidden rounded-2xl border border-line/70 bg-surface/70 px-5 py-7 shadow-card sm:px-7 lg:px-9 lg:py-9">
        <div
          className="pointer-events-none absolute -right-20 -top-24 h-64 w-64 rounded-full bg-accent/10 blur-3xl"
          aria-hidden
        />
        <div
          className="pointer-events-none absolute -bottom-24 left-1/3 h-48 w-48 rounded-full bg-info/5 blur-3xl"
          aria-hidden
        />
        <div className="relative flex flex-col gap-6 xl:flex-row xl:items-end xl:justify-between">
          <div className="max-w-2xl">
            <p className="eyebrow flex items-center gap-2">
              <Sparkles className="h-3.5 w-3.5 text-accent" />
              长篇小说工作台
            </p>
            <h1 className="mt-3 font-display text-3xl font-semibold tracking-tight text-ink text-balance sm:text-4xl">
              {zh.dashboard.title}
            </h1>
            <p className="mt-3 max-w-xl text-sm leading-6 text-muted text-pretty">
              {zh.dashboard.subtitle}
            </p>
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-2">
            <Button variant="primary" size="lg" leftIcon={<Plus className="h-4 w-4" />} onClick={openCreate}>
              {zh.dashboard.newNovel}
            </Button>
            <Button
              variant="outline"
              size="lg"
              leftIcon={<Trash2 className="h-4 w-4" />}
              onClick={() => navigate("/recycle")}
            >
              {zh.dashboard.recycle}
            </Button>
          </div>
        </div>
      </section>

      <PageHeader
        eyebrow={zh.dashboard.count(total)}
        title="项目书架"
        description="每个项目是一册还在生长的书。打开它，继续写角色、卷章与推演。"
        actions={
          <div className="flex w-full flex-col gap-2 sm:w-auto sm:flex-row">
            <div className="relative min-w-0 sm:w-64">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-faint" />
              <Input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={zh.dashboard.searchPlaceholder}
                aria-label="搜索项目"
                className="pl-9"
              />
            </div>
            <Select
              value={status}
              onChange={(e) => setStatus(e.target.value as typeof status)}
              aria-label="筛选项目状态"
              className="sm:w-32"
            >
              <option value="">{zh.common.all}</option>
              <option value="active">{novelStatusLabel.active}</option>
              <option value="archived">{novelStatusLabel.archived}</option>
            </Select>
          </div>
        }
      />

      {loading && items.length === 0 ? (
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {Array.from({ length: 6 }).map((_, i) => (
            <SkeletonCard key={i} />
          ))}
        </div>
      ) : error && items.length === 0 ? (
        <ErrorState
          message={error}
          onRetry={() => void load({ q: search, status, page: 1 })}
        />
      ) : items.length === 0 ? (
        <EmptyState
          icon={<Feather className="h-5 w-5" />}
          title={query.q || status ? zh.dashboard.noResults : zh.dashboard.emptyTitle}
          description={query.q || status ? zh.dashboard.noResultsHint : zh.dashboard.emptyHint}
          action={
            <Button variant="primary" leftIcon={<Plus className="h-4 w-4" />} onClick={openCreate}>
              {zh.dashboard.newNovel}
            </Button>
          }
        />
      ) : (
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {items.map((novel, index) => (
            <Link
              key={novel.id}
              to={`/novel/${novel.id}`}
              className="group relative flex min-h-[208px] flex-col overflow-hidden rounded-2xl border border-line/80 bg-surface/80 p-5 shadow-card outline-none transition-all duration-200 hover:-translate-y-0.5 hover:border-accent/35 hover:bg-raised hover:shadow-float focus-visible:ring-2 focus-visible:ring-accent/60"
            >
              <span className="absolute inset-y-5 left-0 w-0.5 rounded-full bg-accent/0 transition-all duration-200 group-hover:bg-accent/60" />
              <div className="flex items-center justify-between gap-2">
                <span className="font-mono text-[11px] tracking-widest text-faint">
                  {String(index + 1 + (page - 1) * pageSize).padStart(2, "0")}
                </span>
                <Badge tone={novel.status === "active" ? "accent" : "muted"}>
                  {novelStatusLabel[novel.status]}
                </Badge>
              </div>
              <h2 className="mt-4 font-display text-xl font-semibold leading-snug tracking-tight text-ink transition-colors group-hover:text-accent-strong">
                {novel.title}
              </h2>
              <p className="mt-2 line-clamp-3 flex-1 text-sm leading-6 text-muted">
                {truncate(novel.summary || novel.world_view || novel.background || "还没有摘要，打开项目继续补完这个世界。", 140)}
              </p>
              <div className="mt-4 flex items-center justify-between border-t border-line/60 pt-3 text-[11px] text-faint">
                <span>更新于 {formatDateTime(novel.updated_at)}</span>
                <span className="inline-flex items-center gap-1 text-accent-strong opacity-0 transition-opacity group-hover:opacity-100">
                  打开 <ArrowUpRight className="h-3.5 w-3.5" />
                </span>
              </div>
            </Link>
          ))}
        </div>
      )}

      {pageCount > 1 ? (
        <div className="flex items-center justify-between gap-3">
          <p className="text-xs text-faint">
            第 {page} / {pageCount} 页
          </p>
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              disabled={page <= 1 || loading}
              leftIcon={<ChevronLeft className="h-3.5 w-3.5" />}
              onClick={() => void load({ page: page - 1 })}
            >
              上一页
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={page >= pageCount || loading}
              onClick={() => void load({ page: page + 1 })}
            >
              下一页 <ChevronRight className="ml-1 h-3.5 w-3.5" />
            </Button>
          </div>
        </div>
      ) : null}

      <Dialog
        open={dialogOpen}
        onClose={() => (saving ? undefined : setDialogOpen(false))}
        title={zh.dashboard.newNovel}
        description="先给它一个名字。背景、世界观和摘要都可以之后慢慢补。"
        size="lg"
        footer={
          <>
            <Button variant="ghost" onClick={() => setDialogOpen(false)} disabled={saving}>
              {zh.common.cancel}
            </Button>
            <Button variant="primary" onClick={submit} loading={saving}>
              {zh.common.create}
            </Button>
          </>
        }
      >
        <form onSubmit={submit} className="space-y-4">
          <Field label={zh.novel.title} htmlFor="novel-title" required error={fieldErrors.title}>
            <Input
              id="novel-title"
              value={form.title}
              onChange={(e) => setForm({ ...form, title: e.target.value })}
              placeholder="例如：潮汐图书馆"
              invalid={!!fieldErrors.title}
              autoFocus
              maxLength={200}
            />
          </Field>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label={zh.novel.worldView} htmlFor="novel-world" error={fieldErrors.world_view}>
              <Textarea
                id="novel-world"
                value={form.worldView}
                onChange={(e) => setForm({ ...form, worldView: e.target.value })}
                placeholder="这个世界的规则、时代与氛围…"
                rows={4}
                invalid={!!fieldErrors.world_view}
              />
            </Field>
            <Field label={zh.novel.background} htmlFor="novel-background" error={fieldErrors.background}>
              <Textarea
                id="novel-background"
                value={form.background}
                onChange={(e) => setForm({ ...form, background: e.target.value })}
                placeholder="故事开始前发生过什么…"
                rows={4}
                invalid={!!fieldErrors.background}
              />
            </Field>
          </div>
          <Field label={zh.novel.summary} htmlFor="novel-summary" error={fieldErrors.summary}>
            <Textarea
              id="novel-summary"
              value={form.summary}
              onChange={(e) => setForm({ ...form, summary: e.target.value })}
              placeholder="用几句话抓住这本书的核…"
              rows={3}
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
