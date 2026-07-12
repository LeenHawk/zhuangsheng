import type { JsonValue } from "./graph-types";

export type EffectResolutionKind =
  | "confirm_succeeded"
  | "confirm_failed_retry_safe"
  | "abort_run";

export interface ResolveEffectUnknownInput {
  expectedEffectAttemptId: string;
  expectedRunControlEpoch: number;
  kind: EffectResolutionKind;
  decision: JsonValue;
  resultObjectId: string | null;
  evidenceObjectId: string | null;
  idempotencyKey: string;
}

export interface EffectResolutionSubmission {
  kind: EffectResolutionKind;
  reason: string;
  resultObjectId: string | null;
  evidenceObjectId: string | null;
}

export interface EffectResolutionView {
  resolutionId: string;
  effectId: string;
  effectAttemptId: string;
  waitId: string;
  kind: EffectResolutionKind;
  replayed: boolean;
}
