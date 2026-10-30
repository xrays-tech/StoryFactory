import { useMemo, useState } from "react";
import type { LoaderFunctionArgs } from "react-router-dom";
import { useNavigate, useParams } from "react-router-dom";
import { HeartHandshake, Save, Table2, Grid3X3, Users } from "lucide-react";
import { zh } from "../../copy/zh";
import {
  orderedPair,
  useCharacterStore,
  type Relation,
} from "../../stores/characterStore";
import { useUiStore } from "../../stores/uiStore";
import { userMessage } from "../../lib/errors";
import { PageHeader } from "../../components/common/PageHeader";
import { Button } from "../../components/common/Button";
import { Input } from "../../components/common/Input";
import { EmptyState, ErrorState } from "../../components/common/EmptyState";
import { LoadingBlock } from "../../components/common/Skeleton";
import { Tabs } from "../../components/common/Tabs";
import { Table, TBody, TD, TH, THead, TR } from "../../components/common/Table";
import { Badge } from "../../components/common/Badge";
import { cn } from "../../lib/cn";

export async function relationsLoader({ params }: LoaderFunctionArgs) {
  if (params.novelId) {
    await useCharacterStore.getState().load(params.novelId);
    await useCharacterStore.getState().loadRelations(params.novelId);
  }
  return null;
}

function cellTone(value: number): string {
  if (value >= 60) return "bg-accent/20 text-accent-strong dark:text-accent";
  if (value >= 20) return "bg-accent/10 text-ink";
  if (value <= -60) return "bg-danger/15 text-danger";
  if (value <= -20) return "bg-danger/10 text-ink";
  return "bg-transparent text-muted";
}

