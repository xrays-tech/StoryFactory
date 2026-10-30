import type { SandboxEventType } from "../types/domain";
import { authHeaders, newRequestId } from "./client";
import { eventsUrl } from "./endpoints";

/**
 * 说明：原生 EventSource 不能设置 X-App-Token / Last-Event-ID，也无法监听
 * 未知的命名事件（未知 event 不会进入 onmessage）。这里实现一个
 * EventSource 语义兼容的 fetch 流客户端，保留 after_seq、Last-Event-ID、
 * 指数退避、45s 假死重建、unknown_event 兜底和 sandbox.done 关闭。
 */

export type SandboxEventName = SandboxEventType | "sandbox.done" | "unknown_event";

export type SandboxEvent = {
  seq: number;
  event_type: SandboxEventName;
  original_type: string;
  payload: Record<string, unknown>;
  received_at: number;
  unknown?: boolean;
};

export type StreamState = "idle" | "connecting" | "open" | "reconnecting" | "closed" | "error";

export type SandboxEventHandlers = {
  onEvent?: (event: SandboxEvent) => void;
  onUnknown?: (event: SandboxEvent) => void;
  onOpen?: () => void;
  onDone?: () => void;
  onError?: (error: unknown) => void;
  onReconnect?: (attempt: number) => void;
  onState?: (state: StreamState, attempt: number) => void;
};

export type SandboxEventStreamOptions = {
  afterSeq?: number;
  heartbeatTimeoutMs?: number;
  onReconnect?: (attempt: number) => void;
};

const KNOWN_EVENTS = new Set<string>([
  "session_started",
  "session_paused",
  "session_resumed",
  "director_decision",
  "director_fallback",
  "step_started",
  "actor_action",
  "actor_error",
  "emotion_updated",
  "rolling_summary_applied",
  "waiting_input",
  "user_input_received",
  "finalize_started",
  "outline_ready",
  "session_completed",
  "session_failed",
  "session_aborted",
  "session_interrupted",
  "sandbox.done",
]);

const BACKOFF_MS = [1_000, 2_000, 4_000, 8_000, 16_000, 30_000];
const HEARTBEAT_TIMEOUT_MS = 45_000;

type Frame = { id?: string; event?: string; data: string };

export class SandboxEventStream {
  private readonly sessionId: string;
  private readonly handlers: SandboxEventHandlers;
  private readonly heartbeatTimeoutMs: number;
  private readonly externalReconnect?: (attempt: number) => void;

  private controller: AbortController | null = null;
  private reconnectTimer: number | null = null;
  private watchdogTimer: number | null = null;
  private closed = false;
  private done = false;
  private attempt = 0;
  private lastSeq: number;
  private localSeq = 0;
  private state: StreamState = "idle";

  constructor(
    sessionId: string,
    handlers: SandboxEventHandlers = {},
    options: SandboxEventStreamOptions = {},
  ) {
    this.sessionId = sessionId;
    this.handlers = handlers;
    this.lastSeq = Math.max(0, options.afterSeq ?? 0);
    this.heartbeatTimeoutMs = options.heartbeatTimeoutMs ?? HEARTBEAT_TIMEOUT_MS;
    this.externalReconnect = options.onReconnect;
  }

  getLastSeq(): number {
    return this.lastSeq;
  }

  getState(): StreamState {
    return this.state;
  }

  start(): void {
    if (!this.closed) {
      this.done = false;
      void this.connect();
    }
  }

  /** 关闭流；可以传入 true 表示会话已终结，不再重连。 */
  close(final = false): void {
    this.closed = true;
    if (final) this.done = true;
    this.clearReconnect();
    this.clearWatchdog();
    this.setState(final ? "closed" : "closed");
    this.controller?.abort();
    this.controller = null;
  }

  private setState(state: StreamState): void {
    this.state = state;
    this.handlers.onState?.(state, this.attempt);
  }

