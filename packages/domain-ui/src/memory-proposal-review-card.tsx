import { useState, type FormEvent } from "react";
import { BrainCircuit } from "lucide-react";

import type { MemoryProposalDecisionInput, WaitView } from "@zhuangsheng/api-client";
import { Badge, Button, Card } from "@zhuangsheng/ui";

interface Props {
  wait: WaitView;
  pending: boolean;
  error: string | null;
  onSubmit: (wait: WaitView, decisions: MemoryProposalDecisionInput[]) => Promise<void>;
}

export function MemoryProposalReviewCard({ wait, pending, error, onSubmit }: Props) {
  const request = wait.request.kind === "memory_proposal_review" ? wait.request : null;
  const [decisions, setDecisions] = useState<Record<string, "approve" | "reject">>({});
  if (!request) return null;
  const complete = request.proposals.every((item) => decisions[item.proposalId]);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!complete || pending) return;
    try {
      await onSubmit(wait, request.proposals.map((item) => ({
        proposalId: item.proposalId,
        decision: decisions[item.proposalId]!,
      })));
    } catch {
      // The owner keeps the delivery ID and renders the typed command error.
    }
  };
  return (
    <Card className="border-warning/30 p-5">
      <div className="flex items-center gap-2">
        <BrainCircuit className="size-5 text-warning" />
        <h2 className="font-semibold">需要审核长期记忆变更</h2>
        <Badge className="ml-auto" tone="warning">整批决定</Badge>
      </div>
      <p className="mt-2 text-sm text-secondary">角色只能提出变更；你的逐项决定会被记录，批准后仍需由 Memory Manager 执行应用。</p>
      <form className="mt-4 space-y-4" onSubmit={submit}>
        {request.proposals.map((item, index) => (
          <fieldset key={item.proposalId} className="rounded-xl border border-default p-4" disabled={pending}>
            <legend className="px-1 text-xs font-semibold text-muted">提案 {index + 1}</legend>
            <p className="text-xs font-semibold text-secondary">{changeLabel(item.proposal.changeType)} · {item.proposal.scopeId}</p>
            {item.proposal.proposedContent && <p className="mt-2 whitespace-pre-wrap text-sm text-primary">{item.proposal.proposedContent.text}</p>}
            <p className="mt-2 text-xs text-muted">理由：{item.proposal.reason}</p>
            <div className="mt-3 flex gap-2">
              <Button type="button" size="compact" variant={decisions[item.proposalId] === "approve" ? "primary" : "secondary"} onClick={() => setDecisions((current) => ({ ...current, [item.proposalId]: "approve" }))}>批准提案</Button>
              <Button type="button" size="compact" variant={decisions[item.proposalId] === "reject" ? "danger" : "secondary"} onClick={() => setDecisions((current) => ({ ...current, [item.proposalId]: "reject" }))}>拒绝提案</Button>
            </div>
          </fieldset>
        ))}
        {error && <p className="text-sm text-danger">{error}</p>}
        <Button type="submit" disabled={!complete || pending}>{pending ? "正在提交决定…" : "提交全部决定"}</Button>
      </form>
    </Card>
  );
}

const changeLabel = (change: "create" | "replace_content" | "mark_obsolete" | "delete_tombstone") =>
  change === "create" ? "新增" : change === "replace_content" ? "更正内容" : change === "mark_obsolete" ? "标记过时" : "删除墓碑";
