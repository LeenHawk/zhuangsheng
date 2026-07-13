import { decodeRunStreamFrame } from "./stream-decode";
import { stringifyJsonExact } from "./exact-json";
import type { RunEventStreamObserver } from "./http-sse";

export interface TransportRequest<TPayload = unknown> {
  operation: string;
  payload: TPayload;
}

export interface RuntimeTransport {
  query<TResult>(request: TransportRequest, signal?: AbortSignal): Promise<TResult>;
  command<TResult>(request: TransportRequest, signal?: AbortSignal): Promise<TResult>;
  subscribeRun(
    runId: string,
    afterDurableSeq: number,
    signal: AbortSignal,
    observer: RunEventStreamObserver,
  ): Promise<void>;
}

export interface PlatformCapabilities {
  platform: "web" | "desktop" | "mobile";
  localFirst: boolean;
  filePicker: boolean;
  nativeNotifications: boolean;
  openExternal: (url: string) => Promise<void>;
}

export interface TauriBridge {
  invoke<TResult>(operation: string, payload: unknown): Promise<TResult>;
  listen(event: string, handler: () => void): Promise<() => void>;
}

interface DurableEventWire {
  id: string; runId: string; durableSeq: number; type: string; schemaVersion: number;
  timestamp: number; nodeInstanceId: string | null; attemptId: string | null;
  importance: string; payload: unknown;
}

export class TauriTransport implements RuntimeTransport {
  constructor(private readonly bridge: TauriBridge) {}

  query<TResult>(request: TransportRequest, signal?: AbortSignal): Promise<TResult> {
    return this.invoke(request, signal);
  }

  command<TResult>(request: TransportRequest, signal?: AbortSignal): Promise<TResult> {
    return this.invoke(request, signal);
  }

  async subscribeRun(runId: string, afterDurableSeq: number, signal: AbortSignal, observer: RunEventStreamObserver): Promise<void> {
    let cursor = afterDurableSeq;
    let draining = false;
    let pending = true;
    let wake: (() => void) | null = null;
    const drain = async () => {
      if (draining) { pending = true; return; }
      draining = true;
      try {
        do {
          pending = false;
          const events = await this.bridge.invoke<DurableEventWire[]>("list_run_events", {
            runId, afterDurableSeq: cursor, limit: 500,
          });
          for (const event of events) {
            if (event.durableSeq <= cursor) continue;
            const message = decodeRunStreamFrame({
              id: String(event.durableSeq), event: event.type, data: stringifyJsonExact(event),
            }, runId);
            observer.onMessage(message);
            cursor = event.durableSeq;
          }
          if (events.length === 500) pending = true;
        } while (pending && !signal.aborted);
      } finally { draining = false; }
    };
    observer.onOpen();
    const unlisten = await this.bridge.listen("zhuangsheng://run-events", () => { pending = true; wake?.(); });
    try {
      await drain();
      while (!signal.aborted) {
        await new Promise<void>((resolve) => {
          const finish = () => { signal.removeEventListener("abort", finish); resolve(); };
          wake = finish;
          signal.addEventListener("abort", finish, { once: true });
          if (pending) finish();
        });
        wake = null;
        if (!signal.aborted) await drain();
      }
    } finally { unlisten(); }
  }

  private async invoke<TResult>(request: TransportRequest, signal?: AbortSignal): Promise<TResult> {
    if (signal?.aborted) throw new DOMException("Aborted", "AbortError");
    const result = this.bridge.invoke<TResult>(request.operation, request.payload);
    if (!signal) return result;
    return Promise.race([
      result,
      new Promise<never>((_, reject) => signal.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true })),
    ]);
  }
}

export const webPlatformCapabilities: PlatformCapabilities = {
  platform: "web", localFirst: false, filePicker: true, nativeNotifications: false,
  openExternal: async (url) => { window.open(url, "_blank", "noopener,noreferrer"); },
};
