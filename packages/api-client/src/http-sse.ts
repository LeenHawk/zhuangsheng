import { apiErrorFromPayload } from "./api-error";
import { decodeRunStreamFrame } from "./stream-decode";
import { RunStreamProtocolError } from "./stream-error";
import { SseParser } from "./sse-parser";
import type { RunStreamMessage } from "./stream-types";

export interface RunEventStreamObserver {
  onOpen: () => void;
  onMessage: (message: RunStreamMessage) => void;
}

export async function streamRunEvents(
  baseUrl: string,
  runId: string,
  afterDurableSeq: number,
  signal: AbortSignal,
  observer: RunEventStreamObserver,
): Promise<void> {
  const path = `/v1/runs/${encodeURIComponent(runId)}/events?after=${afterDurableSeq}`;
  const response = await fetch(`${baseUrl}${path}`, {
    headers: { accept: "text/event-stream" },
    cache: "no-store",
    signal,
  });
  if (!response.ok) {
    const payload: unknown = await response.json().catch(() => null);
    throw apiErrorFromPayload(response.status, payload);
  }
  const contentType = response.headers.get("content-type") ?? "";
  if (!contentType.toLowerCase().startsWith("text/event-stream")) {
    throw new RunStreamProtocolError("run event response is not text/event-stream");
  }
  if (!response.body) throw new RunStreamProtocolError("run event response has no body");
  observer.onOpen();
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const parser = new SseParser();
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      for (const frame of parser.push(decoder.decode(value, { stream: true }))) {
        observer.onMessage(decodeRunStreamFrame(frame, runId));
      }
    }
    for (const frame of parser.push(decoder.decode())) {
      observer.onMessage(decodeRunStreamFrame(frame, runId));
    }
    for (const frame of parser.finish()) {
      observer.onMessage(decodeRunStreamFrame(frame, runId));
    }
  } catch (cause) {
    await reader.cancel().catch(() => undefined);
    throw cause;
  } finally {
    reader.releaseLock();
  }
}
