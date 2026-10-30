import { useCallback, useEffect, useState } from "react";
import { RotateCcw, SearchX, Trash2 } from "lucide-react";
import type { RecycleEntity } from "../../types/domain";
import type { RecycleItem } from "../../types/api";
import * as api from "../../api/endpoints";
import { entityLabel, zh } from "../../copy/zh";
import { useUiStore } from "../../stores/uiStore";
import { userMessage } from "../../lib/errors";
import { formatDateTime } from "../../lib/format";
import { Button } from "../common/Button";
import { Select } from "../common/Input";
import { EmptyState, ErrorState } from "../common/EmptyState";
import { LoadingBlock } from "../common/Skeleton";
import { Table, TBody, TD, TH, THead, TR } from "../common/Table";
import { Badge } from "../common/Badge";

const entityTypes: RecycleEntity[] = ["novel", "volume", "chapter", "character", "item"];

export function RecycleBinPanel() {
  const toast = useUiStore((s) => s.toast);
  const askConfirm = useUiStore((s) => s.askConfirm);

  const [type, setType] = useState<RecycleEntity | "">("");
  const [items, setItems] = useState<RecycleItem[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [pageSize] = useState(50);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await api.listRecycle({ type, page, pageSize });
      setItems(result.items);
      setTotal(result.total);
      setSelected(new Set());
    } catch (err) {
      setError(userMessage(err));
    } finally {
      setLoading(false);
    }
  }, [type, page, pageSize]);

  useEffect(() => {
    void load();
  }, [load]);

  const toggle = (item: RecycleItem) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(item.id)) next.delete(item.id);
      else next.add(item.id);
      return next;
    });
  };

  const allSelected = items.length > 0 && items.every((item) => selected.has(item.id));

  const toggleAll = () => {
    if (allSelected) setSelected(new Set());
    else setSelected(new Set(items.map((item) => item.id)));
  };

  const restore = async (targets: RecycleItem[]) => {
    if (targets.length === 0) return;
    setBusy(true);
    try {
      const groups = new Map<RecycleEntity, string[]>();
      for (const item of targets) {
        const list = groups.get(item.entity_type) ?? [];
        list.push(item.id);
        groups.set(item.entity_type, list);
      }
      for (const [entityType, ids] of groups) {
        await api.restoreRecycle({ entity_type: entityType, ids });
      }
      toast({ tone: "success", message: `${targets.length} 项已恢复` });
      await load();
    } catch (err) {
      toast({ tone: "error", message: userMessage(err) });
    } finally {
      setBusy(false);
    }
  };

  const purge = async (targets: RecycleItem[]) => {
    if (targets.length === 0) return;
    const ok = await askConfirm({
      title: zh.recycle.purgeTitle,
      message:
        targets.length > 1
          ? `${targets.length} 项内容将被永久删除。${zh.recycle.purgeHint}`
          : `「${targets[0].name}」将被永久删除。${zh.recycle.purgeHint}`,
      confirmLabel: zh.recycle.purgeConfirm,
      tone: "danger",
    });
    if (!ok) return;
    setBusy(true);
    try {
      for (const item of targets) {
        await api.purgeRecycle(item.entity_type, item.id);
      }
      toast({ tone: "success", message: `${targets.length} 项已永久清除` });
      await load();
    } catch (err) {
      toast({ tone: "error", message: userMessage(err) });
    } finally {
      setBusy(false);
    }
  };

  const selectedItems = items.filter((item) => selected.has(item.id));
  const pageCount = Math.max(1, Math.ceil(total / pageSize));

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <Select
            value={type}
            onChange={(e) => {
              setType(e.target.value as RecycleEntity | "");
              setPage(1);
            }}
            className="w-32"
            aria-label={zh.recycle.type}
          >
            <option value="">{zh.common.all}</option>
            {entityTypes.map((entity) => (
              <option key={entity} value={entity}>
                {entityLabel[entity]}
              </option>
            ))}
          </Select>
          <span className="text-xs text-faint">共 {total} 项</span>
        </div>
        <div className="flex items-center gap-2">
          {selected.size > 0 ? (
            <>
              <Badge tone="accent">{zh.recycle.selected(selected.size)}</Badge>
              <Button
                size="sm"
                variant="outline"
                leftIcon={<RotateCcw className="h-3.5 w-3.5" />}
                loading={busy}
                onClick={() => void restore(selectedItems)}
              >
                {zh.recycle.restore}
              </Button>
              <Button
                size="sm"
                variant="danger"
                leftIcon={<Trash2 className="h-3.5 w-3.5" />}
                loading={busy}
                onClick={() => void purge(selectedItems)}
              >
                {zh.recycle.purge}
              </Button>
            </>
          ) : null}
        </div>
      </div>

      {loading ? (
        <LoadingBlock label="正在读取回收站…" />
      ) : error ? (
        <ErrorState message={error} onRetry={() => void load()} />
      ) : items.length === 0 ? (
        <EmptyState
          icon={<SearchX className="h-5 w-5" />}
          title={zh.recycle.empty}
          description={zh.recycle.emptyHint}
        />
      ) : (
        <Table>
          <THead>
            <tr>
              <TH className="w-10">
                <input
                  type="checkbox"
                  checked={allSelected}
                  onChange={toggleAll}
                  className="h-4 w-4 rounded border-line text-accent focus:ring-accent"
                  aria-label="全选"
                />
              </TH>
              <TH>{zh.recycle.type}</TH>
              <TH>{zh.recycle.name}</TH>
              <TH>{zh.recycle.project}</TH>
              <TH>{zh.recycle.deletedAt}</TH>
              <TH className="w-40 text-right">操作</TH>
            </tr>
          </THead>
          <TBody>
            {items.map((item) => (
              <TR key={`${item.entity_type}-${item.id}`}>
                <TD>
                  <input
                    type="checkbox"
                    checked={selected.has(item.id)}
                    onChange={() => toggle(item)}
                    className="h-4 w-4 rounded border-line text-accent focus:ring-accent"
                    aria-label={`选择 ${item.name}`}
                  />
                </TD>
                <TD>
                  <Badge tone="neutral">{entityLabel[item.entity_type] ?? item.entity_type}</Badge>
                </TD>
                <TD className="max-w-[260px]">
                  <p className="truncate font-medium text-ink">{item.name}</p>
                  <p className="mt-0.5 truncate font-mono text-[10px] text-faint">{item.id.slice(0, 12)}…</p>
                </TD>
                <TD className="max-w-[180px] truncate text-xs text-muted">{item.novel_title}</TD>
                <TD className="whitespace-nowrap text-[11px] text-faint">{formatDateTime(item.deleted_at)}</TD>
                <TD>
                  <div className="flex items-center justify-end gap-1">
                    <Button
                      size="sm"
                      variant="ghost"
                      leftIcon={<RotateCcw className="h-3 w-3" />}
                      disabled={busy}
                      onClick={() => void restore([item])}
                    >
                      {zh.recycle.restore}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      className="text-danger hover:bg-danger-soft"
                      leftIcon={<Trash2 className="h-3 w-3" />}
                      disabled={busy}
                      onClick={() => void purge([item])}
                    >
                      清除
                    </Button>
                  </div>
                </TD>
              </TR>
            ))}
          </TBody>
        </Table>
      )}

      {pageCount > 1 ? (
        <div className="flex items-center justify-end gap-2">
          <Button size="sm" variant="outline" disabled={page <= 1 || loading} onClick={() => setPage(page - 1)}>
            上一页
          </Button>
          <span className="text-xs text-faint">
            {page} / {pageCount}
          </span>
          <Button
            size="sm"
            variant="outline"
            disabled={page >= pageCount || loading}
            onClick={() => setPage(page + 1)}
          >
            下一页
          </Button>
        </div>
      ) : null}
    </div>
  );
}
