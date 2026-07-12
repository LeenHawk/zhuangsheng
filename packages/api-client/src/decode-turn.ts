import { DecodeError } from "./decode-error";
import { nullableString, number, record, string } from "./decode-helpers";
import type {
  CandidateProjectionResolutionView,
  CandidateStatus,
  ConversationCandidateView,
  ConversationTurnView,
} from "./types";

const candidateStatuses = new Set<CandidateStatus>([
  "running", "ready", "failed", "cancelled", "projection_conflicted",
  "projection_failed", "projection_abandoned",
]);

export const decodeConversationCandidate = (
  value: unknown,
  path: string,
): ConversationCandidateView => {
  const candidate = record(value, path);
  const status = string(candidate.status, `${path}.status`) as CandidateStatus;
  if (!candidateStatuses.has(status)) throw new DecodeError(`${path}.status`);
  const rawError = candidate.projectionError;
  const projectionError = rawError === null ? null : (() => {
    const error = record(rawError, `${path}.projectionError`);
    return {
      code: string(error.code, `${path}.projectionError.code`),
      safeMessage: string(error.safeMessage, `${path}.projectionError.safeMessage`),
    };
  })();
  return {
    turnId: string(candidate.turnId, `${path}.turnId`),
    runId: string(candidate.runId, `${path}.runId`),
    branchId: string(candidate.branchId, `${path}.branchId`),
    baseCommitId: string(candidate.baseCommitId, `${path}.baseCommitId`),
    replyOutputKey: string(candidate.replyOutputKey, `${path}.replyOutputKey`),
    status,
    assistantMessageId: nullableString(candidate.assistantMessageId, `${path}.assistantMessageId`),
    candidateCommitId: nullableString(candidate.candidateCommitId, `${path}.candidateCommitId`),
    projectionError,
    createdAt: number(candidate.createdAt, `${path}.createdAt`),
  };
};

export const decodeConversationTurn = (
  value: unknown,
  path: string,
): ConversationTurnView => {
  const turn = record(value, path);
  if (!Array.isArray(turn.candidates)) throw new DecodeError(`${path}.candidates`);
  return {
    id: string(turn.id, `${path}.id`),
    conversationId: string(turn.conversationId, `${path}.conversationId`),
    userMessageId: string(turn.userMessageId, `${path}.userMessageId`),
    userCommitId: string(turn.userCommitId, `${path}.userCommitId`),
    createdAt: number(turn.createdAt, `${path}.createdAt`),
    selectedRunId: nullableString(turn.selectedRunId, `${path}.selectedRunId`),
    candidates: turn.candidates.map((candidate, index) =>
      decodeConversationCandidate(candidate, `${path}.candidates[${index}]`)),
  };
};

export const decodeTurnCandidates = (value: unknown): ConversationTurnView =>
  decodeConversationTurn(value, "turnCandidates");

export const decodeCandidateStatus = (value: unknown, path: string): CandidateStatus => {
  const status = string(value, path) as CandidateStatus;
  if (!candidateStatuses.has(status)) throw new DecodeError(path);
  return status;
};

export const decodeCandidateProjectionResolution = (
  value: unknown,
): CandidateProjectionResolutionView => {
  const path = "candidateProjectionResolution";
  const item = record(value, path);
  const status = decodeCandidateStatus(item.status, `${path}.status`);
  if (status !== "ready" && status !== "projection_abandoned") {
    throw new DecodeError(`${path}.status`);
  }
  const assistantMessageId = nullableString(item.assistantMessageId, `${path}.assistantMessageId`);
  const candidateCommitId = nullableString(item.candidateCommitId, `${path}.candidateCommitId`);
  if ((status === "ready" && (assistantMessageId === null || candidateCommitId === null))
    || (status === "projection_abandoned" && (assistantMessageId !== null || candidateCommitId !== null))) {
    throw new DecodeError(`${path}.status`);
  }
  return {
    turnId: string(item.turnId, `${path}.turnId`),
    runId: string(item.runId, `${path}.runId`),
    branchId: string(item.branchId, `${path}.branchId`),
    branchHeadCommitId: string(item.branchHeadCommitId, `${path}.branchHeadCommitId`),
    status,
    assistantMessageId,
    candidateCommitId,
    resolvedAt: number(item.resolvedAt, `${path}.resolvedAt`),
  };
};
