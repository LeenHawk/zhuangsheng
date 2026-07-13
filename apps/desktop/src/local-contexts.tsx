import { useCallback, useEffect, useState } from "react";

import type {
  ContextBranchView,
  ContextCommitView,
  ContextDiffView,
  MergeContextInput,
  MergeContextView,
  RunView,
  VersionSnapshotView,
  WorkingContextView,
} from "@zhuangsheng/api-client";
import { ContextExplorer, ContextIndex } from "@zhuangsheng/domain-ui";

import { contexts, localErrorMessage, runtime } from "./bridge";

export function LocalContexts({ initial, onOpened }: {
  initial: { contextId: string; branchId: string } | null;
  onOpened: () => void;
}) {
  const [selected, setSelected] = useState(initial);
  useEffect(() => { if (initial) onOpened(); }, [initial, onOpened]);
  return selected
    ? <LocalContext contextId={selected.contextId} initialBranchId={selected.branchId} onBack={() => setSelected(null)} />
    : <LocalContextIndex onOpen={(contextId, branchId) => setSelected({ contextId, branchId: branchId ?? "" })} />;
}

function LocalContextIndex({ onOpen }: { onOpen: (contextId: string, branchId?: string) => void }) {
  const [runs, setRuns] = useState<RunView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    setLoading(true); setError(null);
    try { setRuns((await runtime.listRecentRuns(100)).items); }
    catch (cause) { setError(localErrorMessage(cause)); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void reload(); }, [reload]);
  return <ContextIndex runs={runs} loading={loading} error={error} onReload={() => void reload()} onOpen={onOpen} />;
}

function LocalContext({ contextId, initialBranchId, onBack }: { contextId: string; initialBranchId: string; onBack: () => void }) {
  const [branches, setBranches] = useState<ContextBranchView[]>([]);
  const [commits, setCommits] = useState<ContextCommitView[]>([]);
  const [branchId, setBranchId] = useState(initialBranchId);
  const [projection, setProjection] = useState<WorkingContextView | null>(null);
  const [historical, setHistorical] = useState<WorkingContextView | null>(null);
  const [selectedCommitId, setSelectedCommitId] = useState<string | null>(null);
  const [diff, setDiff] = useState<ContextDiffView | null>(null);
  const [mergeResult, setMergeResult] = useState<MergeContextView | null>(null);
  const [snapshot, setSnapshot] = useState<VersionSnapshotView | null>(null);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async (preferred = branchId) => {
    setLoading(true); setError(null);
    try {
      const [nextBranches, nextCommits] = await Promise.all([
        contexts.listBranches(contextId), contexts.listCommits(contextId),
      ]);
      setBranches(nextBranches); setCommits(nextCommits);
      const branch = nextBranches.find((item) => item.branchId === preferred)
        ?? nextBranches.find((item) => item.status === "active") ?? nextBranches[0];
      if (branch) setBranchId(branch.branchId);
    } catch (cause) { setError(localErrorMessage(cause)); }
    finally { setLoading(false); }
  }, [contextId, branchId]);
  useEffect(() => { void reload(); }, [contextId]);
  useEffect(() => {
    if (!branchId) return;
    setHistorical(null); setSelectedCommitId(null);
    void contexts.getBranch(contextId, branchId).then(setProjection).catch((cause) => setError(localErrorMessage(cause)));
  }, [contextId, branchId]);
  const act = async (kind: string, action: () => Promise<void>) => {
    setPending(kind); setError(null);
    try { await action(); } catch (cause) { setError(localErrorMessage(cause)); throw cause; }
    finally { setPending(null); }
  };
  const selectCommit = (id: string) => {
    setSelectedCommitId(id); setHistorical(null);
    void contexts.getCommit(id).then(setHistorical).catch((cause) => setError(localErrorMessage(cause)));
  };
  const fork = (branch: ContextBranchView, commitId: string) => act("fork", async () => {
    const created = await contexts.fork(contextId, {
      sourceBranchId: branch.branchId, fromCommitId: commitId,
      expectedSourceHead: branch.headCommitId,
    });
    setBranchId(created.branchId); await reload(created.branchId);
  });
  const merge = (input: MergeContextInput) => act("merge", async () => {
    const result = await contexts.merge(contextId, input); setMergeResult(result);
    if (result.status === "merged") { setBranchId(result.targetBranchId); await reload(result.targetBranchId); }
  });
  const createSnapshot = (id: string) => act("snapshot", async () => {
    setSnapshot(await contexts.createSnapshot(id, { pinned: true }));
  });
  return <ContextExplorer contextId={contextId} branches={branches} commits={commits} selectedBranchId={branchId} selectedCommitId={selectedCommitId} projection={projection} historical={historical} diff={diff} mergeResult={mergeResult} snapshot={snapshot} loading={loading} pending={pending} error={error} onBack={onBack} onReload={() => void reload()} onSelectBranch={setBranchId} onSelectCommit={selectCommit} onDiff={(from, to) => void contexts.diff(contextId, from, to).then(setDiff).catch((cause) => setError(localErrorMessage(cause)))} onFork={fork} onMerge={merge} onSnapshot={createSnapshot} />;
}
