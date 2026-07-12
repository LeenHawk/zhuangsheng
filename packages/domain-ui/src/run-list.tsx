import { Activity, ArrowRight, RefreshCw } from "lucide-react";

import type { RunStatus, RunView } from "@zhuangsheng/api-client";
import { Badge, Button, Card } from "@zhuangsheng/ui";

interface RunListProps {
  runs: RunView[];
  loading: boolean;
  error: string | null;
  onReload: () => void;
  onOpen: (runId: string) => void;
}

export function RunList({ runs, loading, error, onReload, onOpen }: RunListProps) {
  return (
    <div className="mx-auto max-w-6xl pb-24">
      <header className="flex items-end justify-between gap-4">
        <div><p className="text-xs font-bold uppercase tracking-[0.18em] text-muted">Expert runtime</p><h1 className="mt-2 font-display text-3xl font-bold">运行与 Trace</h1><p className="mt-2 text-sm text-secondary">按权威 RunView 与 durable sequence 检查执行，不从时间戳猜测状态。</p></div>
        <Button variant="secondary" onClick={onReload}><RefreshCw className="size-4" />刷新</Button>
      </header>
      {error && <Card className="mt-6 border-danger/30 p-4 text-sm text-danger">{error}</Card>}
      {loading ? (
        <div className="mt-6 space-y-3">{[0, 1, 2].map((item) => <div key={item} className="h-24 animate-pulse rounded-2xl bg-elevated" />)}</div>
      ) : runs.length === 0 ? (
        <Card className="mt-6 p-10 text-center"><Activity className="mx-auto size-7 text-muted" /><h2 className="mt-3 font-semibold">还没有运行</h2><p className="mt-1 text-sm text-muted">从故事提交 Turn 或在 Agent Studio 启动 Graph 后会出现在这里。</p></Card>
      ) : (
        <div className="mt-6 space-y-3">{runs.map((run) => (
          <button key={run.id} className="group w-full text-left" onClick={() => onOpen(run.id)}>
            <Card className="flex items-center gap-4 p-4 transition hover:border-accent/40 hover:shadow-panel">
              <div className="grid size-11 shrink-0 place-items-center rounded-xl bg-elevated"><Activity className="size-5 text-accent" /></div>
              <div className="min-w-0 flex-1"><div className="flex items-center gap-2"><span className="truncate font-mono text-sm font-semibold">{run.id}</span><RunStatusBadge status={run.status} /></div><p className="mt-1 truncate text-xs text-muted">Graph {run.graphRevisionId} · seq {run.lastDurableSeq} · epoch {run.controlEpoch}</p></div>
              <time className="hidden text-xs text-muted sm:block">{new Date(run.updatedAt).toLocaleString()}</time>
              <ArrowRight className="size-4 text-muted transition group-hover:translate-x-1 group-hover:text-accent" />
            </Card>
          </button>
        ))}</div>
      )}
    </div>
  );
}

export function RunStatusBadge({ status }: { status: RunStatus }) {
  const [label, tone] = statusView[status];
  return <Badge tone={tone}>{label}</Badge>;
}

const statusView: Record<RunStatus, readonly [string, "neutral" | "running" | "warning" | "success" | "danger"]> = {
  created: ["已创建", "neutral"], running: ["运行中", "running"], waiting: ["等待处理", "warning"],
  interrupting: ["正在暂停", "warning"], interrupted: ["已暂停", "neutral"], completed: ["已完成", "success"],
  failed: ["失败", "danger"], cancelled: ["已取消", "neutral"],
};