export function RelationsPage() {
  const { novelId } = useParams<{ novelId: string }>();
  const navigate = useNavigate();
  const characters = useCharacterStore((s) => s.items);
  const relations = useCharacterStore((s) => s.relationsDraft);
  const saved = useCharacterStore((s) => s.relationsSaved);
  const loading = useCharacterStore((s) => s.relationsLoading);
  const saving = useCharacterStore((s) => s.relationsSaving);
  const error = useCharacterStore((s) => s.relationsError);
  const dirty = useCharacterStore((s) => s.relationsDirty);
  const setField = useCharacterStore((s) => s.setRelationField);
  const save = useCharacterStore((s) => s.saveRelations);
  const loadRelations = useCharacterStore((s) => s.loadRelations);
  const loadCharacters = useCharacterStore((s) => s.load);
  const toast = useUiStore((s) => s.toast);
  const [view, setView] = useState<"table" | "matrix">("table");

  const names = useMemo(() => {
    const map = new Map<string, string>();
    for (const character of characters) map.set(character.id, character.name);
    return map;
  }, [characters]);

  const relationMap = useMemo(() => {
    const map = new Map<string, Relation>();
    for (const relation of relations) {
      map.set(`${relation.char_a_id}::${relation.char_b_id}`, relation);
    }
    return map;
  }, [relations]);

  if (!novelId) return null;

  const reload = async () => {
    await loadCharacters(novelId);
    await loadRelations(novelId);
  };

  const onSave = async () => {
    try {
      await save();
      toast({ tone: "success", message: zh.relation.saved });
    } catch (err) {
      const status = (err as { status?: number }).status;
      if (status !== 409 && status !== 422 && status !== 429) {
        toast({ tone: "error", message: userMessage(err) });
      }
    }
  };

  if (loading && relations.length === 0) {
    return <LoadingBlock label="正在读取羁绊…" />;
  }

  if (error && relations.length === 0 && saved.length === 0 && characters.length === 0) {
    return <ErrorState message={error} onRetry={() => void reload()} />;
  }

  if (characters.length < 2) {
    return (
      <div className="space-y-5">
        <PageHeader title={zh.relation.title} description={zh.relation.subtitle} />
        <EmptyState
          icon={<Users className="h-5 w-5" />}
          title={zh.relation.empty}
          description={zh.relation.emptyHint}
          action={
            <Button variant="primary" onClick={() => navigate(`/novel/${novelId}/characters`)}>
              去创建角色
            </Button>
          }
        />
      </div>
    );
  }

  return (
    <div className="space-y-5">
      <PageHeader
        title={zh.relation.title}
        description={zh.relation.subtitle}
        actions={
          <>
            <Tabs
              value={view}
              onChange={setView}
              items={[
                { value: "table", label: <><Table2 className="h-3.5 w-3.5" /> {zh.relation.table}</> },
                { value: "matrix", label: <><Grid3X3 className="h-3.5 w-3.5" /> {zh.relation.matrix}</> },
              ]}
            />
            <Button
              variant="primary"
              leftIcon={<Save className="h-4 w-4" />}
              loading={saving}
              disabled={!dirty}
              onClick={() => void onSave()}
            >
              {zh.relation.save}
            </Button>
          </>
        }
      />

      <div className="flex flex-wrap items-center gap-2 text-xs">
        {dirty ? (
          <Badge tone="warning" dot>
            {zh.relation.unsaved}
          </Badge>
        ) : (
          <Badge tone="success" dot>
            已与服务端同步
          </Badge>
        )}
        <span className="text-faint">
          {relations.length} 组关系 · {characters.length} 位角色
        </span>
      </div>

      {error ? (
        <p className="rounded-xl border border-danger/25 bg-danger-soft/40 px-3 py-2 text-sm text-danger" role="alert">
          {error}
        </p>
      ) : null}

      {view === "table" ? (
        <Table>
          <THead>
            <tr>
              <TH>角色 A</TH>
              <TH>角色 B</TH>
              <TH className="w-28 text-center">{zh.relation.affinity}</TH>
              <TH className="w-28 text-center">{zh.relation.trust}</TH>
              <TH>{zh.relation.conflict}</TH>
            </tr>
          </THead>
          <TBody>
            {relations.map((relation) => {
              const [a, b] = orderedPair(relation.char_a_id, relation.char_b_id);
              return (
                <TR key={`${a}::${b}`}>
                  <TD className="whitespace-nowrap font-medium text-ink">{names.get(a) ?? "未知角色"}</TD>
                  <TD className="whitespace-nowrap font-medium text-ink">{names.get(b) ?? "未知角色"}</TD>
                  <TD>
                    <Input
                      type="number"
                      min={-100}
                      max={100}
                      step={5}
                      value={relation.affinity}
                      onChange={(e) => setField(a, b, "affinity", e.target.value)}
                      aria-label={`${names.get(a)} 与 ${names.get(b)} 的亲密度`}
                      className={cn("h-8 text-center font-mono text-xs", relation.affinity > 0 && "text-success", relation.affinity < 0 && "text-danger")}
                    />
                  </TD>
                  <TD>
                    <Input
                      type="number"
                      min={0}
                      max={100}
                      step={5}
                      value={relation.trust}
                      onChange={(e) => setField(a, b, "trust", e.target.value)}
                      aria-label={`${names.get(a)} 与 ${names.get(b)} 的信任度`}
                      className="h-8 text-center font-mono text-xs"
                    />
                  </TD>
                  <TD>
                    <Input
                      value={relation.conflict_desc ?? ""}
                      onChange={(e) => setField(a, b, "conflict_desc", e.target.value)}
                      placeholder="未说出口的冲突、旧债或默契…"
                      maxLength={1000}
                      className="h-8 min-w-[180px] text-xs"
                    />
                  </TD>
                </TR>
              );
            })}
          </TBody>
        </Table>
      ) : (
        <div className="space-y-3">
          <p className="text-xs text-faint">{zh.relation.matrixHint}</p>
          <div className="w-full overflow-x-auto rounded-xl border border-line/70 bg-raised">
            <table className="w-full border-collapse text-xs">
              <thead>
                <tr>
                  <th className="sticky left-0 z-10 min-w-[120px] border-b border-r border-line/70 bg-paper/80 px-3 py-2 text-left font-medium text-muted">
                    角色
                  </th>
                  {characters.map((character) => (
                    <th
                      key={character.id}
                      className="min-w-[92px] border-b border-line/70 px-2 py-2 text-center font-medium text-muted"
                      title={character.name}
                    >
                      <span className="block max-w-[84px] truncate">{character.name}</span>
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {characters.map((row) => (
                  <tr key={row.id} className="border-b border-line/50 last:border-0">
                    <th className="sticky left-0 z-10 max-w-[140px] truncate border-r border-line/70 bg-raised px-3 py-2 text-left font-medium text-ink">
                      {row.name}
                    </th>
                    {characters.map((col) => {
                      if (row.id === col.id) {
                        return (
                          <td key={col.id} className="px-2 py-2 text-center text-faint/50">
                            —
                          </td>
                        );
                      }
                      const [a, b] = orderedPair(row.id, col.id);
                      const relation = relationMap.get(`${a}::${b}`);
                      const affinity = relation?.affinity ?? 0;
                      const trust = relation?.trust ?? 0;
                      return (
                        <td key={col.id} className="p-1 text-center">
                          <div
                            className={cn(
                              "rounded-md border border-transparent px-1 py-1.5 font-mono text-[11px] leading-4 transition-colors",
                              cellTone(affinity),
                            )}
                            title={`${row.name} ↔ ${col.name}：亲密度 ${affinity}，信任度 ${trust}`}
                          >
                            <span className="block">{affinity}</span>
                            <span className="block text-[10px] opacity-65">{trust}</span>
                          </div>
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="flex items-center gap-4 text-[11px] text-faint">
            <span className="inline-flex items-center gap-1.5">
              <span className="h-2.5 w-4 rounded bg-accent/20" /> 亲近
            </span>
            <span className="inline-flex items-center gap-1.5">
              <span className="h-2.5 w-4 rounded bg-danger/15" /> 对立
            </span>
            <span className="inline-flex items-center gap-1.5">
              <HeartHandshake className="h-3 w-3" /> 上：亲密度 / 下：信任度
            </span>
          </div>
        </div>
      )}
    </div>
  );
}