  private clearReconnect(): void {
    if (this.reconnectTimer !== null) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  private clearWatchdog(): void {
    if (this.watchdogTimer !== null) {
      window.clearTimeout(this.watchdogTimer);
      this.watchdogTimer = null;
    }
  }

  private armWatchdog(): void {
    this.clearWatchdog();
    this.watchdogTimer = window.setTimeout(() => {
      // 45 秒没有任何字节（包括 ping 注释）视为假死，主动断开重建。
      this.controller?.abort();
      this.controller = null;
      if (!this.closed && !this.done) this.scheduleReconnect();
    }, this.heartbeatTimeoutMs);
  }

  private scheduleReconnect(): void {
    if (this.closed || this.done) return;
    if (this.reconnectTimer !== null) return;
    this.attempt += 1;
    this.setState("reconnecting");
    this.handlers.onReconnect?.(this.attempt);
    this.externalReconnect?.(this.attempt);
    const delay = BACKOFF_MS[Math.min(this.attempt - 1, BACKOFF_MS.length - 1)];
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      void this.connect();
    }, delay);
  }

  private async connect(): Promise<void> {
    if (this.closed || this.done) return;
    this.clearReconnect();
    this.clearWatchdog();
    this.controller?.abort();
    const controller = new AbortController();
    this.controller = controller;
    this.setState(this.attempt > 0 ? "reconnecting" : "connecting");
    this.armWatchdog();

    const headers = authHeaders({
      Accept: "text/event-stream",
      "Cache-Control": "no-cache",
      "X-Request-Id": newRequestId(),
    });
    if (this.lastSeq > 0) headers["Last-Event-ID"] = String(this.lastSeq);

    let response: Response;
    try {
      response = await fetch(eventsUrl(this.sessionId, this.lastSeq), {
        method: "GET",
        headers,
        signal: controller.signal,
        cache: "no-store",
      });
    } catch (error) {
      if (this.closed || this.done || controller.signal.aborted) {
        if (!this.closed && !this.done && this.controller === controller) {
          this.scheduleReconnect();
        }
        return;
      }
      this.handlers.onError?.(error);
      this.setState("error");
      this.scheduleReconnect();
      return;
    }

    if (!response.ok || !response.body) {
      const error = new Error(`SSE 连接失败 (${response.status})`);
      this.handlers.onError?.(error);
      this.setState("error");
      this.scheduleReconnect();
      return;
    }

    this.attempt = 0;
    this.setState("open");
    this.handlers.onOpen?.();

    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    try {
      while (!this.closed && !this.done) {
        const { value, done } = await reader.read();
        if (done) break;
        this.armWatchdog();
        buffer += decoder.decode(value, { stream: true });
        buffer = buffer.replaceAll("\r\n", "\n");
        let boundary = buffer.indexOf("\n\n");
        while (boundary >= 0) {
          const raw = buffer.slice(0, boundary);
          buffer = buffer.slice(boundary + 2);
          const frame = parseFrame(raw);
          if (frame) this.dispatch(frame);
          if (this.done || this.closed) break;
          boundary = buffer.indexOf("\n\n");
        }
      }
      if (buffer.trim() && !this.done && !this.closed) {
        const frame = parseFrame(buffer);
        if (frame) this.dispatch(frame);
      }
    } catch (error) {
      if (!this.closed && !this.done && !controller.signal.aborted) {
        this.handlers.onError?.(error);
        this.setState("error");
      }
    } finally {
      reader.releaseLock();
    }

    this.clearWatchdog();
    if (this.controller === controller) this.controller = null;
    if (!this.closed && !this.done) this.scheduleReconnect();
  }

  private dispatch(frame: Frame): void {
    const name = frame.event || "message";
    if (name === "sandbox.done") {
      this.done = true;
      this.handlers.onDone?.();
      this.close(true);
      return;
    }

    const seqFromId = Number(frame.id);
    const seq = Number.isFinite(seqFromId) && seqFromId > 0 ? seqFromId : this.localSeq + 1;
    this.localSeq = Math.max(this.localSeq, seq);
    if (seq > 0) this.lastSeq = Math.max(this.lastSeq, seq);

    let payload: Record<string, unknown> = {};
    if (frame.data) {
      try {
        const parsed: unknown = JSON.parse(frame.data);
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
          payload = parsed as Record<string, unknown>;
        } else {
          payload = { value: parsed };
        }
      } catch {
        payload = { raw: frame.data };
      }
    }

    const unknown = !KNOWN_EVENTS.has(name);
    const event: SandboxEvent = {
      seq,
      event_type: (unknown ? "unknown_event" : name) as SandboxEventName,
      original_type: name,
      payload,
      received_at: Date.now(),
      unknown,
    };
    if (unknown) this.handlers.onUnknown?.(event);
    this.handlers.onEvent?.(event);
  }
}

function parseFrame(raw: string): Frame | null {
  if (!raw.trim()) return null;
  let id: string | undefined;
  let event: string | undefined;
  const data: string[] = [];
  for (const line of raw.split("\n")) {
    if (!line || line.startsWith(":")) continue;
    const colon = line.indexOf(":");
    const field = colon >= 0 ? line.slice(0, colon) : line;
    let value = colon >= 0 ? line.slice(colon + 1) : "";
    if (value.startsWith(" ")) value = value.slice(1);
    if (field === "id") id = value;
    else if (field === "event") event = value;
    else if (field === "data") data.push(value);
  }
  if (!id && !event && data.length === 0) return null;
  return { id, event, data: data.join("\n") };
}
