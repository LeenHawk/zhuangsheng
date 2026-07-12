import { useCallback, useEffect, useRef, useState } from "react";

import {
  createIdempotencyKey,
  type EffectResolutionSubmission,
  type JsonValue,
  type MemoryProposalDecisionInput,
  type SecretStoreStatusView,
  type ToolApprovalDecisionInput,
  type WaitView,
} from "@zhuangsheng/api-client";
import type { HandledWaitSummary } from "@zhuangsheng/domain-ui";

import { localErrorMessage, runtime, secrets } from "./bridge";

export function useLocalWaits(runIds: string[]) {
  const runKey = runIds.join("\0");
  const [waits, setWaits] = useState<WaitView[]>([]);
  const [handledWaits, setHandledWaits] = useState<HandledWaitSummary[]>([]);
  const [secretStatus, setSecretStatus] = useState<SecretStoreStatusView | null>(null);
  const [pendingWaitId, setPendingWaitId] = useState<string | null>(null);
  const [waitError, setWaitError] = useState<string | null>(null);
  const [actionErrors, setActionErrors] = useState<Record<string, string>>({});
  const deliveryIds = useRef<Record<string, string>>({});
  const secretCommandIds = useRef<Record<string, string>>({});
  const effectCommandIds = useRef<Record<string, string>>({});
  const reload = useCallback(async () => {
    if (runIds.length === 0) {
      setWaits([]); setSecretStatus(null); return;
    }
    setWaitError(null);
    try {
      const loaded = (await Promise.all(runIds.map((id) => runtime.listOpenWaits(id)))).flat();
      setWaits(loaded);
      setSecretStatus(loaded.some((wait) => wait.kind === "secret_store_unlocked")
        ? await secrets.status()
        : null);
    } catch (cause) { setWaitError(localErrorMessage(cause)); }
  }, [runKey]);
  useEffect(() => { void reload(); }, [reload]);

  const remember = (wait: WaitView, summary: string) => setHandledWaits((current) => [
    ...current.filter((item) => item.waitId !== wait.id),
    { waitId: wait.id, runId: wait.runId, summary },
  ]);
  const act = async (wait: WaitView, action: () => Promise<void>) => {
    setPendingWaitId(wait.id);
    setActionErrors((current) => ({ ...current, [wait.id]: "" }));
    try { await action(); }
    catch (cause) {
      setActionErrors((current) => ({ ...current, [wait.id]: localErrorMessage(cause) }));
      throw cause;
    } finally { setPendingWaitId(null); }
  };
  const deliveryId = (wait: WaitView) => {
    const value = deliveryIds.current[wait.id] ?? createIdempotencyKey();
    deliveryIds.current[wait.id] = value;
    return value;
  };
  const submitApproval = (wait: WaitView, decisions: ToolApprovalDecisionInput[]) => act(wait, async () => {
    const result = await runtime.submitToolApproval(wait.id, { deliveryId: deliveryId(wait), decisions });
    remember(wait, `已批准 ${result.preparedToolCallIds.length} 项，拒绝 ${result.deniedToolCallIds.length} 项工具操作`);
    await reload();
  });
  const submitMemoryProposals = (wait: WaitView, decisions: MemoryProposalDecisionInput[]) => act(wait, async () => {
    const result = await runtime.submitMemoryProposalDecisions(wait.id, { deliveryId: deliveryId(wait), decisions });
    remember(wait, `已处理 ${result.decidedMemoryProposalIds.length} 项长期记忆提案`);
    await reload();
  });
  const submitHumanResponse = (wait: WaitView, value: JsonValue) => act(wait, async () => {
    await runtime.submitHumanResponse(wait.id, { deliveryId: deliveryId(wait), value });
    remember(wait, "已提交等待中的角色回应");
    await reload();
  });
  const submitSecretPassword = (
    wait: WaitView,
    mode: "initialize" | "unlock",
    masterPassword: string,
  ) => act(wait, async () => {
    const ref = `${mode}:${wait.id}`;
    const idempotencyKey = secretCommandIds.current[ref] ?? createIdempotencyKey();
    secretCommandIds.current[ref] = idempotencyKey;
    await secrets[mode]({ masterPassword, idempotencyKey });
    remember(wait, mode === "initialize" ? "安全存储已初始化并解锁" : "安全存储已解锁");
    await reload();
  });
  const resolveEffect = (wait: WaitView, submission: EffectResolutionSubmission) => act(wait, async () => {
    if (wait.request.kind !== "effect_resolution") return;
    const request = wait.request;
    const ref = `${request.effectId}:${request.effectAttemptId}:${submission.kind}`;
    const idempotencyKey = effectCommandIds.current[ref] ?? createIdempotencyKey();
    effectCommandIds.current[ref] = idempotencyKey;
    const run = await runtime.getRun(wait.runId);
    await runtime.resolveEffectUnknown(request.effectId, {
      expectedEffectAttemptId: request.effectAttemptId,
      expectedRunControlEpoch: run.controlEpoch,
      kind: submission.kind,
      decision: { reason: submission.reason },
      resultObjectId: submission.resultObjectId,
      evidenceObjectId: submission.evidenceObjectId,
      idempotencyKey,
    });
    remember(wait, submission.kind === "abort_run" ? "已终止未知结果运行" : "未知结果已处理");
    await reload();
  });
  return {
    waits, handledWaits, secretStatus, pendingWaitId, waitError, actionErrors,
    submitApproval, submitMemoryProposals, submitHumanResponse, submitSecretPassword,
    resolveEffect, reloadWaits: () => void reload(),
  };
}
