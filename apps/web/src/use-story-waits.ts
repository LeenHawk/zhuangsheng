import { useCallback, useEffect, useRef, useState } from "react";

import {
  createIdempotencyKey,
  type EffectResolutionKind,
  type SecretStoreStatusView,
  type MemoryProposalDecisionInput,
  type ToolApprovalDecisionInput,
  type WaitView,
} from "@zhuangsheng/api-client";
import type { HandledWaitSummary, StoryLiveCandidate } from "@zhuangsheng/domain-ui";

import { client, messageFor } from "./api";

export function useStoryWaits(liveCandidates: StoryLiveCandidate[]) {
  const runIds = liveCandidates.map((candidate) => candidate.runId);
  const refreshKey = liveCandidates
    .map((candidate) => `${candidate.runId}:${candidate.refreshVersion}`)
    .join("\0");
  const [waits, setWaits] = useState<WaitView[]>([]);
  const [handledWaits, setHandledWaits] = useState<HandledWaitSummary[]>([]);
  const [secretStatus, setSecretStatus] = useState<SecretStoreStatusView | null>(null);
  const [pendingWaitId, setPendingWaitId] = useState<string | null>(null);
  const [waitError, setWaitError] = useState<string | null>(null);
  const [actionErrors, setActionErrors] = useState<Record<string, string>>({});
  const deliveryIds = useRef<Record<string, string>>({});
  const secretCommandIds = useRef<Record<string, string>>({});
  const effectCommandIds = useRef<Record<string, string>>({});

  const reload = useCallback(async (signal?: AbortSignal) => {
    if (runIds.length === 0) {
      setWaits([]);
      setHandledWaits([]);
      setSecretStatus(null);
      return;
    }
    setWaitError(null);
    try {
      const loaded = (await Promise.all(
        runIds.map((runId) => client.runtime.listOpenWaits(runId, signal)),
      )).flat();
      setWaits(loaded);
      if (loaded.some((wait) => wait.kind === "secret_store_unlocked")) {
        setSecretStatus(await client.secrets.status(signal));
      } else {
        setSecretStatus(null);
      }
    } catch (cause) {
      if (!signal?.aborted) setWaitError(messageFor(cause));
    }
  }, [runIds.join("\0")]);

  useEffect(() => {
    const controller = new AbortController();
    void reload(controller.signal);
    return () => controller.abort();
  }, [refreshKey, reload]);

  const submitApproval = async (wait: WaitView, decisions: ToolApprovalDecisionInput[]) => {
    const deliveryId = deliveryIds.current[wait.id] ?? createIdempotencyKey();
    deliveryIds.current[wait.id] = deliveryId;
    await act(wait, async () => {
      const result = await client.runtime.submitToolApproval(wait.id, { deliveryId, decisions });
      const summary = result.deniedToolCallIds.length === 0
        ? `已批准 ${result.preparedToolCallIds.length} 项工具操作`
        : `已批准 ${result.preparedToolCallIds.length} 项，拒绝 ${result.deniedToolCallIds.length} 项`;
      rememberHandled(wait, summary);
      await reload();
    });
  };

  const submitMemoryProposals = async (wait: WaitView, decisions: MemoryProposalDecisionInput[]) => {
    const deliveryId = deliveryIds.current[wait.id] ?? createIdempotencyKey();
    deliveryIds.current[wait.id] = deliveryId;
    await act(wait, async () => {
      const result = await client.runtime.submitMemoryProposalDecisions(wait.id, { deliveryId, decisions });
      rememberHandled(wait, `已处理 ${result.decidedMemoryProposalIds.length} 项长期记忆提案`);
      await reload();
    });
  };

  const submitSecretPassword = async (
    wait: WaitView,
    mode: "initialize" | "unlock",
    masterPassword: string,
  ) => {
    const key = `${mode}:${wait.id}`;
    const idempotencyKey = secretCommandIds.current[key] ?? createIdempotencyKey();
    secretCommandIds.current[key] = idempotencyKey;
    await act(wait, async () => {
      if (mode === "initialize") {
        await client.secrets.initialize({ masterPassword, idempotencyKey });
      } else {
        await client.secrets.unlock({ masterPassword, idempotencyKey });
      }
      rememberHandled(wait, mode === "initialize" ? "安全存储已初始化并解锁" : "安全存储已解锁");
      await reload();
    });
  };

  const resolveEffect = async (
    wait: WaitView,
    kind: EffectResolutionKind,
    reason: string,
  ) => {
    if (wait.request.kind !== "effect_resolution") return;
    const request = wait.request;
    const commandKey = `${request.effectId}:${request.effectAttemptId}:${kind}`;
    const idempotencyKey = effectCommandIds.current[commandKey] ?? createIdempotencyKey();
    effectCommandIds.current[commandKey] = idempotencyKey;
    await act(wait, async () => {
      const run = await client.runtime.getRun(wait.runId);
      await client.runtime.resolveEffectUnknown(request.effectId, {
        expectedEffectAttemptId: request.effectAttemptId,
        expectedRunControlEpoch: run.controlEpoch,
        kind,
        decision: { reason },
        resultObjectId: null,
        evidenceObjectId: null,
        idempotencyKey,
      });
      rememberHandled(wait, kind === "abort_run"
        ? "已隔离未知结果并终止运行"
        : "已确认外部操作未执行，运行将安全重试");
      await reload();
    });
  };

  const rememberHandled = (wait: WaitView, summary: string) => {
    setHandledWaits((current) => [
      ...current.filter((item) => item.waitId !== wait.id),
      { waitId: wait.id, runId: wait.runId, summary },
    ]);
  };

  const act = async (wait: WaitView, action: () => Promise<void>) => {
    setPendingWaitId(wait.id);
    setActionErrors((current) => ({ ...current, [wait.id]: "" }));
    try {
      await action();
    } catch (cause) {
      setActionErrors((current) => ({ ...current, [wait.id]: messageFor(cause) }));
      throw cause;
    } finally {
      setPendingWaitId(null);
    }
  };

  return {
    waits,
    handledWaits,
    secretStatus,
    pendingWaitId,
    waitError,
    actionErrors,
    submitApproval,
    submitMemoryProposals,
    submitSecretPassword,
    resolveEffect,
    reloadWaits: () => void reload(),
  };
}
