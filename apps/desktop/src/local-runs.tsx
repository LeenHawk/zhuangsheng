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
import { RunDetail, RunList } from "@zhuangsheng/domain-ui";

import { config, localErrorMessage, runtime } from "./bridge";

export function LocalRuns({ initialRunId, onRunOpened, onOpenContext, onReturnToStory }: {
  initialRunId: string | null;
  onRunOpened: () => void;
  onOpenContext: () => void;
  onReturnToStory: () => void;
}) {
  const [selected, setSelected] = useState<string | null>(initialRunId);
  const [returnToStory, setReturnToStory] = useState(initialRunId !== null);
  useEffect(() => {
    if (initialRunId) { setSelected(initialRunId); setReturnToStory(true); onRunOpened(); }
  }, [initialRunId, onRunOpened]);
  return selected
    ? <LocalRunDetail runId={selected} onBack={() => {
        if (returnToStory) { setReturnToStory(false); onReturnToStory(); }
        else setSelected(null);
      }} onOpenContext={onOpenContext} />
    : <LocalRunList onOpen={(id) => { setReturnToStory(false); setSelected(id); }} />;
}

function LocalRunList({ onOpen }: { onOpen: (id: string) => void }) {
  const [runs, setRuns] = useState<RunView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    setLoading(true); setError(null);
    try { setRuns((await runtime.listRecentRuns()).items); }
    catch (cause) { setError(localErrorMessage(cause)); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void reload(); }, [reload]);
  return <RunList runs={runs} loading={loading} error={error} onReload={() => void reload()} onOpen={onOpen} />;
}

function LocalRunDetail({ runId, onBack, onOpenContext }: { runId: string; onBack: () => void; onOpenContext: () => void }) {
  const [run, setRun] = useState<RunView | null>(null);
  const [revision, setRevision] = useState<GraphRevisionView | null>(null);
  const [waits, setWaits] = useState<WaitView[]>([]);
  const [projection, setProjection] = useState<RunStreamProjection>(() => createRunStreamProjection(runId));
  const [connection, setConnection] = useState<RunStreamConnectionState>("idle");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [streamError, setStreamError] = useState<string | null>(null);
  const [controlPending, setControlPending] = useState<"interrupt" | "resume" | "cancel" | null>(null);
  const [controlError, setControlError] = useState<string | null>(null);
  const keys = useRef<Record<string, string>>({});
  const load = useCallback(async (showLoading = false) => {
    if (showLoading) setLoading(true);
    try {
      const next = await runtime.getRun(runId);
      const [fixed, open] = await Promise.all([
        config.getGraphRevision(next.graphRevisionId), runtime.listOpenWaits(runId),
      ]);
      setRun(next); setRevision(fixed); setWaits(open); setError(null);
    } catch (cause) { setError(localErrorMessage(cause)); }
    finally { if (showLoading) setLoading(false); }
  }, [runId]);
  useEffect(() => {
    const controller = new AbortController();
    let seenRefresh = 0;
    setProjection(createRunStreamProjection(runId)); setStreamError(null);
    void load(true);
    void followRunEvents(runtime, runId, controller.signal, {
      onConnection: (state, cause) => {
        setConnection(state); if (cause) setStreamError(localErrorMessage(cause));
      },
      onProjection: (next) => {
        setProjection(next);
        if (next.refreshVersion > seenRefresh) { seenRefresh = next.refreshVersion; void load(); }
      },
    }).catch((cause: unknown) => {
      if (!controller.signal.aborted) setStreamError(localErrorMessage(cause));
    });
    return () => controller.abort();
  }, [load, runId]);
  const control = async (action: "interrupt" | "resume" | "cancel") => {
    if (!run) return;
    const scope = `${action}:${run.controlEpoch}`;
    const input: RunControlInput = {
      expectedEpoch: run.controlEpoch,
      idempotencyKey: keys.current[scope] ?? createIdempotencyKey(),
      reason: "expert_run_monitor",
    };
    keys.current[scope] = input.idempotencyKey;
    setControlPending(action); setControlError(null);
    try {
      setRun(action === "interrupt" ? await runtime.interrupt(run.id, input)
        : action === "resume" ? await runtime.resume(run.id, input)
          : await runtime.cancel(run.id, input));
    } catch (cause) {
      setControlError(localErrorMessage(cause)); await load(); throw cause;
    } finally { setControlPending(null); }
  };
  return <RunDetail run={run} revision={revision} waits={waits} projection={projection} connection={connection} loading={loading} error={error} streamError={streamError} controlPending={controlPending} controlError={controlError} reload={() => void load(true)} onBack={onBack} onControl={control} onOpenContext={onOpenContext} />;
}
