import { useState } from "react";
import { GitBranch, Search } from "lucide-react";

import type { RunView } from "@zhuangsheng/api-client";
import { Badge, Button, Card, Input } from "@zhuangsheng/ui";

export function ContextIndex(props: {
  runs: RunView[];
  loading: boolean;
  error: string | null;
  onOpen: (contextId: string, branchId?: string) => void;
  onReload: () => void;
}) {
  const [contextId, setContextId] = useState("");
  const recent = Array.from(new Map(props.runs.map((run) => [run.contextId, run])).values());
  return (
    <div className="mx-auto max-w-5xl pb-24">
      <header className="flex flex-wrap items-end gap-4">
        <div><Badge tone="info">WorkingContext</Badge><h1 className="mt-3 font-display text-3xl font-bold">Context 与分支</h1><p className="mt-2 text-secondary">查看权威 branch、commit、JSON Pointer diff，并执行显式 fork/merge。</p></div>
        <form className="ml-auto flex gap-2" onSubmit={(event) => { event.preventDefault(); if (contextId.trim()) props.onOpen(contextId.trim()); }}>
          <Input aria-label="Context ID" value={contextId} onChange={(event) => setContextId(event.target.value)} placeholder="context_…" />
          <Button type="submit" disabled={!contextId.trim()}><Search className="size-4" />打开</Button>
        </form>
      </header>
      {props.error && <Card className="mt-5 border-danger/30 p-4 text-sm text-danger">{props.error}<Button className="ml-3" size="compact" variant="secondary" onClick={props.onReload}>重试</Button></Card>}
      <div className="mt-6 grid gap-3 sm:grid-cols-2">
        {recent.map((run) => <button key={run.contextId} className="rounded-2xl border border-default bg-surface p-5 text-left transition hover:border-info/40 hover:bg-elevated" onClick={() => props.onOpen(run.contextId, run.branchId)}><div className="flex items-center gap-2 font-semibold"><GitBranch className="size-4 text-info" /><span className="truncate font-mono">{run.contextId}</span></div><p className="mt-2 truncate text-xs text-muted">branch {run.branchId}</p><p className="mt-1 text-xs text-secondary">最近运行：{run.id}</p></button>)}
      </div>
      {!props.loading && !props.error && recent.length === 0 && <Card className="mt-6 p-8 text-center text-secondary">暂无可发现的 Context。可直接输入 Context ID。</Card>}
    </div>
  );
}
