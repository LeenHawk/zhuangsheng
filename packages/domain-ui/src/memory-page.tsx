import { useEffect, useState, type FormEvent } from "react";
import { Plus, RefreshCw } from "lucide-react";

import type { MemoryProposalView, MemoryRecordView, ProposeMemoryInput } from "@zhuangsheng/api-client";
import { Badge, Button, Input } from "@zhuangsheng/ui";

import { MemoryEditor } from "./memory-editor";
import { MemoryProposalList } from "./memory-proposal-list";
import { MemoryRecordList } from "./memory-record-list";

type ProposalInput = Omit<ProposeMemoryInput, "scopeId" | "idempotencyKey">;
interface Props { scopeId: string; records: MemoryRecordView[]; proposals: MemoryProposalView[]; hasMore: boolean; loading: boolean; pending: boolean; error: string | null; onScopeChange: (scope: string) => void; onReload: () => void; onLoadMore: () => void; onPropose: (input: ProposalInput) => Promise<void>; onDecide: (proposal: MemoryProposalView, decision: "approve" | "reject") => void; onApply: (proposal: MemoryProposalView) => void }

export function MemoryPage(props: Props) {
  const [scope, setScope] = useState(props.scopeId);
  const [editor, setEditor] = useState<{ mode: "create" | "replace" | "delete"; record: MemoryRecordView | null } | null>(null);
  useEffect(() => setScope(props.scopeId), [props.scopeId]);
  const selectScope = (event: FormEvent) => { event.preventDefault(); const value = scope.trim(); if (value) props.onScopeChange(value); };
  return <div className="mx-auto max-w-6xl space-y-5 pb-24"><header className="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between"><div><Badge tone="info">MemoryManager projection</Badge><h1 className="mt-3 font-display text-3xl font-bold">长期记忆</h1><p className="mt-2 text-secondary">更正、忘记与新增都先进入可审核 proposal，不直接修改记录。</p></div><form className="flex gap-2" onSubmit={selectScope}><Input aria-label="Memory scope" value={scope} onChange={(event) => setScope(event.target.value)} className="w-56" /><Button type="submit" variant="secondary">切换 scope</Button></form></header>
    {props.error && <div role="alert" className="rounded-xl border border-danger/25 bg-danger/5 p-3 text-sm text-danger">{props.error}</div>}
    <div className="flex flex-wrap gap-2"><Button onClick={() => setEditor({ mode: "create", record: null })}><Plus className="size-4" />提出新记忆</Button><Button variant="secondary" onClick={props.onReload}><RefreshCw className="size-4" />刷新</Button></div>
    {editor && <MemoryEditor key={`${editor.mode}:${editor.record?.id ?? "new"}`} mode={editor.mode} record={editor.record} pending={props.pending} onCancel={() => setEditor(null)} onSubmit={props.onPropose} />}
    <section><div className="mb-3 flex items-center justify-between"><h2 className="font-display text-xl font-bold">当前记录</h2><span className="text-xs text-muted">{props.records.length} 条 · {props.scopeId}</span></div>{props.loading ? <p className="text-sm text-muted">正在加载权威 projection…</p> : <MemoryRecordList records={props.records} onEdit={(record) => setEditor({ mode: "replace", record })} onForget={(record) => setEditor({ mode: "delete", record })} />}</section>
    <section><div className="mb-3 flex items-center justify-between"><h2 className="font-display text-xl font-bold">Proposal inbox</h2><span className="text-xs text-muted">已加载 {props.proposals.length} 条</span></div><MemoryProposalList proposals={props.proposals} pending={props.pending} onDecide={props.onDecide} onApply={props.onApply} />{props.hasMore && <Button className="mt-3" variant="secondary" onClick={props.onLoadMore} disabled={props.pending}>加载更多 proposal</Button>}</section>
  </div>;
}
