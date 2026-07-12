import { useCallback, useEffect, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

import type {
  ContextBranchView, ContextCommitView, ContextDiffView, MergeContextInput,
  MergeContextView, RunView, VersionSnapshotView, WorkingContextView,
} from "@zhuangsheng/api-client";
import { ContextExplorer, ContextIndex } from "@zhuangsheng/domain-ui";

import { client, messageFor } from "./api";

export function ContextsRoute() {
  const navigate = useNavigate();
  const [runs, setRuns] = useState<RunView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    setLoading(true); setError(null);
    try { setRuns((await client.runtime.listRecentRuns(100)).items); }
    catch (cause) { setError(messageFor(cause)); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void reload(); }, [reload]);
  return <ContextIndex runs={runs} loading={loading} error={error} onReload={() => void reload()} onOpen={(contextId, branchId) => navigate(`/expert/contexts/${encodeURIComponent(contextId)}${branchId ? `?branch=${encodeURIComponent(branchId)}` : ""}`)} />;
}

export function ContextRoute() {
  const { contextId = "" } = useParams();
  const navigate = useNavigate();
  const [search, setSearch] = useSearchParams();
  const [branches, setBranches] = useState<ContextBranchView[]>([]);
  const [commits, setCommits] = useState<ContextCommitView[]>([]);
  const [projection, setProjection] = useState<WorkingContextView | null>(null);
  const [historical, setHistorical] = useState<WorkingContextView | null>(null);
  const [selectedCommitId, setSelectedCommitId] = useState<string | null>(null);
  const [diff, setDiff] = useState<ContextDiffView | null>(null);
  const [mergeResult, setMergeResult] = useState<MergeContextView | null>(null);
  const [snapshot, setSnapshot] = useState<VersionSnapshotView | null>(null);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const selectedBranchId = search.get("branch") ?? branches.find((branch) => branch.status === "active")?.branchId ?? branches[0]?.branchId ?? "";

  const reload = useCallback(async (preferredBranchId?: string) => {
    if (!contextId) return;
    setLoading(true); setError(null);
    try {
      const [nextBranches, nextCommits] = await Promise.all([
        client.contexts.listBranches(contextId), client.contexts.listCommits(contextId),
      ]);
      setBranches(nextBranches); setCommits(nextCommits);
      const requested = preferredBranchId ?? search.get("branch");
      const branch = nextBranches.find((item) => item.branchId === requested)
        ?? nextBranches.find((item) => item.status === "active") ?? nextBranches[0];
      if (branch && branch.branchId !== requested) setSearch({ branch: branch.branchId }, { replace: true });
    } catch (cause) { setError(messageFor(cause)); }
    finally { setLoading(false); }
  }, [contextId, search.get("branch")]);
  useEffect(() => { void reload(); }, [reload]);
  useEffect(() => {
    if (!selectedBranchId) { setProjection(null); return; }
    const controller = new AbortController();
    setHistorical(null); setSelectedCommitId(null);
    void client.contexts.getBranch(contextId, selectedBranchId, controller.signal)
      .then(setProjection).catch((cause) => { if (!controller.signal.aborted) setError(messageFor(cause)); });
    return () => controller.abort();
  }, [contextId, selectedBranchId]);

  const selectCommit = (commitId: string) => {
    setSelectedCommitId(commitId); setHistorical(null); setError(null);
    void client.contexts.getCommit(commitId).then(setHistorical).catch((cause) => setError(messageFor(cause)));
  };
  const act = async (kind: string, action: () => Promise<void>) => {
    setPending(kind); setError(null);
    try { await action(); } catch (cause) { setError(messageFor(cause)); throw cause; }
    finally { setPending(null); }
  };
  const fork = (branch: ContextBranchView, commitId: string) => act("fork", async () => {
    const created = await client.contexts.fork(contextId, {
      sourceBranchId: branch.branchId, fromCommitId: commitId, expectedSourceHead: branch.headCommitId,
    });
    setSearch({ branch: created.branchId }); await reload(created.branchId);
  });
  const merge = (input: MergeContextInput) => act("merge", async () => {
    const result = await client.contexts.merge(contextId, input);
    setMergeResult(result);
    if (result.status === "merged") { setSearch({ branch: result.targetBranchId }); await reload(result.targetBranchId); }
  });
  const createSnapshot = (commitId: string) => act("snapshot", async () => {
    setSnapshot(await client.contexts.createSnapshot(commitId, { pinned: true }));
  });
  const loadDiff = (from: string, to: string) => {
    setError(null); void client.contexts.diff(contextId, from, to).then(setDiff).catch((cause) => setError(messageFor(cause)));
  };
  return <ContextExplorer contextId={contextId} branches={branches} commits={commits} selectedBranchId={selectedBranchId} selectedCommitId={selectedCommitId} projection={projection} historical={historical} diff={diff} mergeResult={mergeResult} snapshot={snapshot} loading={loading} pending={pending} error={error} onBack={() => navigate("/expert/contexts")} onReload={() => void reload()} onSelectBranch={(branch) => setSearch({ branch })} onSelectCommit={selectCommit} onDiff={loadDiff} onFork={fork} onMerge={merge} onSnapshot={createSnapshot} />;
}
