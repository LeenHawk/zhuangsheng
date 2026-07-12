import { DecodeError } from "./decode-error";
import { boolean, nullableString, number, record, string, stringArray } from "./decode-helpers";
import { decodeEffectResolutionKind } from "./decode-effect";
import { decodeMemoryProposal } from "./decode-memory";
import type {
  ToolApprovalCallView,
  MemoryProposalReviewItem,
  WaitBlockerView,
  WaitDeliveryView,
  WaitKind,
  WaitRequestView,
  WaitView,
} from "./wait-types";

const waitKinds = new Set<WaitKind>([
  "human_response", "approval", "webhook", "timer", "external_job",
  "effect_resolution", "secret_store_unlocked",
]);

export const decodeOpenWaits = (value: unknown, expectedRunId: string): WaitView[] => {
  if (!Array.isArray(value)) throw new DecodeError("openWaits");
  return value.map((raw, index) => wait(raw, `openWaits[${index}]`, expectedRunId));
};

export const decodeWaitDelivery = (value: unknown): WaitDeliveryView => {
  const item = record(value, "waitDelivery");
  if (item.status !== "resolved") throw new DecodeError("waitDelivery.status");
  return {
    waitId: string(item.waitId, "waitDelivery.waitId"),
    deliveryId: string(item.deliveryId, "waitDelivery.deliveryId"),
    status: item.status,
    preparedToolCallIds: stringArray(item.preparedToolCallIds, "waitDelivery.preparedToolCallIds"),
    deniedToolCallIds: stringArray(item.deniedToolCallIds, "waitDelivery.deniedToolCallIds"),
    decidedMemoryProposalIds: stringArray(item.decidedMemoryProposalIds, "waitDelivery.decidedMemoryProposalIds"),
    replayed: boolean(item.replayed, "waitDelivery.replayed"),
  };
};

const wait = (value: unknown, path: string, expectedRunId: string): WaitView => {
  const item = record(value, path);
  const runId = string(item.runId, `${path}.runId`);
  if (runId !== expectedRunId) throw new DecodeError(`${path}.runId`);
  const kind = string(item.kind, `${path}.kind`) as WaitKind;
  if (!waitKinds.has(kind)) throw new DecodeError(`${path}.kind`);
  if (item.status !== "open" || item.acceptedDeliveryId !== null || item.resolvedAt !== null) {
    throw new DecodeError(`${path}.status`);
  }
  if (!Array.isArray(item.blockers)) throw new DecodeError(`${path}.blockers`);
  const blockers = item.blockers.map((blocker, index) =>
    decodeBlocker(blocker, `${path}.blockers[${index}]`));
  return {
    id: string(item.id, `${path}.id`),
    runId,
    nodeInstanceId: string(item.nodeInstanceId, `${path}.nodeInstanceId`),
    attemptId: string(item.attemptId, `${path}.attemptId`),
    kind,
    requestRef: string(item.requestRef, `${path}.requestRef`),
    request: decodeRequest(item.request, `${path}.request`, kind, blockers),
    correlationKey: nullableString(item.correlationKey, `${path}.correlationKey`),
    deadlineAt: nullableNumber(item.deadlineAt, `${path}.deadlineAt`),
    status: item.status,
    blockers,
    acceptedDeliveryId: null,
    createdAt: number(item.createdAt, `${path}.createdAt`),
    resolvedAt: null,
  };
};

const decodeBlocker = (value: unknown, path: string): WaitBlockerView => {
  const item = record(value, path);
  const kind = string(item.kind, `${path}.kind`);
  const status = string(item.status, `${path}.status`);
  if (kind !== "tool_call" && kind !== "memory_proposal" && kind !== "effect") {
    throw new DecodeError(`${path}.kind`);
  }
  if (status !== "open" && status !== "satisfied" && status !== "rejected" && status !== "aborted") {
    throw new DecodeError(`${path}.status`);
  }
  const order = number(item.order, `${path}.order`);
  if (order < 0) throw new DecodeError(`${path}.order`);
  return {
    kind,
    id: string(item.id, `${path}.id`),
    order,
    status,
    decisionRef: nullableString(item.decisionRef, `${path}.decisionRef`),
  };
};

