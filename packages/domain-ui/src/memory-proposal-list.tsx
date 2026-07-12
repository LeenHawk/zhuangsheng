import { Check, GitCommitHorizontal, X } from "lucide-react";

import type { MemoryProposalView } from "@zhuangsheng/api-client";
import { Badge, Button, Card } from "@zhuangsheng/ui";

const labels: Record<MemoryProposalView["status"], string> = { proposed: "已提出", awaiting_confirmation: "待确认", awaiting_review: "待审核", approved: "已批准", rejected: "已拒绝", applied: "已应用", conflicted: "版本冲突" };

export function MemoryProposalList({ proposals, pending, onDecide, onApply }: { proposals: MemoryProposalView[]; pending: boolean; onDecide: (proposal: MemoryProposalView, decision: "approve" | "reject") => void; onApply: (proposal: MemoryProposalView) => void }) {
  if (proposals.length === 0) return <p className="text-sm text-muted">没有 proposal。</p>;
  return <div className="space-y-3">{proposals.map((proposal) => {
    const reviewable = proposal.status === "awaiting_review" || proposal.status === "awaiting_confirmation";
    return <Card key={proposal.id} className="p-4"><div className="flex flex-wrap items-center gap-2"><Badge tone={proposal.status === "conflicted" ? "danger" : proposal.status === "applied" ? "success" : proposal.status === "approved" ? "info" : "warning"}>{labels[proposal.status]}</Badge><Badge>{changeLabel(proposal.changeType)}</Badge><span className="ml-auto font-mono text-[10px] text-muted">{proposal.id}</span></div>{proposal.proposedContent && <p className="mt-3 whitespace-pre-wrap text-sm leading-6">{proposal.proposedContent.text}</p>}<p className="mt-3 text-sm text-secondary"><strong>原因：</strong>{proposal.reason}</p><div className="mt-2 text-xs text-muted">expected head: <span className="font-mono">{proposal.expectedHeadCommitId ?? "new record"}</span> · policy {proposal.policyVersion}</div>{proposal.evidenceRefs.length > 0 && <div className="mt-2 text-xs text-muted">证据：{proposal.evidenceRefs.join("、")}</div>}<div className="mt-4 flex gap-2">{reviewable && <><Button size="compact" onClick={() => onDecide(proposal, "approve")} disabled={pending}><Check className="size-3.5" />批准</Button><Button size="compact" variant="secondary" onClick={() => onDecide(proposal, "reject")} disabled={pending}><X className="size-3.5" />拒绝</Button></>}{proposal.status === "approved" && <Button size="compact" onClick={() => onApply(proposal)} disabled={pending}><GitCommitHorizontal className="size-3.5" />Apply</Button>}</div>{proposal.status === "conflicted" && <p className="mt-3 text-xs text-danger">Memory head 已变化。请基于最新记录创建新的更正 proposal，系统不会强制覆盖。</p>}</Card>;
  })}</div>;
}

function changeLabel(type: MemoryProposalView["changeType"]) { return type === "create" ? "新增" : type === "replace_content" ? "更正" : type === "mark_obsolete" ? "标记过时" : "忘记"; }
