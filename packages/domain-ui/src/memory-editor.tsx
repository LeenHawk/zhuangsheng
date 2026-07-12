import { useState, type FormEvent } from "react";

import type { JsonObject, MemoryRecordView, ProposeMemoryInput } from "@zhuangsheng/api-client";
import { Button, Card, Input, Textarea } from "@zhuangsheng/ui";

type ProposalInput = Omit<ProposeMemoryInput, "scopeId" | "idempotencyKey">;

export function MemoryEditor({ mode, record, pending, onCancel, onSubmit }: { mode: "create" | "replace" | "delete"; record: MemoryRecordView | null; pending: boolean; onCancel: () => void; onSubmit: (input: ProposalInput) => Promise<void> }) {
  const [text, setText] = useState(record?.content?.text ?? "");
  const [tags, setTags] = useState(record?.content?.tags.join(", ") ?? "");
  const [reason, setReason] = useState(mode === "create" ? "记录对后续故事有用的信息" : mode === "replace" ? "更正不准确或过时的记忆" : "忘记这条记忆");
  const [evidence, setEvidence] = useState("");
  const valid = reason.trim().length > 0 && (mode === "delete" || text.trim().length > 0) && (mode === "create" || Boolean(record?.headCommitId));
  const submit = async (event: FormEvent) => {
    event.preventDefault(); if (!valid || pending) return;
    const evidenceRefs = evidence.split(/[\n,]/).map((value) => value.trim()).filter(Boolean);
    const base = { memoryId: record?.id ?? null, expectedHeadCommitId: record?.headCommitId ?? null, reason: reason.trim(), evidenceRefs };
    const content = { schemaVersion: 1 as const, text: text.trim(), tags: tags.split(",").map((value) => value.trim()).filter(Boolean), attributes: (record?.content?.attributes ?? {}) as JsonObject };
    const change = mode === "create" ? { type: "create" as const, content } : mode === "replace" ? { type: "replace_content" as const, content } : { type: "delete_tombstone" as const };
    await onSubmit({ ...base, change });
    onCancel();
  };
  return (
    <Card className="border-accent/30 p-5">
      <h2 className="font-semibold">{mode === "create" ? "提出一条新记忆" : mode === "replace" ? "提出记忆更正" : "提出忘记请求"}</h2>
      <p className="mt-1 text-xs text-muted">这里只创建 proposal；审核与 Apply 是后续独立步骤。</p>
      <form className="mt-4 grid gap-3 md:grid-cols-2" onSubmit={submit}>
        {mode !== "delete" && <><Field label="记忆内容"><Textarea value={text} onChange={(event) => setText(event.target.value)} maxLength={1_048_576} /></Field><Field label="标签（逗号分隔）"><Input value={tags} onChange={(event) => setTags(event.target.value)} /></Field></>}
        <Field label="原因"><Textarea value={reason} onChange={(event) => setReason(event.target.value)} maxLength={4096} /></Field>
        <Field label="证据引用（每行或逗号分隔）"><Textarea value={evidence} onChange={(event) => setEvidence(event.target.value)} placeholder="例如 message_123" /></Field>
        {mode === "delete" && <p className="md:col-span-2 text-sm text-warning">Apply 后当前 projection 不再显示内容，但历史 commit 与审计引用仍会保留。</p>}
        <div className="flex gap-2 md:col-span-2"><Button type="button" variant="ghost" onClick={onCancel}>取消</Button><Button type="submit" disabled={!valid || pending}>{pending ? "正在提交…" : "创建 proposal"}</Button></div>
      </form>
    </Card>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) { return <label className="block text-xs font-semibold text-secondary">{label}<div className="mt-1.5">{children}</div></label>; }
