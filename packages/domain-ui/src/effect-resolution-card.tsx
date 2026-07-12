import { useState, type FormEvent } from "react";
import { ShieldAlert } from "lucide-react";

import type { EffectResolutionKind, EffectResolutionSubmission, WaitView } from "@zhuangsheng/api-client";
import { Badge, Button, Card, Input } from "@zhuangsheng/ui";

interface Props {
  wait: WaitView;
  pending: boolean;
  error: string | null;
  onSubmit: (wait: WaitView, submission: EffectResolutionSubmission) => Promise<void>;
}

export function EffectResolutionCard({ wait, pending, error, onSubmit }: Props) {
  const request = wait.request;
  const [reason, setReason] = useState("");
  const [resultObjectId, setResultObjectId] = useState("");
  const [evidenceObjectId, setEvidenceObjectId] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  if (request.kind !== "effect_resolution") return null;
  const successAllowed = request.allowedResolutions.includes("confirm_succeeded");
  const retryAllowed = request.allowedResolutions.includes("confirm_failed_retry_safe");
  const abortAllowed = request.allowedResolutions.includes("abort_run");
  const valid = (kind: EffectResolutionKind) => Boolean(reason.trim() && confirmed
    && (kind !== "confirm_succeeded" || resultObjectId.trim())
    && (kind !== "confirm_failed_retry_safe" || request.classification !== "non_idempotent" || evidenceObjectId.trim()));
  const submit = (kind: EffectResolutionKind) => async (event: FormEvent) => {
    event.preventDefault();
    if (!valid(kind) || pending) return;
    try {
      await onSubmit(wait, {
        kind,
        reason: reason.trim(),
        resultObjectId: kind === "confirm_succeeded" ? resultObjectId.trim() : null,
        evidenceObjectId: evidenceObjectId.trim() || null,
      });
    } catch { /* route renders typed error */ }
  };
  return (
    <Card className="border-danger/30 p-5">
      <div className="flex items-center gap-2"><ShieldAlert className="size-5 text-danger" /><h3 className="font-semibold">外部操作结果无法确认</h3><Badge className="ml-auto" tone="warning">{request.classification}</Badge></div>
      <p className="mt-2 text-sm leading-6 text-secondary">系统不会盲目重试。请选择可证明的处理方式；终止会隔离迟到结果并取消本次运行。</p>
      <form className="mt-4 space-y-3">
        <label className="block text-xs font-semibold text-secondary">处理依据<Input className="mt-1.5" value={reason} onChange={(event) => setReason(event.target.value)} placeholder="记录查询结果或人工判断依据" /></label>
        {successAllowed && <label className="block text-xs font-semibold text-secondary">结果 Object ID<Input className="mt-1.5" value={resultObjectId} onChange={(event) => setResultObjectId(event.target.value)} placeholder="确认成功时必填的已授权结果引用" /></label>}
        <label className="block text-xs font-semibold text-secondary">Evidence Object ID（可选）<Input className="mt-1.5" value={evidenceObjectId} onChange={(event) => setEvidenceObjectId(event.target.value)} placeholder="非幂等安全重试时必填" /></label>
        <label className="flex items-start gap-2 text-xs text-secondary"><input className="mt-0.5" type="checkbox" checked={confirmed} onChange={(event) => setConfirmed(event.target.checked)} />我理解此决定会成为不可变审计事实，并可能重试外部操作或终止运行。</label>
        <div className="flex flex-wrap gap-2">{successAllowed && <Button type="submit" disabled={!valid("confirm_succeeded") || pending} onClick={submit("confirm_succeeded")}>{pending ? "正在处理…" : "确认已成功"}</Button>}{retryAllowed && <Button type="submit" variant="secondary" disabled={!valid("confirm_failed_retry_safe") || pending} onClick={submit("confirm_failed_retry_safe")}>{pending ? "正在处理…" : "确认未执行，安全重试"}</Button>}{abortAllowed && <Button type="submit" variant="danger" disabled={!valid("abort_run") || pending} onClick={submit("abort_run")}>{pending ? "正在处理…" : "隔离结果并终止运行"}</Button>}</div>
      </form>
      {request.classification === "non_idempotent" && <p className="mt-3 text-xs text-warning">非幂等操作不能在缺少服务端可验证 evidence 时声明安全重试。</p>}
      {error && <p className="mt-3 text-sm text-danger">{error}</p>}
    </Card>
  );
}
