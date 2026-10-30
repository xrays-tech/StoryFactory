import { useEffect, useRef, useState } from "react";
import type { LoaderFunctionArgs } from "react-router-dom";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";
import { Boxes, Play, Plus, RefreshCw, Sparkles } from "lucide-react";
import type { SessionStatus } from "../types/domain";
import { modeLabel, sessionStatusLabel, zh } from "../copy/zh";
import { useCharacterStore } from "../stores/characterStore";
import { useOutlineStore } from "../stores/outlineStore";
import { useSandboxStore } from "../stores/sandboxStore";
import { PageHeader } from "../components/common/PageHeader";
import { Button } from "../components/common/Button";
import { Select } from "../components/common/Input";
import { SessionStatusBadge } from "../components/common/StatusBadge";
import { EmptyState, ErrorState } from "../components/common/EmptyState";
import { LoadingBlock } from "../components/common/Skeleton";
import { ProgressBar } from "../components/common/Progress";
import { CreateSessionWizard } from "../components/sandbox/CreateSessionWizard";
import { formatCost, formatDateTime } from "../lib/format";
import { cn } from "../lib/cn";

export async function sandboxListLoader({ params }: LoaderFunctionArgs) {
  const novelId = params.novelId;
  if (!novelId) return null;
  await Promise.all([
    useCharacterStore.getState().load(novelId),
    useOutlineStore.getState().loadTree(novelId),
    useSandboxStore.getState().loadSessions(novelId),
  ]);
  return null;
}

const statusOptions: SessionStatus[] = [
  "created",
  "running",
  "paused",
  "waiting_input",
  "finalizing",
  "completed",
  "failed",
  "interrupted",
  "aborted",
];

