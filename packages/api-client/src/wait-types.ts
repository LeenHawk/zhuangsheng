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

export interface MemoryProposalReviewItem {
  proposalId: string;
  toolCallId: string;
  proposal: MemoryProposalView;
}

export type WaitRequestView =
  | {
      kind: "human_response";
      title: string | null;
      description: string | null;
      payload: JsonObject;
    }
  | { kind: "tool_approval"; modelCallId: string; calls: ToolApprovalCallView[] }
  | { kind: "memory_proposal_review"; modelCallId: string; proposals: MemoryProposalReviewItem[] }
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
  responseSchema: JsonSchemaSpecView | null;
  responseSchemaCompilation: SchemaCompilationView | null;
  correlationKey: string | null;
  deadlineAt: number | null;
  status: "open";
  blockers: WaitBlockerView[];
  acceptedDeliveryId: null;
  createdAt: number;
  resolvedAt: null;
}

export interface JsonSchemaSpecView {
  schemaVersion: 1;
  dialect: "https://json-schema.org/draft/2020-12/schema";
  validationProfileVersion: 1;
  formatPolicyVersion: 1;
  document: JsonObject;
  limits: Record<string, number>;
}

export interface SchemaCompilationView {
  canonicalDocumentHash: string;
  schemaHash: string;
  canonicalSource: string;
  compiledPayload: string;
  compiledPayloadHash: string;
  compilerId: string;
  compilerVersion: string;
  payloadFormatVersion: number;
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

export interface MemoryProposalDecisionInput {
  proposalId: string;
  decision: "approve" | "reject";
}

export interface SubmitMemoryProposalDecisionInput {
  deliveryId: string;
  decisions: MemoryProposalDecisionInput[];
}

export interface SubmitHumanResponseInput {
  deliveryId: string;
  value: JsonValue;
}

export interface WaitDeliveryView {
  waitId: string;
  deliveryId: string;
  status: "resolved";
  preparedToolCallIds: string[];
  deniedToolCallIds: string[];
  decidedMemoryProposalIds: string[];
  replayed: boolean;
}
import type { EffectResolutionKind } from "./effect-types";
import type { MemoryProposalView } from "./memory-types";
import type { JsonObject, JsonValue } from "./graph-types";
