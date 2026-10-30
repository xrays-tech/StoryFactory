import { useEffect, useMemo, useRef, useState } from "react";
import type { LoaderFunctionArgs } from "react-router-dom";
import { useBlocker, useNavigate, useParams } from "react-router-dom";
import {
  AlertTriangle,
  ArrowLeft,
  ArrowRight,
  ChevronDown,
  ChevronRight,
  CircleDot,
  Clock,
  Coins,
  CornerDownLeft,
  Cpu,
  FastForward,
  Pause,
  Play,
  RotateCcw,
  Save,
  Send,
  Square,
  StepForward,
  Terminal,
  Wifi,
  WifiOff,
} from "lucide-react";
import type { ControlAction, SessionStatus } from "../types/domain";
import { modeLabel, sessionStatusLabel, zh } from "../copy/zh";
import {
  isSessionTerminal,
  useSandboxStore,
} from "../stores/sandboxStore";
import { useOutlineStore } from "../stores/outlineStore";
import { useUiStore } from "../stores/uiStore";
import type { SandboxEvent } from "../api/sse";
import * as api from "../api/endpoints";
import { ApiError, userMessage } from "../lib/errors";
import { formatCost, formatDateTime, formatDuration, formatNumber } from "../lib/format";
import { cn } from "../lib/cn";
import { Button } from "../components/common/Button";
import { Badge } from "../components/common/Badge";
import { Dialog } from "../components/common/Dialog";
import { SessionStatusBadge } from "../components/common/StatusBadge";
import { ProgressBar } from "../components/common/Progress";
import { LoadingBlock, Spinner } from "../components/common/Skeleton";
import { ErrorState, EmptyState } from "../components/common/EmptyState";
import { MarkdownPreview } from "../components/common/MarkdownPreview";
import { Textarea } from "../components/common/Input";

export async function sandboxConsoleLoader({ params }: LoaderFunctionArgs) {
  const sid = params.sid;
  if (!sid) return null;
  useSandboxStore.getState().disconnect(true);
  await useSandboxStore.getState().loadSession(sid);
  return null;
}

const controlMeta: Record<
  ControlAction,
  { label: string; icon: typeof Play; danger?: boolean }
> = {
  start: { label: zh.sandbox.start, icon: Play },
  pause: { label: zh.sandbox.pause, icon: Pause },
  resume: { label: zh.sandbox.resume, icon: Play },
  step: { label: zh.sandbox.step, icon: StepForward },
  abort: { label: zh.sandbox.abort, icon: Square, danger: true },
  recover: { label: zh.sandbox.recover, icon: RotateCcw },
};

const allowedByStatus: Record<SessionStatus, ControlAction[]> = {
  created: ["start", "abort"],
  running: ["pause", "abort"],
  paused: ["resume", "step", "abort"],
  waiting_input: ["abort"],
  finalizing: ["abort"],
  completed: [],
  failed: [],
  interrupted: ["recover", "abort"],
  aborted: [],
};

const actorTones = [
  {
    text: "text-accent-strong dark:text-accent",
    border: "border-l-accent",
    bg: "bg-accent-soft/35",
    dot: "bg-accent",
  },
  {
    text: "text-info",
    border: "border-l-info",
    bg: "bg-info-soft/35",
    dot: "bg-info",
  },
  {
    text: "text-success",
    border: "border-l-success",
    bg: "bg-success-soft/35",
    dot: "bg-success",
  },
  {
    text: "text-warning",
    border: "border-l-warning",
    bg: "bg-warning-soft/35",
    dot: "bg-warning",
  },
  {
    text: "text-danger",
    border: "border-l-danger",
    bg: "bg-danger-soft/30",
    dot: "bg-danger",
  },
];

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function asNumber(value: unknown): number {
  const n = Number(value);
  return Number.isFinite(n) ? n : 0;
}

function asStringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function actionTypeLabel(value: string): string {
  const map: Record<string, string> = {
    dialogue: "对白",
    action: "行动",
    thought: "内心",
    inner: "内心",
    narration: "叙述",
    description: "描写",
  };
  return map[value] ?? value ?? "行动";
}

type EventGroup = { key: string; step: number | null; events: SandboxEvent[] };

function groupEvents(events: SandboxEvent[]): EventGroup[] {
  const groups: EventGroup[] = [];
  for (const event of events) {
    const step = typeof event.payload.step === "number" ? event.payload.step : null;
    const last = groups[groups.length - 1];
    if (step !== null && last && last.step === step) {
      last.events.push(event);
    } else {
      groups.push({ key: `${event.seq}-${step ?? "sys"}`, step, events: [event] });
    }
  }
  return groups;
}

function connectionLabel(state: string): string {
  switch (state) {
    case "open":
      return zh.sandbox.connected;
    case "connecting":
      return zh.sandbox.connecting;
    case "reconnecting":
      return zh.sandbox.reconnecting;
    case "closed":
      return zh.sandbox.disconnected;
    case "error":
      return "连接异常";
    default:
      return zh.sandbox.disconnected;
  }
}

