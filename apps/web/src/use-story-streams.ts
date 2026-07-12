import { useEffect, useMemo, useState, type Dispatch, type SetStateAction } from "react";

import {
  followRunEvents,
  selectLiveText,
  type ConversationTimelineView,
} from "@zhuangsheng/api-client";
import type { StoryLiveCandidate } from "@zhuangsheng/domain-ui";

import { client, messageFor } from "./api";

export function useStoryStreams(
  conversationId: string,
  timeline: ConversationTimelineView | null,
  setTimeline: Dispatch<SetStateAction<ConversationTimelineView | null>>,
): StoryLiveCandidate[] {
  const runIds = useMemo(() => runningRunIds(timeline), [timeline]);
  const runKey = runIds.join("\0");
  const [views, setViews] = useState<Record<string, StoryLiveCandidate>>({});

  useEffect(() => {
    const ids = runKey ? runKey.split("\0") : [];
    const controllers = ids.map(() => new AbortController());
    const settled = new Set<string>();
    setViews(Object.fromEntries(ids.map((runId) => [runId, initial(runId)])));
    ids.forEach((runId, index) => {
      const signal = controllers[index]!.signal;
      void followRunEvents(client.runtime, runId, signal, {
        onConnection: (connection, error) => {
          updateView(setViews, runId, { connection, error: error ? messageFor(error) : null });
        },
        onProjection: (projection) => {
          updateView(setViews, runId, {
            text: selectLiveText(projection),
            truncated: projection.liveTruncated,
            refreshVersion: projection.refreshVersion,
          });
          if (projection.terminalStatus && !settled.has(runId)) {
            settled.add(runId);
            void settleTimeline(conversationId, runId, signal, setTimeline).catch((cause: unknown) => {
              if (!signal.aborted) updateView(setViews, runId, { error: messageFor(cause) });
            });
          }
        },
      }).catch((cause: unknown) => {
        if (!signal.aborted) updateView(setViews, runId, { error: messageFor(cause) });
      });
    });
    return () => controllers.forEach((controller) => controller.abort());
  }, [conversationId, runKey, setTimeline]);

  return runIds.map((runId) => views[runId] ?? initial(runId));
}

const runningRunIds = (timeline: ConversationTimelineView | null) => {
  const ids = timeline?.turns
    .flatMap((turn) => turn.candidates)
    .filter((candidate) => candidate.status === "running")
    .map((candidate) => candidate.runId) ?? [];
  return [...new Set(ids)];
};

const initial = (runId: string): StoryLiveCandidate => ({
  runId,
  connection: "connecting",
  text: "",
  truncated: false,
  error: null,
  refreshVersion: 0,
});

const updateView = (
  update: Dispatch<SetStateAction<Record<string, StoryLiveCandidate>>>,
  runId: string,
  patch: Partial<StoryLiveCandidate>,
) => update((current) => ({
  ...current,
  [runId]: { ...(current[runId] ?? initial(runId)), ...patch },
}));

async function settleTimeline(
  conversationId: string,
  runId: string,
  signal: AbortSignal,
  update: Dispatch<SetStateAction<ConversationTimelineView | null>>,
) {
  for (let attempt = 0; attempt < 12 && !signal.aborted; attempt += 1) {
    if (attempt > 0) await wait(250, signal);
    const timeline = await client.getTimeline(conversationId, signal);
    update(timeline);
    const candidate = timeline.turns
      .flatMap((turn) => turn.candidates)
      .find((item) => item.runId === runId);
    if (!candidate || candidate.status !== "running") return;
  }
}

const wait = (milliseconds: number, signal: AbortSignal) =>
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
