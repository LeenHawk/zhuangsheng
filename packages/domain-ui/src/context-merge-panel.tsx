import { useState } from "react";

import { stringifyJsonExact, type ContextBranchView, type ExplicitMergeSelection, type MergeContextInput, type MergeContextView } from "@zhuangsheng/api-client";
import { Button, Card } from "@zhuangsheng/ui";

export function ContextMergePanel(props: {
  branches: ContextBranchView[];
  selectedBranchId: string;
  result: MergeContextView | null;
  pending: boolean;
  onMerge: (input: MergeContextInput) => Promise<void>;
}) {
  const active = props.branches.filter((branch) => branch.status === "active");
  const [sourceId, setSourceId] = useState(props.selectedBranchId);
  const [targetId, setTargetId] = useState(active.find((branch) => branch.branchId !== props.selectedBranchId)?.branchId ?? props.selectedBranchId);
  const [disposition, setDisposition] = useState<"mark_merged" | "keep_active">("keep_active");
  const [choices, setChoices] = useState<Record<string, "base" | "source" | "target">>({});
  const source = active.find((branch) => branch.branchId === sourceId);
  const target = active.find((branch) => branch.branchId === targetId);
  const merge = (selections: ExplicitMergeSelection[] = []) => {
    if (!source || !target || source.branchId === target.branchId) return Promise.resolve();
    return props.onMerge({
      sourceBranchId: source.branchId,
      targetBranchId: target.branchId,
      expectedSourceHead: source.headCommitId,
      expectedTargetHead: target.headCommitId,
      sourceDisposition: disposition,
      selections,
    });
  };
  const resolve = () => {
    if (props.result?.status !== "conflicted") return Promise.resolve();
    return merge(props.result.conflicts.map((conflict) => ({
      conflictId: conflict.conflictId,
      path: conflict.path,
      resolution: { type: "final_value", value: conflict[choices[conflict.conflictId] ?? "target"] },
    })));
  };
  return (
    <Card className="p-5">
      <h2 className="font-semibold">有限 Merge</h2>
      <div className="mt-4 grid gap-3 sm:grid-cols-2">
        <Select label="Source branch" value={sourceId} branches={active} onChange={setSourceId} />
        <Select label="Target branch" value={targetId} branches={active} onChange={setTargetId} />
      </div>
      <label className="mt-3 block text-xs font-semibold text-secondary">Source disposition<select className="mt-1.5 min-h-10 w-full rounded-xl border border-default bg-canvas px-3" value={disposition} onChange={(event) => setDisposition(event.target.value as typeof disposition)}><option value="keep_active">keep_active</option><option value="mark_merged">mark_merged</option></select></label>
      <Button className="mt-4" size="compact" variant="secondary" disabled={props.pending || !source || !target || sourceId === targetId} onClick={() => void merge()}>检查并合并</Button>
      {props.result?.status === "merged" && <p className="mt-3 text-sm text-success">已创建 merge commit：<span className="font-mono">{props.result.mergeCommitId}</span></p>}
      {props.result?.status === "conflicted" && <div className="mt-5 space-y-3"><p className="text-sm text-warning">需要逐 path 选择最终值。</p>{props.result.conflicts.map((conflict) => <div key={conflict.conflictId} className="rounded-xl border border-warning/25 bg-warning/5 p-3"><p className="font-mono text-xs font-semibold">{conflict.path}</p><div className="mt-2 grid gap-2 md:grid-cols-3">{(["base", "source", "target"] as const).map((choice) => <label key={choice} className="rounded-lg bg-surface p-2 text-xs"><span className="flex gap-2 font-semibold"><input type="radio" name={conflict.conflictId} checked={(choices[conflict.conflictId] ?? "target") === choice} onChange={() => setChoices((current) => ({ ...current, [conflict.conflictId]: choice }))} />{choice}</span><pre className="mt-2 max-h-24 overflow-auto whitespace-pre-wrap text-[11px] text-secondary">{stringifyJsonExact(conflict[choice], 2)}</pre></label>)}</div></div>)}<Button disabled={props.pending} onClick={() => void resolve()}>提交冲突选择</Button></div>}
    </Card>
  );
}

function Select(props: { label: string; value: string; branches: ContextBranchView[]; onChange: (value: string) => void }) {
  return <label className="text-xs font-semibold text-secondary">{props.label}<select className="mt-1.5 min-h-10 w-full rounded-xl border border-default bg-canvas px-3 font-mono" value={props.value} onChange={(event) => props.onChange(event.target.value)}>{props.branches.map((branch) => <option key={branch.branchId} value={branch.branchId}>{branch.branchId}</option>)}</select></label>;
}
