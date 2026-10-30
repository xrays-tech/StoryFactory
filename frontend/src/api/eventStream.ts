import { authHeaders, newRequestId } from "./client";

/**
 * 通用 SSE 客户端。
 *
 * 原生 EventSource 不能设置 X-App-Token / Last-Event-ID，也无法监听未知命名事件，
 * 因此这里用 fetch + ReadableStream 实现 EventSource 语义：
 * - 指数退避重连（1/2/4/8/16/30s）
 * - 45s 无字节看门狗（包括 `: ping` 注释行）
 * - AbortController 防止连接重叠
 * - 未知事件仍然交给调用方兜底
 */

export type StreamState = "idle" | "connecting" | "open" | "reconnecting" | "closed" | "error";

export type EventFrame = {
  id?: string;
  event?: string;
  data: string;
};

export type EventStreamConfig = {
  /** 静态地址，或按 Last-Event-ID 动态生成的地址。 */
  url: string | ((lastEventId: string) => string);
  /** 额外请求头，会与认证头合并。 */
  headers?: Record<string, string> | ((lastEventId: string) => Record<string, string>);
  /** 断点续连的初始 Last-Event-ID。 */
  lastEventId?: string;
  /** 已知事件名；未知事件仍会触发 onEvent，但 known=false。 */
  knownEvents?: ReadonlySet<string> | readonly string[];
  /** 终态事件名；收到后触发 onDone 并关闭流。 */
  terminalEvents?: ReadonlySet<string> | readonly string[];
  heartbeatTimeoutMs?: number;
  onEvent?: (name: string, data: unknown, frame: EventFrame, known: boolean) => void;
  onDone?: (name: string, data: unknown, frame: EventFrame) => void;
  onState?: (state: StreamState, attempt: number) => void;
  onOpen?: () => void;
  onReconnect?: (attempt: number) => void;
  onError?: (error: unknown) => void;
};

const BACKOFF_MS = [1_000, 2_000, 4_000, 8_000, 16_000, 30_000];
const HEARTBEAT_TIMEOUT_MS = 45_000;

function toSet(source: ReadonlySet<string> | readonly string[] | undefined): ReadonlySet<string> {
  if (!source) return new Set<string>();
  return source instanceof Set ? source : new Set(source);
}

function parseFrame(raw: string): EventFrame | null {
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

function parseData(raw: string): unknown {
  if (!raw) return undefined;
  try {
    return JSON.parse(raw) as unknown;
  } catch {
    return raw;
  }
}

export class EventStream {
  private readonly config: EventStreamConfig;
  private readonly knownEvents: ReadonlySet<string>;
  private readonly terminalEvents: ReadonlySet<string>;
  private readonly heartbeatTimeoutMs: number;

  private controller: AbortController | null = null;
  private reconnectTimer: number | null = null;
  private watchdogTimer: number | null = null;
  private closed = false;
  private done = false;
  private attempt = 0;
  private lastEventId: string;
  private state: StreamState = "idle";

  constructor(config: EventStreamConfig) {
    this.config = config;
    this.knownEvents = toSet(config.knownEvents);
    this.terminalEvents = toSet(config.terminalEvents);
    this.heartbeatTimeoutMs = config.heartbeatTimeoutMs ?? HEARTBEAT_TIMEOUT_MS;
    this.lastEventId = config.lastEventId ?? "";
  }

  getLastEventId(): string {
    return this.lastEventId;
  }

  getState(): StreamState {
    return this.state;
  }

  start(): void {
    if (this.closed) return;
    this.done = false;
    void this.connect();
  }

  /** 关闭流；传入 true 表示已到终态，不再重连。 */
  close(final = false): void {
    this.closed = true;
    if (final) this.done = true;
    this.clearReconnect();
    this.clearWatchdog();
    this.setState("closed");
    this.controller?.abort();
    this.controller = null;
  }

  private setState(state: StreamState): void {
    this.state = state;
    this.config.onState?.(state, this.attempt);
  }

  private clearReconnect(): void {
    if (this.reconnectTimer === null) return;
    window.clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
  }

  private clearWatchdog(): void {
    if (this.watchdogTimer === null) return;
    window.clearTimeout(this.watchdogTimer);
    this.watchdogTimer = null;
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
    this.config.onReconnect?.(this.attempt);
    const delay = BACKOFF_MS[Math.min(this.attempt - 1, BACKOFF_MS.length - 1)];
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      void this.connect();
    }, delay);
  }

  private resolveUrl(): string {
    return typeof this.config.url === "function"
      ? this.config.url(this.lastEventId)
      : this.config.url;
  }

  private resolveHeaders(): Record<string, string> {
    const extra =
      typeof this.config.headers === "function"
        ? this.config.headers(this.lastEventId)
        : (this.config.headers ?? {});
    const headers = authHeaders({
      Accept: "text/event-stream",
      "Cache-Control": "no-cache",
      "X-Request-Id": newRequestId(),
      ...extra,
    });
    if (this.lastEventId) headers["Last-Event-ID"] = this.lastEventId;
    return headers;
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

    let response: Response;
    try {
      response = await fetch(this.resolveUrl(), {
        method: "GET",
        headers: this.resolveHeaders(),
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
      this.config.onError?.(error);
      this.setState("error");
      this.scheduleReconnect();
      return;
    }

    if (!response.ok || !response.body) {
      const error = new Error(`SSE 连接失败 (${response.status})`);
      this.config.onError?.(error);
      this.setState("error");
      this.scheduleReconnect();
      return;
    }

    this.attempt = 0;
    this.setState("open");
    this.config.onOpen?.();

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
        this.config.onError?.(error);
        this.setState("error");
      }
    } finally {
      reader.releaseLock();
    }

    this.clearWatchdog();
    if (this.controller === controller) this.controller = null;
    if (!this.closed && !this.done) this.scheduleReconnect();
  }

  private dispatch(frame: EventFrame): void {
    const name = frame.event || "message";
    if (frame.id) this.lastEventId = frame.id;
    const data = parseData(frame.data);

    if (this.terminalEvents.has(name)) {
      this.done = true;
      this.config.onDone?.(name, data, frame);
      this.close(true);
      return;
    }

    this.config.onEvent?.(name, data, frame, this.knownEvents.has(name));
  }
}
