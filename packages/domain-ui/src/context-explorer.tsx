import { useEffect, useState } from "react";
import { ArrowLeft, GitFork, RefreshCw } from "lucide-react";

import { stringifyJsonExact, type ContextBranchView, type ContextCommitView, type ContextDiffView, type MergeContextInput, type MergeContextView, type VersionSnapshotView, type WorkingContextView } from "@zhuangsheng/api-client";
import { Badge, Button, Card } from "@zhuangsheng/ui";

import { ContextMergePanel } from "./context-merge-panel";

export function ContextExplorer(props: {
  contextId: string; branches: ContextBranchView[]; commits: ContextCommitView[];
  selectedBranchId: string; selectedCommitId: string | null;
  projection: WorkingContextView | null; historical: WorkingContextView | null;
  diff: ContextDiffView | null; mergeResult: MergeContextView | null; snapshot: VersionSnapshotView | null;
  loading: boolean; pending: string | null; error: string | null;
  onBack: () => void; onReload: () => void; onSelectBranch: (id: string) => void;
  onSelectCommit: (id: string) => void; onDiff: (from: string, to: string) => void;
  onFork: (branch: ContextBranchView, commitId: string) => Promise<void>;
  onMerge: (input: MergeContextInput) => Promise<void>; onSnapshot: (commitId: string) => Promise<void>;
}) {
  const branchCommits = props.commits.filter((commit) => commit.branchId === props.selectedBranchId);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  useEffect(() => {
    const selected = props.commits.find((commit) => commit.id === props.selectedCommitId);
    setTo(selected?.id ?? branchCommits.at(-1)?.id ?? "");
    setFrom(selected?.parentCommitIds[0] ?? branchCommits.at(-2)?.id ?? "");
  }, [props.selectedCommitId, props.selectedBranchId, props.commits.length]);
  const branch = props.branches.find((item) => item.branchId === props.selectedBranchId);
  return (
    <div className="mx-auto max-w-[1500px] pb-24">
      <header className="flex items-center gap-3"><Button size="icon" variant="ghost" onClick={props.onBack} aria-label="返回 Context 列表"><ArrowLeft className="size-5" /></Button><div className="min-w-0"><Badge tone="info">Expert Context</Badge><h1 className="mt-1 truncate font-mono text-xl font-bold">{props.contextId}</h1></div><Button className="ml-auto" size="compact" variant="secondary" onClick={props.onReload}><RefreshCw className="size-4" />刷新</Button></header>
      {props.error && <Card className="mt-4 border-danger/30 p-4 text-sm text-danger">{props.error}</Card>}
      <div className="mt-5 grid gap-4 lg:grid-cols-[280px_340px_minmax(0,1fr)]">
        <Card className="p-4"><h2 className="font-semibold">Branch tree</h2><div className="mt-3 space-y-2">{props.branches.map((item) => <button key={item.branchId} className={`w-full rounded-xl border p-3 text-left ${item.branchId === props.selectedBranchId ? "border-info/50 bg-info/5" : "border-default bg-elevated"}`} onClick={() => props.onSelectBranch(item.branchId)}><div className="flex items-center gap-2"><GitFork className="size-4 text-info" /><span className="truncate font-mono text-xs font-semibold">{item.branchId}</span></div><p className="mt-1 truncate font-mono text-[11px] text-muted">{item.forkCommitId} → {item.headCommitId}</p><p className="mt-1 text-[11px] text-secondary">{item.status}</p></button>)}</div></Card>
        <Card className="p-4"><h2 className="font-semibold">Commit list</h2><div className="mt-3 max-h-[680px] space-y-2 overflow-auto">{branchCommits.map((commit) => <button key={commit.id} className={`w-full rounded-xl p-3 text-left ${commit.id === props.selectedCommitId ? "bg-info/10" : "bg-elevated"}`} onClick={() => props.onSelectCommit(commit.id)}><p className="truncate font-mono text-xs font-semibold">#{commit.sequenceNo} · {commit.id}</p><p className="mt-1 text-[11px] text-muted">{commit.author.kind} · schema {commit.schemaVersion} · policy {commit.policyVersion}</p><p className="mt-1 truncate text-[11px] text-secondary">operation {commit.operationId}</p></button>)}</div>{branch && props.selectedCommitId && <div className="mt-4 flex flex-wrap gap-2"><Button size="compact" variant="secondary" disabled={props.pending !== null} onClick={() => void props.onFork(branch, props.selectedCommitId!)}>从此处 Fork</Button><Button size="compact" variant="ghost" disabled={props.pending !== null} onClick={() => void props.onSnapshot(props.selectedCommitId!)}>固定 Snapshot</Button></div>}{props.snapshot && <p className="mt-3 break-all text-xs text-success">snapshot {props.snapshot.snapshotRef}</p>}</Card>
        <div className="space-y-4"><JsonCard title={props.historical ? `Commit ${props.historical.headCommitId}` : `Branch head ${props.projection?.headCommitId ?? "—"}`} value={(props.historical ?? props.projection)?.value} /><Card className="p-5"><h2 className="font-semibold">JSON Pointer diff</h2><div className="mt-3 grid gap-2 sm:grid-cols-[1fr_1fr_auto]"><CommitSelect label="From" value={from} commits={props.commits} onChange={setFrom} /><CommitSelect label="To" value={to} commits={props.commits} onChange={setTo} /><Button className="self-end" size="compact" disabled={!from || !to || from === to} onClick={() => props.onDiff(from, to)}>比较</Button></div><div className="mt-4 space-y-2">{props.diff?.changes.map((change) => <div key={change.path} className="rounded-xl bg-elevated p-3"><p className="font-mono text-xs font-semibold">{change.path}</p><div className="mt-2 grid gap-2 sm:grid-cols-2"><Value label="before" value={change.before} /><Value label="after" value={change.after} /></div></div>)}{props.diff && props.diff.changes.length === 0 && <p className="text-sm text-muted">两个 commit 没有可见差异。</p>}</div></Card><ContextMergePanel branches={props.branches} selectedBranchId={props.selectedBranchId} result={props.mergeResult} pending={props.pending === "merge"} onMerge={props.onMerge} /></div>
      </div>
      {props.loading && <p className="mt-4 text-sm text-muted">正在读取权威 Context projection…</p>}
    </div>
  );
}

function JsonCard({ title, value }: { title: string; value: unknown }) { return <Card className="p-5"><h2 className="font-semibold">{title}</h2><pre className="mt-3 max-h-80 overflow-auto whitespace-pre-wrap rounded-xl bg-canvas p-3 text-xs text-secondary">{value === undefined ? "正在加载…" : stringifyJsonExact(value, 2)}</pre></Card>; }
function CommitSelect(props: { label: string; value: string; commits: ContextCommitView[]; onChange: (id: string) => void }) { return <label className="text-xs font-semibold text-secondary">{props.label}<select className="mt-1.5 min-h-10 w-full rounded-xl border border-default bg-canvas px-2 font-mono text-xs" value={props.value} onChange={(event) => props.onChange(event.target.value)}><option value="">选择 commit</option>{props.commits.map((commit) => <option key={commit.id} value={commit.id}>#{commit.sequenceNo} {commit.id}</option>)}</select></label>; }
function Value({ label, value }: { label: string; value: unknown }) { return <div><p className="text-[10px] font-bold uppercase text-muted">{label}</p><pre className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap text-[11px] text-secondary">{stringifyJsonExact(value, 2)}</pre></div>; }
