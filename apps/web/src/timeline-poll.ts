import type { ConversationTimelineView } from "@zhuangsheng/api-client";

import { client } from "./api";

export async function pollTimeline(
  conversationId: string,
  runId: string,
  signal: AbortSignal,
  update: (timeline: ConversationTimelineView) => void,
) {
  for (let attempt = 0; attempt < 21; attempt += 1) {
    if (attempt > 0) await delay(500, signal);
    const next = await client.getTimeline(conversationId, signal);
    if (signal.aborted) return;
    update(next);
    const candidate = next.turns.flatMap((turn) => turn.candidates).find((item) => item.runId === runId);
    if (!candidate || candidate.status !== "running") return;
  }
}

const delay = (milliseconds: number, signal: AbortSignal) =>
  new Promise<void>((resolve, reject) => {
    if (signal.aborted) {
      reject(new DOMException("Aborted", "AbortError"));
      return;
    }
    const abort = () => {
      window.clearTimeout(timeout);
      reject(new DOMException("Aborted", "AbortError"));
    };
    const timeout = window.setTimeout(() => {
      signal.removeEventListener("abort", abort);
      resolve();
    }, milliseconds);
    signal.addEventListener("abort", abort, { once: true });
  });

export const isAbort = (cause: unknown) =>
  cause instanceof DOMException && cause.name === "AbortError";
