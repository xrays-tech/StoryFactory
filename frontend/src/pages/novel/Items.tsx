import { useEffect, useMemo, useState, type FormEvent } from "react";
import type { LoaderFunctionArgs } from "react-router-dom";
import { useLoaderData, useParams, useRevalidator } from "react-router-dom";
import { Package, Pencil, Plus, Trash2 } from "lucide-react";
import type { ItemDto } from "../../types/domain";
import type { Wire } from "../../types/api";
import * as api from "../../api/endpoints";
import { useCharacterStore } from "../../stores/characterStore";
import { useUiStore } from "../../stores/uiStore";
import { zh } from "../../copy/zh";
import { userMessage } from "../../lib/errors";
import { formatDateTime, truncate } from "../../lib/format";
import { PageHeader } from "../../components/common/PageHeader";
import { Button } from "../../components/common/Button";
import { Dialog } from "../../components/common/Dialog";
import { Field } from "../../components/common/Field";
import { Input, Select, Textarea } from "../../components/common/Input";
import { EmptyState, ErrorState } from "../../components/common/EmptyState";
import { Table, TBody, TD, TH, THead, TR } from "../../components/common/Table";
import { Badge } from "../../components/common/Badge";
import { cn } from "../../lib/cn";

type Item = Wire<ItemDto>;

export type ItemsLoaderData = { items: Item[]; error?: string };

export async function itemsLoader({ params }: LoaderFunctionArgs): Promise<ItemsLoaderData> {
  if (!params.novelId) return { items: [] };
  try {
    await useCharacterStore.getState().load(params.novelId);
    const page = await api.listItems(params.novelId, { page: 1, pageSize: 100 });
    return { items: page.items };
  } catch (error) {
    return { items: [], error: userMessage(error) };
  }
}

type ItemForm = {
  name: string;
  features: string;
  description: string;
  ownerId: string;
  importance: number;
};

const emptyForm: ItemForm = {
  name: "",
  features: "",
  description: "",
  ownerId: "",
  importance: 0,
};

function importanceTone(value: number): "neutral" | "info" | "warning" | "accent" {
  if (value >= 4) return "accent";
  if (value >= 3) return "warning";
  if (value >= 1) return "info";
  return "neutral";
}

