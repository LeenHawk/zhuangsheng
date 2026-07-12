import { useState } from "react";
import { Check, FilePlus2, GitCommitHorizontal, Loader2, RefreshCw, Save } from "lucide-react";

import type { GraphDraftView, GraphRevisionView, GraphStructureProjection, GraphSummary, ValidationIssue } from "@zhuangsheng/api-client";
import { GraphCanvas } from "@zhuangsheng/graph-view";
import { Badge, Button, Card, Input } from "@zhuangsheng/ui";

import { GraphDiagnostics } from "./graph-diagnostics";
import { GraphJsonEditor } from "./graph-json-editor";

export interface GraphStudioProps {
  graphs: GraphSummary[];
  selectedGraphId: string | null;
  draft: GraphDraftView | null;
  jsonText: string;
  projection: GraphStructureProjection | null;
  diagnostics: ValidationIssue[];
  applied: GraphRevisionView | null;
  dirty: boolean;
  status: "loading" | "ready" | "saving" | "applying";
  error: string | null;
  onSelectGraph: (id: string) => void;
  onCreateGraph: (name: string) => Promise<void>;
  onJsonChange: (value: string) => void;
  onSave: () => void;
  onApply: () => void;
  onReload: () => void;
}

export function GraphStudio(props: GraphStudioProps) {
  const [newName, setNewName] = useState("");
  const busy = props.status === "saving" || props.status === "applying";
  const create = async () => {
    const name = newName.trim();
    if (!name) return;
    await props.onCreateGraph(name);
    setNewName("");
  };
  return (
    <div className="space-y-4">
      <header className="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
        <div><Badge tone="info">专家模式</Badge><h1 className="mt-3 font-display text-3xl font-bold">Agent Studio</h1><p className="mt-1 text-sm text-secondary">编辑 canonical GraphDraft，检查结构投影，再由服务端 Apply。</p></div>
        {props.draft && <div className="flex flex-wrap items-center gap-2 text-xs text-muted"><Badge tone={props.dirty ? "warning" : "neutral"}>{props.dirty ? "未保存" : "已保存"}</Badge><span className="font-mono">token {short(props.draft.revisionToken)}</span></div>}
      </header>
      <Card className="p-3">
        <div className="flex flex-col gap-3 md:flex-row md:items-center">
          <label className="text-xs font-semibold text-secondary">Graph <select aria-label="选择 Graph" className="ml-2 min-h-10 rounded-xl border border-default bg-canvas px-3 text-sm text-primary" value={props.selectedGraphId ?? ""} onChange={(event) => props.onSelectGraph(event.target.value)} disabled={busy}><option value="" disabled>选择 Graph</option>{props.graphs.map((graph) => <option key={graph.id} value={graph.id}>{graph.name}</option>)}</select></label>
          <div className="flex flex-1 gap-2 md:justify-end"><Input aria-label="新 Graph 名称" placeholder="新 Graph 名称" value={newName} onChange={(event) => setNewName(event.target.value)} className="max-w-64" /><Button variant="secondary" onClick={() => void create()} disabled={!newName.trim() || busy}><FilePlus2 className="size-4" />创建</Button></div>
        </div>
      </Card>
      {props.error && <div role="alert" className="flex items-center justify-between gap-3 rounded-xl border border-danger/25 bg-danger/5 p-3 text-sm text-danger"><span>{props.error}</span><Button size="compact" variant="secondary" onClick={props.onReload}><RefreshCw className="size-3.5" />重新加载</Button></div>}
      {!props.selectedGraphId && props.status !== "loading" && <Card className="py-20 text-center text-secondary">创建或选择一个 Graph 开始编辑。</Card>}
      {props.status === "loading" && <Card className="grid min-h-80 place-items-center text-secondary"><Loader2 className="size-6 animate-spin" aria-label="正在加载 Graph" /></Card>}
      {props.draft && props.status !== "loading" && <>
        <div className="grid gap-4 xl:grid-cols-[minmax(0,1.6fr)_minmax(360px,0.8fr)]">
          <Card className="min-w-0 p-2">{props.projection ? <GraphCanvas graph={props.projection} /> : <div className="grid min-h-[480px] place-items-center text-sm text-muted">JSON 有误，画布投影已暂停。</div>}</Card>
          <Card className="p-4"><GraphJsonEditor value={props.jsonText} onChange={props.onJsonChange} disabled={busy} /></Card>
        </div>
        <Card className="p-4"><div className="mb-3 flex flex-wrap items-center justify-between gap-3"><div><h2 className="font-semibold">Diagnostics 与 Apply</h2><p className="text-xs text-muted">本地检查仅作提示，服务端校验是最终权威。</p></div><div className="flex gap-2"><Button variant="secondary" onClick={props.onSave} disabled={busy || !props.dirty || !props.projection}>{props.status === "saving" ? <Loader2 className="size-4 animate-spin" /> : <Save className="size-4" />}保存草稿</Button><Button onClick={props.onApply} disabled={busy || props.dirty || !props.projection}>{props.status === "applying" ? <Loader2 className="size-4 animate-spin" /> : <GitCommitHorizontal className="size-4" />}Apply</Button></div></div><GraphDiagnostics issues={props.diagnostics} />
          {props.applied && <div className="mt-3 flex flex-wrap items-center gap-2 rounded-xl border border-success/25 bg-success/5 p-3 text-sm text-success"><Check className="size-4" /><strong>Applied revision {props.applied.revisionNo}</strong><span className="font-mono text-xs">{props.applied.id} · {short(props.applied.contentHash)}</span></div>}
        </Card>
      </>}
    </div>
  );
}

const short = (value: string) => value.length > 18 ? `${value.slice(0, 8)}…${value.slice(-6)}` : value;