const decodeRequest = (
  value: unknown,
  path: string,
  waitKind: WaitKind,
  blockers: WaitBlockerView[],
): WaitRequestView => {
  const item = record(value, path);
  if (item.schemaVersion !== 1) throw new DecodeError(`${path}.schemaVersion`);
  if (waitKind === "approval" && item.kind === "tool_approval") {
    if (!Array.isArray(item.calls)) throw new DecodeError(`${path}.calls`);
    const calls = item.calls.map((call, index) => decodeCall(call, `${path}.calls[${index}]`));
    const blockerIds = blockers.filter((blocker) => blocker.status === "open").map((blocker) => blocker.id);
    if (!sameIds(calls.map((call) => call.toolCallId), blockerIds)) {
      throw new DecodeError(`${path}.calls`);
    }
    return { kind: item.kind, modelCallId: string(item.modelCallId, `${path}.modelCallId`), calls };
  }
  if (waitKind === "approval" && item.kind === "memory_proposal_review") {
    if (!Array.isArray(item.proposals)) throw new DecodeError(`${path}.proposals`);
    const proposals = item.proposals.map((proposal, index) =>
      decodeProposal(proposal, `${path}.proposals[${index}]`));
    const blockerIds = blockers.filter((blocker) => blocker.kind === "memory_proposal" && blocker.status === "open").map((blocker) => blocker.id);
    if (!sameIds(proposals.map((proposal) => proposal.proposalId), blockerIds)) {
      throw new DecodeError(`${path}.proposals`);
    }
    return { kind: item.kind, modelCallId: string(item.modelCallId, `${path}.modelCallId`), proposals };
  }
  if (waitKind === "secret_store_unlocked" && item.kind === "secret_store_unlocked") {
    return {
      kind: item.kind,
      reason: string(item.reason, `${path}.reason`),
      channelId: string(item.channelId, `${path}.channelId`),
    };
  }
  if (waitKind === "effect_resolution" && item.kind === "effect_resolution") {
    const effectBlockers = blockers.filter((blocker) => blocker.kind === "effect");
    const effectId = string(item.effectId, `${path}.effectId`);
    if (effectBlockers.length !== 1 || effectBlockers[0]?.id !== effectId) {
      throw new DecodeError(`${path}.effectId`);
    }
    const ownerKind = string(item.ownerKind, `${path}.ownerKind`);
    if (ownerKind !== "model_call" && ownerKind !== "tool_call") {
      throw new DecodeError(`${path}.ownerKind`);
    }
    const classification = string(item.classification, `${path}.classification`);
    if (classification !== "pure" && classification !== "idempotent" && classification !== "non_idempotent") {
      throw new DecodeError(`${path}.classification`);
    }
    if (!Array.isArray(item.allowedResolutions)) {
      throw new DecodeError(`${path}.allowedResolutions`);
    }
    return {
      kind: item.kind,
      effectId,
      effectAttemptId: string(item.effectAttemptId, `${path}.effectAttemptId`),
      ownerKind,
      ownerId: string(item.ownerId, `${path}.ownerId`),
      classification,
      allowedResolutions: item.allowedResolutions.map((resolution, index) =>
        decodeEffectResolutionKind(resolution, `${path}.allowedResolutions[${index}]`)),
    };
  }
  return { kind: "unsupported" };
};

const decodeCall = (value: unknown, path: string): ToolApprovalCallView => {
  const item = record(value, path);
  return {
    toolCallId: string(item.toolCallId, `${path}.toolCallId`),
    callDigest: string(item.callDigest, `${path}.callDigest`),
    riskSummary: string(item.riskSummary, `${path}.riskSummary`),
    expiresAt: number(item.expiresAt, `${path}.expiresAt`),
  };
};

const decodeProposal = (value: unknown, path: string): MemoryProposalReviewItem => {
  const item = record(value, path);
  return {
    proposalId: string(item.proposalId, `${path}.proposalId`),
    toolCallId: string(item.toolCallId, `${path}.toolCallId`),
    proposal: decodeMemoryProposal(item.proposal, `${path}.proposal`),
  };
};

const nullableNumber = (value: unknown, path: string) =>
  value === null ? null : number(value, path);

const sameIds = (left: string[], right: string[]) =>
  left.length === right.length && new Set(left).size === left.length && left.every((id) => right.includes(id));
