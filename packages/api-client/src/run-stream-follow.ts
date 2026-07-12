import { ApiError } from "./api-error";
import { DecodeError } from "./decode-error";
import {
  createRunStreamProjection,
  disconnectRunStream,
  reduceRunStream,
} from "./run-stream-reducer";
import { RunStreamProtocolError } from "./stream-error";
import type { RunEventStreamObserver } from "./http-sse";
import type { RunStreamConnectionState, RunStreamProjection } from "./stream-types";

export interface RunEventStreamClient {
  streamRunEvents(
    runId: string,
    afterDurableSeq: number,
    signal: AbortSignal,
    observer: RunEventStreamObserver,
  ): Promise<void>;
}

export interface FollowRunEventsOptions {
  initialDurableSeq?: number;
  backoffBaseMs?: number;
  backoffMaxMs?: number;
  random?: () => number;
  onProjection: (projection: RunStreamProjection) => void;
  onConnection: (state: RunStreamConnectionState, error?: Error) => void;
}

export async function followRunEvents(
  client: RunEventStreamClient,
  runId: string,
  signal: AbortSignal,
  options: FollowRunEventsOptions,
): Promise<void> {
  let projection = createRunStreamProjection(runId, options.initialDurableSeq ?? 0);
  let reconnectAttempt = 0;
  options.onProjection(projection);
  while (!signal.aborted && !projection.terminalStatus) {
    const connection = new AbortController();
    const abort = () => connection.abort();
    signal.addEventListener("abort", abort, { once: true });
    options.onConnection(reconnectAttempt === 0 ? "connecting" : "reconnecting");
    try {
      await client.streamRunEvents(runId, projection.durableSeq, connection.signal, {
        onOpen: () => {
          reconnectAttempt = 0;
          options.onConnection("live");
        },
        onMessage: (message) => {
          projection = reduceRunStream(projection, message);
          options.onProjection(projection);
          if (projection.terminalStatus) connection.abort();
        },
      });
      if (projection.terminalStatus) break;
    } catch (cause) {
      if (projection.terminalStatus || signal.aborted) break;
      const error = asError(cause);
      if (!isRetryable(error)) {
        projection = disconnectRunStream(projection);
        options.onProjection(projection);
        options.onConnection("incompatible", error);
        throw error;
      }
    } finally {
      signal.removeEventListener("abort", abort);
    }
    if (signal.aborted || projection.terminalStatus) break;
    projection = disconnectRunStream(projection);
    options.onProjection(projection);
    reconnectAttempt += 1;
    options.onConnection("reconnecting");
    await backoff(reconnectAttempt, signal, options);
  }
  options.onConnection("closed");
}

const isRetryable = (error: Error) => {
  if (error instanceof DecodeError || error instanceof RunStreamProtocolError) return false;
  if (error instanceof ApiError) {
    return error.body.retryable || error.status === 408 || error.status === 429 || error.status >= 500;
  }
  return !isAbort(error);
};

const backoff = (
  attempt: number,
  signal: AbortSignal,
  options: FollowRunEventsOptions,
) => {
  const base = options.backoffBaseMs ?? 250;
  const maximum = options.backoffMaxMs ?? 8_000;
  const random = options.random ?? Math.random;
  const raw = Math.min(maximum, base * (2 ** Math.min(attempt - 1, 8)));
  const milliseconds = Math.round(raw * (0.8 + random() * 0.4));
  return wait(milliseconds, signal);
};

const wait = (milliseconds: number, signal: AbortSignal) =>
  new Promise<void>((resolve, reject) => {
    if (signal.aborted) {
      reject(new DOMException("Aborted", "AbortError"));
      return;
    }
    const abort = () => {
      clearTimeout(timeout);
      reject(new DOMException("Aborted", "AbortError"));
    };
    const timeout = setTimeout(() => {
      signal.removeEventListener("abort", abort);
      resolve();
    }, milliseconds);
    signal.addEventListener("abort", abort, { once: true });
  });

const asError = (cause: unknown): Error =>
  cause instanceof Error ? cause : new Error("run event stream failed");

const isAbort = (error: Error) => error.name === "AbortError";
