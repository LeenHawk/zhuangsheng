import { useCallback, useEffect, useRef, useState } from "react";

import {
  createIdempotencyKey,
  createRunStreamProjection,
  followRunEvents,
  type GraphRevisionView,
  type RunControlInput,
  type RunStreamConnectionState,
  type RunStreamProjection,
  type RunView,
  type WaitView,
} from "@zhuangsheng/api-client";

import { client, messageFor } from "./api";

type ControlAction = "interrupt" | "resume" | "cancel";

export function useRunMonitor(runId: string) {
  const [run, setRun] = useState<RunView | null>(null);
  const [revision, setRevision] = useState<GraphRevisionView | null>(null);
  const [waits, setWaits] = useState<WaitView[]>([]);
  const [projection, setProjection] = useState<RunStreamProjection>(() =>
    createRunStreamProjection(runId));
  const [connection, setConnection] = useState<RunStreamConnectionState>("idle");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [streamError, setStreamError] = useState<string | null>(null);
  const [controlPending, setControlPending] = useState<ControlAction | null>(null);
  const [controlError, setControlError] = useState<string | null>(null);
  const controlKeys = useRef<Record<string, string>>({});

  const load = useCallback(async (signal?: AbortSignal, showLoading = true) => {
    if (showLoading) setLoading(true);
    setError(null);
    try {
      const nextRun = await client.runtime.getRun(runId, signal);
      const [nextRevision, nextWaits] = await Promise.all([
        client.graphs.getRevision(nextRun.graphRevisionId, signal),
        client.runtime.listOpenWaits(runId, signal),
      ]);
      setRun(nextRun);
      setRevision(nextRevision);
      setWaits(nextWaits);
    } catch (cause) {
      if (!signal?.aborted) setError(messageFor(cause));
    } finally {
      if (showLoading && !signal?.aborted) setLoading(false);
    }
  }, [runId]);

  useEffect(() => {
    const controller = new AbortController();
    let seenRefresh = 0;
    setRun(null);
    setRevision(null);
    setWaits([]);
    setProjection(createRunStreamProjection(runId));
    setStreamError(null);
    void load(controller.signal);
    void followRunEvents(client.runtime, runId, controller.signal, {
      onConnection: (state, cause) => {
        setConnection(state);
        if (cause) setStreamError(messageFor(cause));
      },
      onProjection: (next) => {
        setProjection(next);
        if (next.refreshVersion > seenRefresh) {
          seenRefresh = next.refreshVersion;
          void load(controller.signal, false);
        }
      },
    }).catch((cause: unknown) => {
      if (!controller.signal.aborted) setStreamError(messageFor(cause));
    });
    return () => controller.abort();
  }, [load, runId]);

  const control = async (action: ControlAction) => {
    if (!run) return;
    const scope = `${action}:${run.controlEpoch}`;
    const input: RunControlInput = {
      expectedEpoch: run.controlEpoch,
      idempotencyKey: controlKeys.current[scope] ?? createIdempotencyKey(),
      reason: "expert_run_monitor",
    };
    controlKeys.current[scope] = input.idempotencyKey;
    setControlPending(action);
    setControlError(null);
    try {
      const next = action === "interrupt"
        ? await client.runtime.interrupt(run.id, input)
        : action === "resume"
          ? await client.runtime.resume(run.id, input)
          : await client.runtime.cancel(run.id, input);
      setRun(next);
    } catch (cause) {
      setControlError(messageFor(cause));
      await load(undefined, false);
      throw cause;
    } finally {
      setControlPending(null);
    }
  };

  return {
    run,
    revision,
    waits,
    projection,
    connection,
    loading,
    error,
    streamError,
    controlPending,
    controlError,
    reload: () => void load(),
    control,
  };
}
