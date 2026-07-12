import { Brain, PencilLine, Trash2 } from "lucide-react";

import type { MemoryRecordView } from "@zhuangsheng/api-client";
import { Badge, Button, Card } from "@zhuangsheng/ui";

export function MemoryRecordList({ records, onEdit, onForget }: { records: MemoryRecordView[]; onEdit: (record: MemoryRecordView) => void; onForget: (record: MemoryRecordView) => void }) {
  if (records.length === 0) return <Card className="grid min-h-44 place-items-center p-6 text-center text-sm text-muted">这个 scope 还没有已 Apply 的记忆。</Card>;
  return <div className="grid gap-3 md:grid-cols-2">{records.map((record) => <Card key={record.id} className="p-4"><div className="flex items-start gap-3"><div className="grid size-9 shrink-0 place-items-center rounded-xl bg-accent-soft text-accent"><Brain className="size-4" /></div><div className="min-w-0 flex-1"><div className="flex flex-wrap items-center gap-2"><Badge tone={record.status === "active" ? "success" : "warning"}>{record.status === "active" ? "有效" : "已过时"}</Badge><span className="font-mono text-[10px] text-muted">{short(record.id)}</span></div><p className="mt-3 whitespace-pre-wrap text-sm leading-6 text-primary">{record.content?.text ?? "内容不可用"}</p>{record.content && <div className="mt-3 flex flex-wrap gap-1.5">{record.content.tags.map((tag) => <Badge key={tag}>{tag}</Badge>)}</div>}<div className="mt-4 flex flex-wrap gap-2">{record.status === "active" && <Button size="compact" variant="secondary" onClick={() => onEdit(record)}><PencilLine className="size-3.5" />更正</Button>}<Button size="compact" variant="ghost" onClick={() => onForget(record)}><Trash2 className="size-3.5" />忘记</Button></div></div></div></Card>)}</div>;
}

const short = (value: string) => value.length > 18 ? `${value.slice(0, 8)}…${value.slice(-5)}` : value;
