export type WaitKind =
  | "human_response"
  | "approval"
  | "webhook"
  | "timer"
  | "external_job"
  | "effect_resolution"
  | "secret_store_unlocked";

export interface ToolApprovalCallView {
  toolCallId: string;
  callDigest: string;
  riskSummary: string;
  expiresAt: number;
}

export type WaitRequestView =
  | { kind: "tool_approval"; modelCallId: string; calls: ToolApprovalCallView[] }
  | { kind: "secret_store_unlocked"; reason: string; channelId: string }
  | {
      kind: "effect_resolution";
      effectId: string;
      effectAttemptId: string;
      ownerKind: "model_call" | "tool_call";
      ownerId: string;
      classification: "pure" | "idempotent" | "non_idempotent";
      allowedResolutions: EffectResolutionKind[];
    }
  | { kind: "unsupported" };

export interface WaitBlockerView {
  kind: "tool_call" | "memory_proposal" | "effect";
  id: string;
  order: number;
  status: "open" | "satisfied" | "rejected" | "aborted";
  decisionRef: string | null;
}

export interface WaitView {
  id: string;
  runId: string;
  nodeInstanceId: string;
  attemptId: string;
  kind: WaitKind;
  requestRef: string;
  request: WaitRequestView;
  correlationKey: string | null;
  deadlineAt: number | null;
  status: "open";
  blockers: WaitBlockerView[];
  acceptedDeliveryId: null;
  createdAt: number;
  resolvedAt: null;
}

export interface ToolApprovalDecisionInput {
  toolCallId: string;
  callDigest: string;
  decision: "approve" | "reject";
  reason?: string;
}

export interface SubmitToolApprovalInput {
  deliveryId: string;
  decisions: ToolApprovalDecisionInput[];
}

export interface WaitDeliveryView {
  waitId: string;
  deliveryId: string;
  status: "resolved";
  preparedToolCallIds: string[];
  deniedToolCallIds: string[];
  replayed: boolean;
}
import type { EffectResolutionKind } from "./effect-types";