export function SandboxPage() {
  const { novelId } = useParams<{ novelId: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const navigate = useNavigate();
  const sessions = useSandboxStore((s) => s.sessions);
  const total = useSandboxStore((s) => s.sessionsTotal);
  const loading = useSandboxStore((s) => s.sessionsLoading);
  const error = useSandboxStore((s) => s.sessionsError);
  const filter = useSandboxStore((s) => s.sessionFilter);
  const loadSessions = useSandboxStore((s) => s.loadSessions);
  const characters = useCharacterStore((s) => s.items);

  const [wizardOpen, setWizardOpen] = useState(false);
  const defaultChapter = searchParams.get("chapter");
  const openedFromParam = useRef("");

  useEffect(() => {
    const autoKey = `${searchParams.get("new") ?? ""}:${searchParams.get("chapter") ?? ""}`;
    if (autoKey === ":" || openedFromParam.current === autoKey) return;
    openedFromParam.current = autoKey;
    setWizardOpen(true);
    if (searchParams.get("new") === "1") {
      const next = new URLSearchParams(searchParams);
      next.delete("new");
      setSearchParams(next, { replace: true });
    }
  }, [searchParams, setSearchParams]);

  if (!novelId) return null;

  const onFilter = (status: SessionStatus | "") => {
    void loadSessions(novelId, { status });
  };

  const refresh = () => void loadSessions(novelId, { silent: false });

  return (
    <div className="space-y-5">
      <PageHeader
        eyebrow={`${total} 个会话`}
        title={zh.sandbox.title}
        description={zh.sandbox.subtitle}
        actions={
          <>
            <Select
              value={filter}
              onChange={(e) => onFilter(e.target.value as SessionStatus | "")}
              aria-label={zh.sandbox.filterStatus}
              className="w-36"
            >
              <option value="">{zh.common.all}</option>
              {statusOptions.map((status) => (
                <option key={status} value={status}>
                  {sessionStatusLabel[status]}
                </option>
              ))}
            </Select>
            <Button
              variant="outline"
              size="icon"
              onClick={refresh}
              aria-label={zh.common.refresh}
              title={zh.common.refresh}
            >
              <RefreshCw className="h-4 w-4" />
            </Button>
            <Button
              variant="primary"
              leftIcon={<Sparkles className="h-4 w-4" />}
              onClick={() => setWizardOpen(true)}
            >
              {zh.sandbox.newSession}
            </Button>
          </>
        }
      />

      {loading && sessions.length === 0 ? (
        <LoadingBlock label="正在读取沙盒会话…" />
      ) : error && sessions.length === 0 ? (
        <ErrorState message={error} onRetry={refresh} />
      ) : sessions.length === 0 ? (
        <EmptyState
          icon={<Boxes className="h-5 w-5" />}
          title={filter ? "这个状态下还没有会话" : zh.sandbox.empty}
          description={filter ? "换一个状态筛选，或发起新的推演。" : zh.sandbox.emptyHint}
          action={
            <Button variant="primary" leftIcon={<Plus className="h-4 w-4" />} onClick={() => setWizardOpen(true)}>
              {zh.sandbox.newSession}
            </Button>
          }
        />
      ) : (
        <div className="space-y-2.5">
          {sessions.map((session) => {
            const active = ["running", "waiting_input", "finalizing", "paused", "interrupted"].includes(
              session.status,
            );
            return (
              <button
                key={session.id}
                type="button"
                onClick={() => navigate(`/sandbox/${session.id}`)}
                className={cn(
                  "focus-ring group relative flex w-full flex-col gap-3 overflow-hidden rounded-xl border border-line/80 bg-surface/80 p-4 text-left shadow-card transition-all duration-200 hover:-translate-y-0.5 hover:border-accent/35 hover:bg-raised hover:shadow-float sm:flex-row sm:items-center",
                )}
              >
                <span
                  className={cn(
                    "absolute inset-y-4 left-0 w-0.5 rounded-full",
                    active ? "bg-accent" : "bg-line",
                  )}
                />
                <div className="flex min-w-0 flex-1 flex-col gap-2 pl-1 sm:flex-row sm:items-center sm:gap-4">
                  <div className="w-[108px] shrink-0">
                    <SessionStatusBadge status={session.status} />
                  </div>
                  <div className="min-w-0 flex-1">
                    <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm font-medium text-ink">
                      <span>{modeLabel[session.mode]}</span>
                      <span className="text-faint">·</span>
                      <span className="truncate text-muted">
                        {session.actor_names.map((actor) => (actor.deleted ? `${actor.name}` : actor.name)).join("、") ||
                          "无角色"}
                      </span>
                    </p>
                    <p className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-faint">
                      <span>{formatDateTime(session.created_at)}</span>
                      <span>成本 {formatCost(session.usage.cost_usd)}</span>
                      <span>{session.usage.calls} 次调用</span>
                      {session.error ? <span className="text-danger">{session.error.code}</span> : null}
                    </p>
                  </div>
                </div>
                <div className="flex w-full shrink-0 items-center gap-3 sm:w-44">
                  <div className="flex-1">
                    <div className="flex items-center justify-between text-[10px] text-faint">
                      <span>{zh.sandbox.currentStep}</span>
                      <span className="font-mono">
                        {session.current_step}/{session.step_limit}
                      </span>
                    </div>
                    <ProgressBar
                      value={session.current_step}
                      max={session.step_limit}
                      className="mt-1"
                      tone={session.status === "failed" ? "danger" : session.status === "completed" ? "success" : "accent"}
                    />
                  </div>
                  <Play className="h-4 w-4 text-faint transition-colors group-hover:text-accent" />
                </div>
              </button>
            );
          })}
        </div>
      )}

      <CreateSessionWizard
        open={wizardOpen}
        onClose={() => setWizardOpen(false)}
        novelId={novelId}
        defaultChapterId={defaultChapter}
        onCreated={(session) => {
          setWizardOpen(false);
          navigate(`/sandbox/${session.id}`);
        }}
      />

      {characters.length === 0 ? (
        <p className="text-center text-xs text-faint">
          提示：沙盒至少需要一位角色。
          <button
            type="button"
            className="ml-1 font-medium text-accent-strong hover:underline"
            onClick={() => navigate(`/novel/${novelId}/characters`)}
          >
            去创建角色
          </button>
        </p>
      ) : null}
    </div>
  );
}