export function SandboxConsole() {
  const { sid } = useParams<{ sid: string }>();
  const navigate = useNavigate();
  const toast = useUiStore((s) => s.toast);
  const askConfirm = useUiStore((s) => s.askConfirm);

  const session = useSandboxStore((s) => s.session);
  const events = useSandboxStore((s) => s.events);
  const connection = useSandboxStore((s) => s.connection);
  const reconnectAttempt = useSandboxStore((s) => s.reconnectAttempt);
  const streamError = useSandboxStore((s) => s.streamError);
  const loading = useSandboxStore((s) => s.snapshotLoading);
  const error = useSandboxStore((s) => s.snapshotError);
  const autoScroll = useSandboxStore((s) => s.autoScroll);
  const outline = useSandboxStore((s) => s.outline);
  const outlineChapterId = useSandboxStore((s) => s.outlineChapterId);
  const loadSession = useSandboxStore((s) => s.loadSession);
  const connect = useSandboxStore((s) => s.connect);
  const disconnect = useSandboxStore((s) => s.disconnect);
  const control = useSandboxStore((s) => s.control);
  const sendInput = useSandboxStore((s) => s.sendInput);
  const setAutoScroll = useSandboxStore((s) => s.setAutoScroll);

  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const [pendingAction, setPendingAction] = useState<ControlAction | null>(null);
  const [inputText, setInputText] = useState("");
  const [sendingInput, setSendingInput] = useState(false);
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set());
  const [outlineSaving, setOutlineSaving] = useState(false);
  const [outlineSaved, setOutlineSaved] = useState(false);
  const [leaveBusy, setLeaveBusy] = useState<"pause" | "abort" | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  const active = !!session && !isSessionTerminal(session.status);
  const blocker = useBlocker(active);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!session || isSessionTerminal(session.status)) return;
    const handler = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", handler);
    return () => window.removeEventListener("beforeunload", handler);
  }, [session?.status]);

  const connectedSessionRef = useRef<string | null>(null);

  useEffect(() => {
    const sid = session?.id;
    if (!sid) return;

    const { connection, lastEventSeq } = useSandboxStore.getState();

    // 首次进入（含已终结会话）必须连一次，服务端会按 after_seq 回放历史事件。
    if (connectedSessionRef.current !== sid) {
      connectedSessionRef.current = sid;
      connect(sid, lastEventSeq);
      return;
    }

    if (isSessionTerminal(session.status)) {
      disconnect();
      return;
    }

    // interrupted -> running（recover）时 id 不变，但流可能已被 sandbox.done 关闭；
    // 只在非终态且当前没有活跃流时重连，connect() 自身负责关闭旧流。
    if (connection === "idle" || connection === "closed") {
      connect(sid, lastEventSeq);
    }
  }, [session?.id, session?.status, connect, disconnect]);

  useEffect(
    () => () => {
      connectedSessionRef.current = null;
      disconnect();
    },
    [disconnect],
  );

  useEffect(() => {
    if (!session?.id || isSessionTerminal(session.status)) return;
    const timer = window.setInterval(() => {
      void loadSession(session.id, { silent: true });
    }, 30_000);
    return () => window.clearInterval(timer);
  }, [session?.id, session?.status, loadSession]);

  useEffect(() => {
    if (!autoScroll) return;
    const el = scrollRef.current;
    if (!el) return;
    const frame = requestAnimationFrame(() => {
      el.scrollTop = el.scrollHeight;
    });
    return () => cancelAnimationFrame(frame);
  }, [events.length, autoScroll]);

  useEffect(() => {
    if (outline) setOutlineSaved(false);
  }, [outline]);

  if (!sid) return null;

  if (loading && !session) return <LoadingBlock label="正在打开会话…" />;

  if (error && !session) {
    return (
      <ErrorState
        title="会话打不开"
        message={error}
        onRetry={() => void loadSession(sid)}
      />
    );
  }

  if (!session) {
    return (
      <EmptyState
        icon={<Terminal className="h-5 w-5" />}
        title="会话不存在"
        description="它可能已随项目一起被删除。"
        action={
          <Button variant="outline" onClick={() => navigate("/")}>
            {zh.nav.backHome}
          </Button>
        }
      />
    );
  }

  const actors = session.actor_names;
  const actorName = (id: string) => actors.find((actor) => actor.id === id)?.name ?? "未知角色";
  const actorTone = (id: string) => {
    const index = session.actor_ids.indexOf(id);
    const hash = id.split("").reduce((sum, ch) => sum + ch.charCodeAt(0), 0);
    return actorTones[(index >= 0 ? index : hash) % actorTones.length];
  };

  const groups = groupEvents(events);
  const latestWaiting = [...events].reverse().find((event) => event.event_type === "waiting_input");
  const suggestions = asStringArray(latestWaiting?.payload.suggestions);
  const fallbackEvents = events.filter((event) => event.event_type === "director_fallback");
  const deadlineRemaining = session.session_deadline ? session.session_deadline - now : null;

  const statusMessage =
    session.status === "completed"
      ? "会话已完成，可以保存大纲定稿。"
      : session.status === "failed"
        ? "会话失败，检查右侧错误信息后可以重新发起。"
        : session.status === "aborted"
          ? "会话已中止。"
          : session.status === "interrupted"
            ? "会话因服务重启或超时被中断，可以恢复。"
            : "";

  const runControl = async (action: ControlAction) => {
    if (action === "abort") {
      const ok = await askConfirm({
        title: "中止会话？",
        message: "中止后不能恢复，已经落盘的事件会保留。",
        confirmLabel: zh.sandbox.abort,
        tone: "danger",
      });
      if (!ok) return;
    }
    setPendingAction(action);
    try {
      await control(action);
      toast({ tone: "success", message: `已发送：${controlMeta[action].label}` });
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) {
        await loadSession(session.id, { silent: true });
      } else {
        toast({ tone: "error", message: userMessage(err) });
      }
    } finally {
      setPendingAction(null);
    }
  };

  const submitInput = async () => {
    const text = inputText.trim();
    if (!text) return;
    setSendingInput(true);
    try {
      await sendInput(text);
      setInputText("");
      toast({ tone: "success", message: "指令已发送" });
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) {
        await loadSession(session.id, { silent: true });
      } else {
        toast({ tone: "error", message: userMessage(err) });
      }
    } finally {
      setSendingInput(false);
    }
  };

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distance > 90 && autoScroll) setAutoScroll(false);
    if (distance < 36 && !autoScroll) setAutoScroll(true);
  };

  const scrollToLatest = () => {
    setAutoScroll(true);
    const el = scrollRef.current;
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  };

  const saveOutline = async () => {
    if (!outline) return;
    const chapterId = outlineChapterId ?? session.chapter_id;
    if (!chapterId) {
      toast({ tone: "warning", message: "这次是探索会话，没有可写入的章节。" });
      return;
    }
    setOutlineSaving(true);
    try {
      await api.updateChapter(session.novel_id, chapterId, {
        final_outline: outline,
        status: "completed",
      });
      setOutlineSaved(true);
      toast({ tone: "success", message: zh.sandbox.saveOutlineDone });
      await useOutlineStore.getState().refreshChapter(session.novel_id, chapterId);
    } catch (err) {
      toast({ tone: "error", message: userMessage(err) });
    } finally {
      setOutlineSaving(false);
    }
  };

  const toggleDirector = (seq: number) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(seq)) next.delete(seq);
      else next.add(seq);
      return next;
    });
  };

  const leave = async (action: "background" | "pause" | "abort") => {
    if (blocker.state !== "blocked") return;
    if (action === "background") {
      blocker.proceed?.();
      return;
    }
    setLeaveBusy(action);
    try {
      if (action === "pause" && session.status === "running") {
        await control("pause");
      }
      if (action === "abort") {
        await control("abort");
      }
      blocker.proceed?.();
    } catch (err) {
      toast({ tone: "error", message: userMessage(err) });
    } finally {
      setLeaveBusy(null);
    }
  };

  const renderEvent = (event: SandboxEvent) => {
    const payload = event.payload;
    switch (event.event_type) {
      case "step_started":
        return null;
      case "session_started": {
        const ids = asStringArray(payload.actor_ids);
        return (
          <div key={event.seq} className="rounded-xl border border-line/60 bg-paper/50 px-3 py-2 text-[11px] text-muted">
            <span className="font-medium text-ink">会话开始</span> · {String(payload.mode ?? "")} ·{" "}
            {ids.map(actorName).join("、")} · 上限 {asNumber(payload.step_limit)} 步
          </div>
        );
      }
      case "session_paused":
        return (
          <div key={event.seq} className="flex items-center gap-2 text-[11px] text-warning">
            <Pause className="h-3 w-3" /> 会话暂停（{asString(payload.by) || "system"}）
          </div>
        );
      case "session_resumed":
        return (
          <div key={event.seq} className="flex items-center gap-2 text-[11px] text-info">
            <Play className="h-3 w-3" /> 会话继续（{asString(payload.cause) || "resume"}）
          </div>
        );
      case "session_interrupted":
        return (
          <div key={event.seq} className="rounded-xl border border-warning/30 bg-warning-soft/50 px-3 py-2 text-[11px] text-warning">
            <AlertTriangle className="mr-1 inline h-3 w-3" /> 会话被中断（{asString(payload.cause) || "unknown"}），可点击「恢复」继续。
          </div>
        );
      case "director_decision": {
        const open = expanded.has(event.seq);
        const actorId = asString(payload.next_actor_id);
        const tone = actorTone(actorId);
        return (
          <div key={event.seq} className="rounded-xl border border-line/60 bg-paper/45">
            <button
              type="button"
              onClick={() => toggleDirector(event.seq)}
              className="focus-ring flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left"
              aria-expanded={open}
            >
              {open ? <ChevronDown className="h-3.5 w-3.5 text-faint" /> : <ChevronRight className="h-3.5 w-3.5 text-faint" />}
              <CircleDot className={cn("h-3 w-3", tone.text)} />
              <span className={cn("text-[11px] font-medium", tone.text)}>导演</span>
              <span className="text-[11px] text-muted">选择 {actorName(actorId)}</span>
              <span className="ml-auto text-[10px] text-faint">#{event.seq}</span>
            </button>
            {open ? (
              <div className="space-y-2 border-t border-line/50 px-3 py-2 text-[11px] leading-5 text-muted">
                {asString(payload.reason) ? <p>理由：{asString(payload.reason)}</p> : null}
                {asString(payload.hint) ? <p>提示：{asString(payload.hint)}</p> : null}
              </div>
            ) : null}
          </div>
        );
      }
      case "director_fallback":
        return (
          <div key={event.seq} className="rounded-xl border border-warning/35 bg-warning-soft/60 px-3 py-2 text-[11px] leading-5 text-warning">
            <AlertTriangle className="mr-1 inline h-3 w-3" />
            导演降级为 {asString(payload.fallback_to) || "roundrobin"} · {asString(payload.cause) || "输出不可用"}
          </div>
        );
      case "actor_action": {
        const actorId = asString(payload.actor_id);
        const tone = actorTone(actorId);
        const action = asRecord(payload.action);
        const delta = asRecord(payload.emotion_delta);
        const affinity = asNumber(delta.affinity_delta);
        const trust = asNumber(delta.trust_delta);
        return (
          <article
            key={event.seq}
            className={cn("rounded-xl border border-line/60 border-l-2 bg-raised p-3", tone.border)}
          >
            <header className="flex flex-wrap items-center gap-2">
              <span className={cn("inline-flex items-center gap-1.5 text-xs font-semibold", tone.text)}>
                <span className={cn("h-1.5 w-1.5 rounded-full", tone.dot)} />
                {asString(payload.actor_name) || actorName(actorId)}
              </span>
              <Badge tone="muted">{actionTypeLabel(asString(action.type))}</Badge>
              <span className="ml-auto font-mono text-[10px] text-faint">#{event.seq}</span>
            </header>
            <p className="mt-2 whitespace-pre-wrap text-sm leading-7 text-ink">
              {asString(action.content) || "（空行动）"}
            </p>
            {affinity !== 0 || trust !== 0 ? (
              <div className="mt-2 flex flex-wrap gap-1.5 text-[10px]">
                <span className={cn("rounded-full px-2 py-0.5", affinity >= 0 ? "bg-accent-soft text-accent-strong" : "bg-danger-soft text-danger")}>
                  亲密度 {affinity > 0 ? "+" : ""}{affinity}
                </span>
                <span className={cn("rounded-full px-2 py-0.5", trust >= 0 ? "bg-success-soft text-success" : "bg-danger-soft text-danger")}>
                  信任度 {trust > 0 ? "+" : ""}{trust}
                </span>
                {asString(delta.target_actor_id) ? (
                  <span className="rounded-full bg-line/30 px-2 py-0.5 text-muted">
                    对象：{actorName(asString(delta.target_actor_id))}
                  </span>
                ) : null}
              </div>
            ) : null}
            {asString(payload.director_note) ? (
              <details className="mt-2 border-t border-line/50 pt-1.5">
                <summary className="cursor-pointer text-[10px] text-faint hover:text-muted">导演备注</summary>
                <p className="mt-1 text-[11px] leading-5 text-muted">{asString(payload.director_note)}</p>
              </details>
            ) : null}
          </article>
        );
      }
      case "actor_error":
        return (
          <div key={event.seq} className="rounded-xl border border-danger/30 bg-danger-soft/50 px-3 py-2 text-[11px] leading-5 text-danger">
            <span className="font-medium">{actorName(asString(payload.actor_id))} 行动失败</span>
            {asNumber(payload.attempt) ? ` · 第 ${asNumber(payload.attempt)} 次` : ""}
            <p className="mt-1 text-danger/90">{asString(payload.message)}</p>
          </div>
        );
      case "emotion_updated": {
        const pair = Array.isArray(payload.pair) ? payload.pair : [];
        const a = typeof pair[0] === "string" ? pair[0] : "";
        const b = typeof pair[1] === "string" ? pair[1] : "";
        return (
          <div key={event.seq} className="flex flex-wrap items-center gap-1.5 text-[10px] text-muted">
            <span className="inline-flex items-center gap-1 rounded-full bg-line/25 px-2 py-0.5">
              {actorName(a)} ↔ {actorName(b)}
            </span>
            <span>亲密度 {asNumber(payload.affinity_delta) > 0 ? "+" : ""}{asNumber(payload.affinity_delta)}</span>
            <span>信任度 {asNumber(payload.trust_delta) > 0 ? "+" : ""}{asNumber(payload.trust_delta)}</span>
          </div>
        );
      }
      case "rolling_summary_applied":
        return (
          <div key={event.seq} className="flex items-center gap-2 text-[10px] text-faint">
            <span className="h-px flex-1 bg-line/70" />
            {zh.sandbox.summaryApplied} · until #{asNumber(payload.until_seq)} · {asNumber(payload.summary_tokens)} tokens
            <span className="h-px flex-1 bg-line/70" />
          </div>
        );
      case "waiting_input":
        return (
          <div key={event.seq} className="rounded-xl border border-info/30 bg-info-soft/50 p-3">
            <p className="text-[11px] font-medium text-info">{zh.sandbox.waitingInput}</p>
            <p className="mt-1 whitespace-pre-wrap text-sm leading-6 text-ink">{asString(payload.question)}</p>
            {asStringArray(payload.suggestions).length > 0 ? (
              <div className="mt-2 flex flex-wrap gap-1.5">
                {asStringArray(payload.suggestions).map((suggestion) => (
                  <button
                    key={suggestion}
                    type="button"
                    onClick={() => setInputText(suggestion)}
                    className="focus-ring rounded-full border border-info/25 bg-raised px-2 py-0.5 text-[10px] text-info hover:bg-info-soft"
                  >
                    {suggestion}
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        );
      case "user_input_received":
        return (
          <div key={event.seq} className="ml-auto max-w-[85%] rounded-xl rounded-br-sm bg-ink px-3 py-2 text-sm leading-6 text-paper dark:bg-raised dark:text-ink">
            <p className="mb-0.5 text-[10px] opacity-60">你</p>
            <p className="whitespace-pre-wrap">{asString(payload.text)}</p>
          </div>
        );
      case "finalize_started":
        return (
          <div key={event.seq} className="flex items-center gap-2 rounded-lg bg-info-soft/40 px-3 py-2 text-[11px] text-info">
            <FastForward className="h-3.5 w-3.5" /> 开始收束 · 原因 {asString(payload.cause) || "unknown"}
          </div>
        );
      case "outline_ready":
        return (
          <div key={event.seq} className="rounded-xl border border-accent/30 bg-accent-soft/50 px-3 py-2 text-[11px] text-accent-strong dark:text-accent">
            <SparklesInline /> 大纲已生成，可在右侧预览并保存定稿。
          </div>
        );
      case "session_completed":
        return (
          <div key={event.seq} className="rounded-xl border border-success/30 bg-success-soft/60 px-3 py-2.5 text-sm text-success">
            <span className="font-medium">会话完成</span> · 花费 {formatCost(session.usage.cost_usd)} · {session.usage.calls} 次调用
          </div>
        );
      case "session_failed":
        return (
          <div key={event.seq} className="rounded-xl border border-danger/30 bg-danger-soft/60 px-3 py-2.5 text-sm text-danger">
            <span className="font-medium">会话失败</span> · {asString(payload.error_code)} · {asString(payload.message)}
          </div>
        );
      case "session_aborted":
        return (
          <div key={event.seq} className="rounded-xl border border-line/70 bg-line/20 px-3 py-2 text-sm text-muted">
            <span className="font-medium text-ink">会话已中止</span> · by {asString(payload.by) || "user"}
          </div>
        );
      default:
        return (
          <div key={event.seq} className="rounded-xl border border-line/70 bg-paper/40 px-3 py-2 text-[11px] text-muted">
            <span className="font-medium text-ink">{zh.sandbox.unknownEvent}</span> · {event.original_type}
            <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all font-mono text-[10px] text-faint">
              {JSON.stringify(payload, null, 2)}
            </pre>
          </div>
        );
    }
  };

  const canControl = (action: ControlAction) => allowedByStatus[session.status].includes(action);

  return (
    <div className="space-y-4">
      <div className="flex flex-col gap-3 border-b border-line/70 pb-4 lg:flex-row lg:items-center lg:justify-between">
        <div className="flex min-w-0 items-start gap-3">
          <Button
            variant="ghost"
            size="icon"
            className="mt-0.5 shrink-0"
            onClick={() => navigate(`/novel/${session.novel_id}/sandbox`)}
            aria-label={zh.nav.backHome}
            title="返回沙盒列表"
          >
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <SessionStatusBadge status={session.status} />
              <span className="text-[11px] text-faint">#{session.id.slice(0, 8)}</span>
              <span className="text-[11px] text-faint">{modeLabel[session.mode]}</span>
              {session.chapter_id ? (
                <button
                  type="button"
                  onClick={() => navigate(`/novel/${session.novel_id}/outline`)}
                  className="focus-ring rounded text-[11px] font-medium text-accent-strong hover:underline"
                >
                  关联章节
                </button>
              ) : (
                <span className="text-[11px] text-faint">探索会话</span>
              )}
            </div>
            <h1 className="mt-1 font-display text-xl font-semibold tracking-tight text-ink sm:text-2xl">
              沙盒控制台
            </h1>
            <p className="mt-0.5 truncate text-xs text-muted">
              {actors.map((actor) => actor.name).join("、") || "无角色"} · 创建于 {formatDateTime(session.created_at)}
            </p>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <span
            className={cn(
              "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px]",
              connection === "open"
                ? "border-success/30 bg-success-soft text-success"
                : connection === "reconnecting" || connection === "connecting"
                  ? "border-warning/30 bg-warning-soft text-warning"
                  : "border-line bg-line/20 text-muted",
            )}
            title={streamError ?? undefined}
          >
            {connection === "open" ? <Wifi className="h-3 w-3" /> : <WifiOff className="h-3 w-3" />}
            {connectionLabel(connection)}
            {reconnectAttempt > 0 ? ` · 第 ${reconnectAttempt} 次` : ""}
          </span>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void loadSession(session.id, { silent: true })}
          >
            刷新快照
          </Button>
        </div>
      </div>

      {statusMessage ? (
        <p
          className={cn(
            "rounded-lg border px-3 py-2 text-xs leading-5",
            session.status === "completed"
              ? "border-success/30 bg-success-soft/50 text-success"
              : session.status === "failed"
                ? "border-danger/30 bg-danger-soft/50 text-danger"
                : "border-line/70 bg-surface text-muted",
          )}
        >
          {statusMessage}
        </p>
      ) : null}

      <div className="grid gap-4 lg:grid-cols-[280px_minmax(0,1fr)] xl:grid-cols-[260px_minmax(0,1fr)_320px]">
        <aside className="order-1 space-y-3 lg:sticky lg:top-[4.8rem] lg:self-start">
          <section className="surface-card p-4">
            <div className="flex items-center justify-between gap-2">
              <h2 className="font-display text-sm font-semibold text-ink">会话控制</h2>
              <span className="font-mono text-[10px] text-faint">{sessionStatusLabel[session.status]}</span>
            </div>
            <div className="mt-3">
              <div className="flex items-end justify-between">
                <span className="font-display text-2xl font-semibold text-ink">
                  {session.current_step}
                  <span className="text-sm text-faint"> / {session.step_limit}</span>
                </span>
                <span className="text-[10px] text-faint">{zh.sandbox.currentStep}</span>
              </div>
              <ProgressBar
                value={session.current_step}
                max={session.step_limit}
                className="mt-1.5"
                tone={session.status === "failed" ? "danger" : session.status === "completed" ? "success" : "accent"}
              />
            </div>

            <div className="mt-3 flex items-center justify-between rounded-lg bg-paper/50 px-2.5 py-2 text-[11px]">
              <span className="flex items-center gap-1.5 text-faint">
                <Clock className="h-3 w-3" /> 会话剩余
              </span>
              <span
                className={cn(
                  "font-mono",
                  deadlineRemaining !== null && deadlineRemaining < 300 ? "text-danger" : "text-muted",
                )}
              >
                {deadlineRemaining === null
                  ? "—"
                  : deadlineRemaining <= 0
                    ? "已到期"
                    : formatDuration(deadlineRemaining)}
              </span>
            </div>

            <div className="mt-3 grid grid-cols-2 gap-1.5">
              {(["start", "pause", "resume", "step", "abort", "recover"] as ControlAction[]).map((action) => {
                const meta = controlMeta[action];
                const Icon = meta.icon;
                const allowed = canControl(action);
                const busy = pendingAction === action;
                return (
                  <Button
                    key={action}
                    variant={meta.danger ? "outline" : action === "start" || action === "resume" ? "primary" : "secondary"}
                    size="sm"
                    className={cn("justify-start", meta.danger && "text-danger hover:border-danger/50 hover:bg-danger-soft")}
                    disabled={!allowed || (pendingAction !== null && !busy)}
                    loading={busy}
                    leftIcon={<Icon className="h-3.5 w-3.5" />}
                    title={!allowed ? `当前状态（${sessionStatusLabel[session.status]}）不可执行` : meta.label}
                    onClick={() => void runControl(action)}
                  >
                    {meta.label}
                  </Button>
                );
              })}
            </div>

            <div className="mt-3 rounded-lg border border-line/60 bg-paper/40 px-2.5 py-2 text-[10px] text-faint">
              按钮会按 9 态状态机动态禁用；非法迁移会返回 409。
            </div>
          </section>

          <section
            className={cn(
              "surface-card p-4 transition-colors",
              session.status === "waiting_input" && "border-info/50 shadow-float",
            )}
          >
            <div className="flex items-center justify-between gap-2">
              <h2 className="font-display text-sm font-semibold text-ink">{zh.sandbox.input}</h2>
              {session.status === "waiting_input" ? <Badge tone="info" dot>{zh.sandbox.waitingInput}</Badge> : null}
            </div>
            <Textarea
              value={inputText}
              onChange={(e) => setInputText(e.target.value.slice(0, 2000))}
              rows={4}
              disabled={session.status !== "waiting_input"}
              placeholder={
                session.status === "waiting_input"
                  ? zh.sandbox.inputPlaceholder
                  : "仅在「等待输入」状态可以注入指令"
              }
              className="mt-2 text-xs"
            />
            <div className="mt-1.5 flex items-center justify-between text-[10px] text-faint">
              <span>{inputText.length} / 2000</span>
              <span>{session.status === "waiting_input" ? "导演正在等你的回应" : "输入框已锁定"}</span>
            </div>
            {suggestions.length > 0 && session.status === "waiting_input" ? (
              <div className="mt-2 flex flex-wrap gap-1">
                {suggestions.map((suggestion) => (
                  <button
                    key={suggestion}
                    type="button"
                    onClick={() => setInputText(suggestion)}
                    className="focus-ring rounded-full border border-info/25 bg-info-soft/40 px-2 py-0.5 text-[10px] text-info hover:bg-info-soft"
                  >
                    {suggestion}
                  </button>
                ))}
              </div>
            ) : null}
            <Button
              variant="primary"
              size="sm"
              className="mt-2 w-full"
              leftIcon={<Send className="h-3.5 w-3.5" />}
              loading={sendingInput}
              disabled={session.status !== "waiting_input" || !inputText.trim()}
              onClick={() => void submitInput()}
            >
              {zh.sandbox.send}
            </Button>
          </section>

          <section className="surface-card p-4">
            <h2 className="flex items-center gap-1.5 font-display text-sm font-semibold text-ink">
              <Coins className="h-3.5 w-3.5 text-accent" /> 成本
            </h2>
            <p className="mt-2 font-display text-xl font-semibold text-ink">
              {formatCost(session.usage.cost_usd)}
            </p>
            <div className="mt-2 grid grid-cols-2 gap-2 text-[10px] text-muted">
              <div className="rounded-lg bg-paper/50 px-2 py-1.5">
                <span className="block text-faint">调用</span>
                <span className="font-mono text-xs text-ink">{formatNumber(session.usage.calls)}</span>
              </div>
              <div className="rounded-lg bg-paper/50 px-2 py-1.5">
                <span className="block text-faint">Tokens</span>
                <span className="font-mono text-xs text-ink">
                  {formatNumber(session.usage.prompt_tokens + session.usage.completion_tokens)}
                </span>
              </div>
            </div>
            <div className="mt-2 space-y-1">
              {session.usage.by_purpose.length === 0 ? (
                <p className="text-[10px] text-faint">还没有调用记录</p>
              ) : (
                session.usage.by_purpose.map((item) => (
                  <div key={item.purpose} className="flex items-center justify-between text-[10px]">
                    <span className="truncate text-muted">{item.purpose}</span>
                    <span className="font-mono text-faint">{item.calls}</span>
                  </div>
                ))
              )}
            </div>
            <p className="mt-2 flex items-center gap-1 text-[10px] text-faint">
              <Cpu className="h-3 w-3" /> 每 30 秒刷新快照
            </p>
          </section>

          {session.error ? (
            <section className="rounded-xl border border-danger/30 bg-danger-soft/50 p-3">
              <p className="text-[11px] font-semibold text-danger">{zh.sandbox.sessionError}</p>
              <p className="mt-1 font-mono text-[10px] text-danger/90">{session.error.code}</p>
              <p className="mt-1 text-[11px] leading-5 text-danger">{session.error.message}</p>
            </section>
          ) : null}
        </aside>

        <section className="order-2 min-w-0">
          <div className="surface-card relative flex h-[62vh] min-h-[420px] flex-col overflow-hidden lg:h-[calc(100dvh-11.5rem)]">
            <header className="flex flex-wrap items-center justify-between gap-2 border-b border-line/70 px-3.5 py-2.5">
              <div className="flex items-center gap-2">
                <Terminal className="h-3.5 w-3.5 text-accent" />
                <h2 className="font-display text-sm font-semibold text-ink">{zh.sandbox.events}</h2>
                <span className="text-[10px] text-faint">{events.length} / 500</span>
              </div>
              <div className="flex items-center gap-1.5">
                <button
                  type="button"
                  onClick={() => setAutoScroll(!autoScroll)}
                  className={cn(
                    "focus-ring rounded-md px-2 py-1 text-[10px] transition-colors",
                    autoScroll ? "bg-accent-soft text-accent-strong dark:text-accent" : "text-faint hover:bg-line/30",
                  )}
                  aria-pressed={autoScroll}
                >
                  {zh.sandbox.autoScroll}
                </button>
                {!autoScroll ? (
                  <button
                    type="button"
                    onClick={scrollToLatest}
                    className="focus-ring inline-flex items-center gap-1 rounded-md bg-ink px-2 py-1 text-[10px] text-paper dark:bg-raised dark:text-ink"
                  >
                    {zh.sandbox.scrollToLatest} <ArrowDownIcon />
                  </button>
                ) : null}
              </div>
            </header>

            <div
              ref={scrollRef}
              onScroll={onScroll}
              className="relative flex-1 space-y-4 overflow-y-auto px-3.5 py-3"
              aria-live="polite"
              aria-label="沙盒事件流"
              tabIndex={0}
            >
              {events.length === 0 ? (
                <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
                  <Spinner label={zh.sandbox.noEvents} />
                  {connection === "reconnecting" ? (
                    <p className="text-[10px] text-faint">连接正在重建，已发送的事件会按 seq 补发。</p>
                  ) : null}
                </div>
              ) : (
                groups.map((group) => {
                  const stepStart = group.events.find((event) => event.event_type === "step_started");
                  const stepActor = stepStart ? asString(stepStart.payload.actor_id) : "";
                  return (
                    <section key={group.key} className="space-y-2">
                      {group.step !== null ? (
                        <div className="sticky top-0 z-10 -mx-1 flex items-center gap-2 rounded-lg bg-paper/90 px-1 py-1 backdrop-blur-sm">
                          <span className="rounded-md border border-line/70 bg-raised px-1.5 py-0.5 font-mono text-[10px] text-muted">
                            STEP {group.step}
                          </span>
                          {stepActor ? (
                            <span className={cn("text-[10px]", actorTone(stepActor).text)}>
                              {actorName(stepActor)} 开始行动
                            </span>
                          ) : null}
                          <span className="h-px flex-1 bg-line/60" />
                        </div>
                      ) : null}
                      {group.events.map(renderEvent)}
                    </section>
                  );
                })
              )}
            </div>

            {!autoScroll && events.length > 0 ? (
              <div className="pointer-events-none absolute bottom-3 left-1/2 -translate-x-1/2">
                <span className="rounded-full bg-ink/80 px-2 py-1 text-[10px] text-paper shadow-float">
                  自动滚动已暂停
                </span>
              </div>
            ) : null}
          </div>
        </section>

        <aside className="order-3 space-y-3 lg:col-span-2 xl:col-span-1 xl:sticky xl:top-[4.8rem] xl:self-start">
          <section className="surface-card overflow-hidden">
            <header className="flex items-center justify-between gap-2 border-b border-line/70 px-3.5 py-2.5">
              <h2 className="flex items-center gap-1.5 font-display text-sm font-semibold text-ink">
                <CornerDownLeft className="h-3.5 w-3.5 text-accent" /> {zh.sandbox.outline}
              </h2>
              {outline ? (
                <Button
                  size="sm"
                  variant="primary"
                  leftIcon={<Save className="h-3.5 w-3.5" />}
                  loading={outlineSaving}
                  onClick={() => void saveOutline()}
                >
                  {outlineSaved ? "已保存" : zh.sandbox.saveOutline}
                </Button>
              ) : null}
            </header>
            <div className="max-h-[calc(100dvh-18rem)] min-h-[220px] overflow-y-auto p-4">
              {outline ? (
                <>
                  <MarkdownPreview source={outline} />
                  <p className="mt-3 border-t border-line/60 pt-2 text-[10px] text-faint">
                    {outlineChapterId || session.chapter_id
                      ? zh.sandbox.saveOutlineHint
                      : "探索会话不会生成章节大纲，可在章节沙盒中收束定稿。"}
                  </p>
                </>
              ) : (
                <div className="flex h-full min-h-[180px] flex-col items-center justify-center text-center">
                  <p className="text-sm text-muted">{zh.sandbox.outlineEmpty}</p>
                  {session.chapter_id ? (
                    <p className="mt-1 text-[10px] text-faint">关联章节会在大纲生成后出现在这里。</p>
                  ) : (
                    <p className="mt-1 text-[10px] text-faint">当前是探索会话，不会写回章节。</p>
                  )}
                </div>
              )}
            </div>
          </section>

          {fallbackEvents.length > 0 ? (
            <section className="rounded-xl border border-warning/35 bg-warning-soft/60 p-3">
              <p className="flex items-center gap-1.5 text-[11px] font-semibold text-warning">
                <AlertTriangle className="h-3.5 w-3.5" /> 导演降级 {fallbackEvents.length} 次
              </p>
              <p className="mt-1 text-[10px] leading-5 text-warning/90">
                导演输出曾不可用并降级为轮转。可以调整场景提示或模型配置后重新推演。
              </p>
              <Button
                size="sm"
                variant="outline"
                className="mt-2 border-warning/40 text-warning hover:bg-warning-soft"
                onClick={() => navigate(`/novel/${session.novel_id}/sandbox?new=1`)}
              >
                重新发起推演
              </Button>
            </section>
          ) : null}

          {streamError ? (
            <section className="rounded-xl border border-danger/25 bg-danger-soft/40 p-3">
              <p className="text-[11px] font-medium text-danger">事件流提示</p>
              <p className="mt-1 text-[10px] leading-5 text-danger/90">{streamError}</p>
              {connection !== "open" && session.id ? (
                <Button
                  size="sm"
                  variant="outline"
                  className="mt-2 border-danger/30 text-danger"
                  onClick={() => {
                    const maxSeq = events.reduce((max, event) => Math.max(max, event.seq), 0);
                    connect(session.id, maxSeq);
                  }}
                >
                  重新连接
                </Button>
              ) : null}
            </section>
          ) : null}
        </aside>
      </div>

      <Dialog
        open={blocker.state === "blocked"}
        onClose={() => blocker.reset?.()}
        title={zh.sandbox.leaveTitle}
        description={zh.sandbox.leaveHint}
        size="sm"
        footer={
          <>
            <Button variant="ghost" onClick={() => blocker.reset?.()} disabled={leaveBusy !== null}>
              {zh.sandbox.leaveStay}
            </Button>
            <Button
              variant="outline"
              onClick={() => void leave("background")}
              disabled={leaveBusy !== null}
            >
              {zh.sandbox.leaveBackground}
            </Button>
            {session.status === "running" ? (
              <Button
                variant="secondary"
                loading={leaveBusy === "pause"}
                onClick={() => void leave("pause")}
                disabled={leaveBusy !== null}
              >
                {zh.sandbox.leavePause}
              </Button>
            ) : null}
            <Button
              variant="danger"
              loading={leaveBusy === "abort"}
              onClick={() => void leave("abort")}
              disabled={leaveBusy !== null}
            >
              {zh.sandbox.leaveAbort}
            </Button>
          </>
        }
      >
        <div className="space-y-2 text-sm text-muted">
          <p>
            当前状态：<span className="font-medium text-ink">{sessionStatusLabel[session.status]}</span> ·
            第 {session.current_step}/{session.step_limit} 步
          </p>
          <p className="text-xs leading-6">
            沙盒引擎在服务端运行。关闭页面或离开不会自动终止会话，但你可以在这里选择暂停或中止。
          </p>
        </div>
      </Dialog>
    </div>
  );
}

function ArrowDownIcon() {
  return <ArrowRight className="h-3 w-3 rotate-90" />;
}

function SparklesInline() {
  return <CircleDot className="mr-1 inline h-3 w-3" />;
}