export function ItemsPage() {
  const { novelId } = useParams<{ novelId: string }>();
  const data = useLoaderData() as ItemsLoaderData;
  const revalidator = useRevalidator();
  const characters = useCharacterStore((s) => s.items);
  const toast = useUiStore((s) => s.toast);
  const askConfirm = useUiStore((s) => s.askConfirm);

  const [items, setItems] = useState<Item[]>(data.items);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editing, setEditing] = useState<Item | null>(null);
  const [form, setForm] = useState<ItemForm>(emptyForm);
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    setItems(data.items);
  }, [data.items]);

  const owners = useMemo(() => {
    const map = new Map<string, string>();
    for (const character of characters) map.set(character.id, character.name);
    return map;
  }, [characters]);

  if (!novelId) return null;

  const openCreate = () => {
    setEditing(null);
    setForm(emptyForm);
    setFormError(null);
    setFieldErrors({});
    setDialogOpen(true);
  };

  const openEdit = (item: Item) => {
    setEditing(item);
    setForm({
      name: item.name,
      features: item.features ?? "",
      description: item.description ?? "",
      ownerId: item.owner_id ?? "",
      importance: item.importance,
    });
    setFormError(null);
    setFieldErrors({});
    setDialogOpen(true);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!form.name.trim()) {
      setFieldErrors({ name: "名称不能为空" });
      return;
    }
    setSaving(true);
    setFormError(null);
    setFieldErrors({});
    try {
      if (editing) {
        const updated = await api.updateItem(novelId, editing.id, {
          name: form.name.trim(),
          features: form.features.trim() || null,
          description: form.description.trim() || null,
          owner_id: form.ownerId || null,
          importance: form.importance,
        });
        setItems((list) => list.map((item) => (item.id === updated.id ? updated : item)));
        toast({ tone: "success", message: "物品已更新" });
      } else {
        const created = await api.createItem(novelId, {
          name: form.name.trim(),
          features: form.features.trim() || null,
          description: form.description.trim() || null,
          owner_id: form.ownerId || null,
          importance: form.importance,
        });
        setItems((list) => [created, ...list]);
        toast({ tone: "success", message: "物品已创建" });
      }
      setDialogOpen(false);
    } catch (error) {
      setFormError(userMessage(error));
      setFieldErrors((error as { fieldErrors?: Record<string, string> }).fieldErrors ?? {});
    } finally {
      setSaving(false);
    }
  };

  const remove = async (item: Item) => {
    const ok = await askConfirm({
      title: "删除物品？",
      message: zh.item.deleteConfirm,
      confirmLabel: zh.common.delete,
      tone: "danger",
    });
    if (!ok) return;
    try {
      await api.deleteItem(novelId, item.id);
      setItems((list) => list.filter((row) => row.id !== item.id));
      toast({ tone: "success", message: "物品已移入回收站" });
    } catch (error) {
      toast({ tone: "error", message: userMessage(error) });
    }
  };

  return (
    <div className="space-y-5">
      <PageHeader
        title={zh.item.title}
        description={zh.item.subtitle}
        actions={
          <Button variant="primary" leftIcon={<Plus className="h-4 w-4" />} onClick={openCreate}>
            {zh.item.newItem}
          </Button>
        }
      />

      {data.error && items.length === 0 ? (
        <ErrorState
          message={data.error}
          onRetry={() => revalidator.revalidate()}
        />
      ) : items.length === 0 ? (
        <EmptyState
          icon={<Package className="h-5 w-5" />}
          title={zh.item.empty}
          description={zh.item.emptyHint}
          action={
            <Button variant="primary" leftIcon={<Plus className="h-4 w-4" />} onClick={openCreate}>
              {zh.item.newItem}
            </Button>
          }
        />
      ) : (
        <Table>
          <THead>
            <tr>
              <TH>名称</TH>
              <TH>特征</TH>
              <TH>持有者</TH>
              <TH className="w-24">{zh.item.importance}</TH>
              <TH>更新</TH>
              <TH className="w-20 text-right">操作</TH>
            </tr>
          </THead>
          <TBody>
            {items.map((item) => (
              <TR key={item.id}>
                <TD>
                  <p className="font-medium text-ink">{item.name}</p>
                  {item.description ? (
                    <p className="mt-0.5 max-w-[360px] text-[11px] text-faint">{truncate(item.description, 100)}</p>
                  ) : null}
                </TD>
                <TD className="max-w-[260px] text-xs text-muted">{truncate(item.features || "—", 80)}</TD>
                <TD className="whitespace-nowrap text-xs text-muted">
                  {item.owner_id ? owners.get(item.owner_id) ?? "未知角色" : "—"}
                </TD>
                <TD>
                  <div className="flex items-center gap-1.5">
                    <Badge tone={importanceTone(item.importance)}>{item.importance}</Badge>
                    <span className="flex gap-0.5" aria-hidden>
                      {Array.from({ length: 5 }).map((_, index) => (
                        <span
                          key={index}
                          className={cn(
                            "h-1.5 w-1.5 rounded-full",
                            index < item.importance ? "bg-accent/70" : "bg-line",
                          )}
                        />
                      ))}
                    </span>
                  </div>
                </TD>
                <TD className="whitespace-nowrap text-[11px] text-faint">{formatDateTime(item.updated_at)}</TD>
                <TD>
                  <div className="flex items-center justify-end gap-0.5">
                    <Button variant="ghost" size="icon" className="h-7 w-7" onClick={() => openEdit(item)} aria-label={`编辑 ${item.name}`}>
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 hover:text-danger"
                      onClick={() => void remove(item)}
                      aria-label={`删除 ${item.name}`}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </TD>
              </TR>
            ))}
          </TBody>
        </Table>
      )}

      <Dialog
        open={dialogOpen}
        onClose={() => (saving ? undefined : setDialogOpen(false))}
        title={editing ? `编辑物品 · ${editing.name}` : zh.item.newItem}
        size="lg"
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
            <Field label={zh.item.name} htmlFor="item-name" required error={fieldErrors.name}>
              <Input
                id="item-name"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                invalid={!!fieldErrors.name}
                autoFocus
                maxLength={100}
              />
            </Field>
            <Field label={zh.item.importance} htmlFor="item-importance" error={fieldErrors.importance}>
              <Select
                id="item-importance"
                value={form.importance}
                onChange={(e) => setForm({ ...form, importance: Number(e.target.value) })}
              >
                {[0, 1, 2, 3, 4, 5].map((value) => (
                  <option key={value} value={value}>
                    {value} {value === 0 ? "（普通）" : value >= 4 ? "（关键）" : ""}
                  </option>
                ))}
              </Select>
            </Field>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label={zh.item.features} htmlFor="item-features" error={fieldErrors.features}>
              <Textarea
                id="item-features"
                rows={4}
                value={form.features}
                onChange={(e) => setForm({ ...form, features: e.target.value })}
                invalid={!!fieldErrors.features}
                placeholder="材质、外观、特殊能力…"
              />
            </Field>
            <Field label={zh.item.owner} htmlFor="item-owner">
              <Select
                id="item-owner"
                value={form.ownerId}
                onChange={(e) => setForm({ ...form, ownerId: e.target.value })}
              >
                <option value="">无持有者</option>
                {characters.map((character) => (
                  <option key={character.id} value={character.id}>
                    {character.name}
                  </option>
                ))}
              </Select>
            </Field>
          </div>
          <Field label={zh.item.description} htmlFor="item-description" error={fieldErrors.description}>
            <Textarea
              id="item-description"
              rows={4}
              value={form.description}
              onChange={(e) => setForm({ ...form, description: e.target.value })}
              invalid={!!fieldErrors.description}
              placeholder="它为什么重要？会在哪一章出场？"
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
