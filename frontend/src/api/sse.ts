import type { SandboxEventType } from "../types/domain";
import { eventsUrl } from "./endpoints";
import { EventStream, type EventFrame, type StreamState } from "./eventStream";

/**
 * 沙盒事件流是通用 EventStream 的薄包装：保留事件序号、未知事件兜底、
 * sandbox.done 终止和 after_seq 断点续连，行为与重构前一致。
 */

export type { StreamState } from "./eventStream";

export type SandboxEventName = SandboxEventType | "sandbox.done" | "unknown_event";

export type SandboxEvent = {
  seq: number;
  event_type: SandboxEventName;
  original_type: string;
  payload: Record<string, unknown>;
  received_at: number;
  unknown?: boolean;
};

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

export class SandboxEventStream {
  private readonly sessionId: string;
  private readonly handlers: SandboxEventHandlers;
  private readonly stream: EventStream;
  private lastSeq: number;
  private localSeq = 0;

  constructor(
    sessionId: string,
    handlers: SandboxEventHandlers = {},
    options: SandboxEventStreamOptions = {},
  ) {
    this.sessionId = sessionId;
    this.handlers = handlers;
    this.lastSeq = Math.max(0, options.afterSeq ?? 0);
    this.stream = new EventStream({
      url: () => eventsUrl(this.sessionId, this.lastSeq),
      lastEventId: this.lastSeq > 0 ? String(this.lastSeq) : "",
      knownEvents: KNOWN_EVENTS,
      terminalEvents: ["sandbox.done"],
      heartbeatTimeoutMs: options.heartbeatTimeoutMs,
      onState: (state, attempt) => handlers.onState?.(state, attempt),
      onOpen: () => handlers.onOpen?.(),
      onReconnect: (attempt) => {
        handlers.onReconnect?.(attempt);
        options.onReconnect?.(attempt);
      },
      onError: (error) => handlers.onError?.(error),
      onDone: () => handlers.onDone?.(),
      onEvent: (name, _data, frame, known) => this.dispatch(name, frame, known),
    });
  }

  getLastSeq(): number {
    return this.lastSeq;
  }

  getState(): StreamState {
    return this.stream.getState();
  }

  start(): void {
    this.stream.start();
  }

  /** 关闭流；可以传入 true 表示会话已终结，不再重连。 */
  close(final = false): void {
    this.stream.close(final);
  }

  private dispatch(name: string, frame: EventFrame, known: boolean): void {
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

    const event: SandboxEvent = {
      seq,
      event_type: (known ? name : "unknown_event") as SandboxEventName,
      original_type: name,
      payload,
      received_at: Date.now(),
      unknown: !known,
    };
    if (!known) this.handlers.onUnknown?.(event);
    this.handlers.onEvent?.(event);
  }
}
