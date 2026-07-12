import { useState, type FormEvent } from "react";
import { ShieldAlert } from "lucide-react";

import type {
  ToolApprovalDecisionInput,
  ToolApprovalCallView,
  WaitView,
} from "@zhuangsheng/api-client";
import { Badge, Button, Card, Input } from "@zhuangsheng/ui";

interface ToolApprovalCardProps {
  wait: WaitView;
  pending: boolean;
  error: string | null;
  onSubmit: (wait: WaitView, decisions: ToolApprovalDecisionInput[]) => Promise<void>;
}

export function ToolApprovalCard({ wait, pending, error, onSubmit }: ToolApprovalCardProps) {
  const request = wait.request.kind === "tool_approval" ? wait.request : null;
  const [decisions, setDecisions] = useState<Record<string, "approve" | "reject">>({});
  const [reasons, setReasons] = useState<Record<string, string>>({});
  if (!request) return null;
  const complete = request.calls.every((call) => decisions[call.toolCallId]);
  const expired = wait.deadlineAt !== null && wait.deadlineAt <= Date.now();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!complete || pending) return;
    try {
      await onSubmit(wait, request.calls.map((call) => ({
        toolCallId: call.toolCallId,
        callDigest: call.callDigest,
        decision: decisions[call.toolCallId]!,
        reason: reasons[call.toolCallId]?.trim() || undefined,
      })));
    } catch {
      // The owner retains the delivery id and renders the typed command error.
    }
  };
  return (
    <Card className="border-warning/30 p-5">
      <div className="flex items-center gap-2">
        <ShieldAlert className="size-5 text-warning" />
        <h2 className="font-semibold">需要确认工具操作</h2>
        <Badge className="ml-auto" tone="warning">整批确认</Badge>
      </div>
      <p className="mt-2 text-sm text-secondary">角色请求执行以下能力。所有项目确认后才会继续，本次批准不会扩大原有权限。</p>
      <form className="mt-4 space-y-4" onSubmit={submit}>
        {request.calls.map((call, index) => (
          <ApprovalCall
            key={call.toolCallId}
            call={call}
            index={index}
            decision={decisions[call.toolCallId]}
            reason={reasons[call.toolCallId] ?? ""}
            disabled={pending}
            onDecision={(decision) => setDecisions((current) => ({ ...current, [call.toolCallId]: decision }))}
            onReason={(reason) => setReasons((current) => ({ ...current, [call.toolCallId]: reason }))}
          />
        ))}
        {wait.deadlineAt && <p className="text-xs text-muted">确认期限：{new Date(wait.deadlineAt).toLocaleString()}</p>}
        {expired && <p className="text-sm text-warning">本机时间显示期限可能已过；提交时仍由服务端时钟重新校验。</p>}
        {error && <p className="text-sm text-danger">{error}</p>}
        <Button type="submit" disabled={!complete || pending}>
          {pending ? "正在提交决定…" : "提交全部决定"}
        </Button>
      </form>
    </Card>
  );
}

function ApprovalCall(props: {
  call: ToolApprovalCallView;
  index: number;
  decision?: "approve" | "reject";
  reason: string;
  disabled: boolean;
  onDecision: (decision: "approve" | "reject") => void;
  onReason: (reason: string) => void;
}) {
  return (
    <fieldset className="rounded-xl border border-default p-4" disabled={props.disabled}>
      <legend className="px-1 text-xs font-semibold text-muted">操作 {props.index + 1}</legend>
      <p className="text-sm leading-6 text-primary">{props.call.riskSummary}</p>
      <div className="mt-3 flex gap-2">
        <Button type="button" size="compact" variant={props.decision === "approve" ? "primary" : "secondary"} onClick={() => props.onDecision("approve")}>允许一次</Button>
        <Button type="button" size="compact" variant={props.decision === "reject" ? "danger" : "secondary"} onClick={() => props.onDecision("reject")}>拒绝</Button>
      </div>
      {props.decision === "reject" && (
        <label className="mt-3 block text-xs font-semibold text-secondary">
          原因（可选）
          <Input className="mt-1.5" value={props.reason} onChange={(event) => props.onReason(event.target.value)} maxLength={512} autoComplete="off" />
        </label>
      )}
    </fieldset>
  );
}
