import { useMemo, useState } from "react";
import { ArrowLeft, Ban, CirclePause, Play, RefreshCw } from "lucide-react";

import type {
  GraphRevisionView,
  GraphStructureProjection,
  RunStreamConnectionState,
  RunStreamProjection,
  RunView,
  WaitView,
} from "@zhuangsheng/api-client";
import { projectGraphStructure } from "@zhuangsheng/api-client";
import { selectRunGraphNodeOverlay } from "@zhuangsheng/api-client";
import { GraphCanvas } from "@zhuangsheng/graph-view";
import { Button, Card } from "@zhuangsheng/ui";

import { RunStatusBadge } from "./run-list";
import { RunTrace } from "./run-trace";

interface RunDetailProps {
  run: RunView | null;
  revision: GraphRevisionView | null;
  waits: WaitView[];
  projection: RunStreamProjection;
  connection: RunStreamConnectionState;
  loading: boolean;
  error: string | null;
  streamError: string | null;
  controlPending: "interrupt" | "resume" | "cancel" | null;
  controlError: string | null;
  reload: () => void;
  onBack: () => void;
  onControl: (action: "interrupt" | "resume" | "cancel") => Promise<void>;
}

export function RunDetail(props: RunDetailProps) {
  const [confirmCancel, setConfirmCancel] = useState(false);
  const run = props.run;
  const control = async (action: "interrupt" | "resume" | "cancel") => {
    try { await props.onControl(action); setConfirmCancel(false); } catch { /* typed error stays visible */ }
  };
  return (
    <div className="mx-auto max-w-7xl pb-24">
      <header className="flex items-center gap-3"><Button variant="ghost" size="icon" onClick={props.onBack} aria-label="返回运行列表"><ArrowLeft className="size-5" /></Button><div className="min-w-0"><p className="text-xs font-bold uppercase tracking-[0.16em] text-muted">Run monitor</p><h1 className="truncate font-mono text-xl font-bold">{run?.id || "正在加载"}</h1></div><Button className="ml-auto" variant="secondary" size="compact" onClick={props.reload}><RefreshCw className="size-3.5" />刷新</Button></header>
      {props.error && <Card className="mt-5 border-danger/30 p-4 text-sm text-danger">{props.error}</Card>}
      {run && (
        <>
          <div className="mt-6 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <Metric label="状态" value={<RunStatusBadge status={run.status} />} />
            <Metric label="Control epoch" value={String(run.controlEpoch)} mono />
            <Metric label="Durable cursor" value={String(Math.max(run.lastDurableSeq, props.projection.durableSeq))} mono />
            <Metric label="事件连接" value={connectionText[props.connection]} />
          </div>
          <Card className="mt-4 p-5"><dl className="grid gap-4 text-xs sm:grid-cols-2 lg:grid-cols-4"><Item label="Graph revision" value={run.graphRevisionId} /><Item label="Context / branch" value={`${run.contextId} / ${run.branchId}`} /><Item label="Input commit / ref" value={`${run.inputCommitId} / ${run.inputRef}`} /><Item label="Output commit" value={run.outputCommitId ?? "—"} /><Item label="Deadline" value={new Date(run.deadlineAt).toLocaleString()} /></dl><div className="mt-5 flex flex-wrap gap-2">{(run.status === "running" || run.status === "waiting") && <Button size="compact" variant="secondary" disabled={props.controlPending !== null} onClick={() => void control("interrupt")}><CirclePause className="size-3.5" />暂停</Button>}{run.status === "interrupted" && <Button size="compact" disabled={props.controlPending !== null} onClick={() => void control("resume")}><Play className="size-3.5" />继续</Button>}{!isTerminal(run.status) && (!confirmCancel ? <Button size="compact" variant="ghost" disabled={props.controlPending !== null} onClick={() => setConfirmCancel(true)}><Ban className="size-3.5" />取消运行</Button> : <><Button size="compact" variant="danger" disabled={props.controlPending !== null} onClick={() => void control("cancel")}>确认取消</Button><Button size="compact" variant="ghost" onClick={() => setConfirmCancel(false)}>返回</Button></>)}</div>{props.controlPending && <p className="mt-3 text-xs text-muted">正在提交 {props.controlPending} command…</p>}{props.controlError && <p className="mt-3 text-sm text-danger">{props.controlError}</p>}</Card>
          {props.revision && <RevisionGraph revision={props.revision} projection={props.projection} />}
          {props.waits.length > 0 && <Card className="mt-4 border-warning/30 p-5"><h2 className="font-semibold">Open waits</h2><div className="mt-3 space-y-2">{props.waits.map((wait) => <div key={wait.id} className="rounded-xl bg-elevated p-3 text-xs"><span className="font-mono">{wait.id}</span><span className="ml-2 text-warning">{wait.kind}</span><span className="ml-2 text-muted">{wait.blockers.length} blockers</span></div>)}</div></Card>}
          {props.streamError && <Card className="mt-4 border-warning/30 p-4 text-sm text-warning">事件投影已停止：{props.streamError}</Card>}
          <div className="mt-4"><RunTrace projection={props.projection} /></div>
        </>
      )}
      {props.loading && !run && <div className="mt-6 h-40 animate-pulse rounded-2xl bg-elevated" />}
    </div>
  );
}

function RevisionGraph({
  revision,
  projection,
}: {
  revision: GraphRevisionView;
  projection: RunStreamProjection;
}) {
  const overlay = useMemo(() => selectRunGraphNodeOverlay(projection), [projection]);
  let graph: GraphStructureProjection;
  try {
    graph = projectGraphStructure(revision.definition);
  } catch {
    return <Card className="mt-4 border-danger/30 p-4 text-sm text-danger">固定 Graph revision 无法投影，请升级客户端。</Card>;
  }
  return (
    <Card className="mt-4 p-2">
      <div className="flex flex-wrap items-center justify-between gap-2 px-3 py-2 text-xs">
        <h2 className="font-semibold text-primary">Fixed graph revision</h2>
        <span className="font-mono text-muted">{revision.id} · {revision.contentHash}</span>
      </div>
      <GraphCanvas graph={graph} nodeOverlay={overlay} />
    </Card>
  );
}

function Metric({ label, value, mono = false }: { label: string; value: React.ReactNode; mono?: boolean }) { return <Card className="p-4"><p className="text-xs text-muted">{label}</p><div className={`mt-2 text-sm font-semibold ${mono ? "font-mono" : ""}`}>{value}</div></Card>; }
function Item({ label, value }: { label: string; value: string }) { return <div><dt className="text-muted">{label}</dt><dd className="mt-1 break-all font-mono text-secondary">{value}</dd></div>; }
const isTerminal = (status: RunView["status"]) => status === "completed" || status === "failed" || status === "cancelled";
const connectionText: Record<RunStreamConnectionState, string> = { idle: "未连接", connecting: "连接中", live: "实时", reconnecting: "重连中", incompatible: "不兼容", closed: "已关闭" };
