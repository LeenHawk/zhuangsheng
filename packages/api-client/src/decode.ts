import type {
  ConversationListView,
  ConversationMessageView,
  ConversationRunProfile,
  ConversationSelectionView,
  ConversationTimelineView,
  ConversationView,
  LlmContentPart,
  RegenerateConversationCandidateAck,
  SubmitConversationTurnAck,
} from "./types";
import { DecodeError } from "./decode-error";
import { nullableString, number, record, string } from "./decode-helpers";
import { decodeCandidateStatus, decodeConversationTurn } from "./decode-turn";

export { DecodeError } from "./decode-error";

const contentPart = (value: unknown, path: string): LlmContentPart => {
  const item = record(value, path);
  const type = string(item.type, `${path}.type`);
  if (type === "text") return { type, text: string(item.text, `${path}.text`) };
  if (type !== "image" && type !== "file") throw new DecodeError(`${path}.type`);
  const ref = record(item.artifactRef, `${path}.artifactRef`);
  return {
    type,
    artifactRef: {
      artifactId: string(ref.artifactId, `${path}.artifactRef.artifactId`),
      contentHash: string(ref.contentHash, `${path}.artifactRef.contentHash`),
      byteSize: number(ref.byteSize, `${path}.artifactRef.byteSize`),
      mediaType: string(ref.mediaType, `${path}.artifactRef.mediaType`),
    },
  };
};

export const decodeConversation = (value: unknown): ConversationView => {
  const item = record(value, "conversation");
  const profile = item.runProfile;
  return {
    id: string(item.id, "conversation.id"),
    title: nullableString(item.title, "conversation.title"),
    contextId: string(item.contextId, "conversation.contextId"),
    activeBranchId: string(item.activeBranchId, "conversation.activeBranchId"),
    activeHeadCommitId: string(item.activeHeadCommitId, "conversation.activeHeadCommitId"),
    runProfile: profile === null ? null : decodeRunProfile(profile, "conversation.runProfile"),
    createdAt: number(item.createdAt, "conversation.createdAt"),
    updatedAt: number(item.updatedAt, "conversation.updatedAt"),
  };
};

export const decodeRunProfile = (
  value: unknown,
  path = "runProfile",
): ConversationRunProfile => {
  const run = record(value, path);
  if (run.inputShape !== "conversation_message_v1") {
    throw new DecodeError(`${path}.inputShape`);
  }
  return {
    graphRevisionId: string(run.graphRevisionId, `${path}.graphRevisionId`),
    replyOutputKey: string(run.replyOutputKey, `${path}.replyOutputKey`),
    inputShape: run.inputShape,
    revisionNo: number(run.revisionNo, `${path}.revisionNo`),
  };
};

export const decodeConversationList = (value: unknown): ConversationListView => {
  const item = record(value, "conversationList");
  if (!Array.isArray(item.items)) throw new DecodeError("conversationList.items");
  return { items: item.items.map(decodeConversation) };
};

const message = (value: unknown, index: number): ConversationMessageView => {
  const path = `timeline.messages[${index}]`;
  const item = record(value, path);
  if (item.role !== "user" && item.role !== "assistant") throw new DecodeError(`${path}.role`);
  if (!Array.isArray(item.content)) throw new DecodeError(`${path}.content`);
  if (item.source !== "user_input" && item.source !== "run_output" && item.source !== "saved_partial") {
    throw new DecodeError(`${path}.source`);
  }
  return {
    id: string(item.id, `${path}.id`), turnId: string(item.turnId, `${path}.turnId`),
    branchId: string(item.branchId, `${path}.branchId`), commitId: string(item.commitId, `${path}.commitId`),
    parentMessageId: nullableString(item.parentMessageId, `${path}.parentMessageId`),
    role: item.role, source: item.source,
    content: item.content.map((part, partIndex) => contentPart(part, `${path}.content[${partIndex}]`)),
    originRunId: nullableString(item.originRunId, `${path}.originRunId`),
    createdAt: number(item.createdAt, `${path}.createdAt`),
  };
};

export const decodeTimeline = (value: unknown): ConversationTimelineView => {
  const item = record(value, "timeline");
  if (!Array.isArray(item.messages) || !Array.isArray(item.turns)) throw new DecodeError("timeline");
  return {
    conversationId: string(item.conversationId, "timeline.conversationId"),
    activeBranchId: string(item.activeBranchId, "timeline.activeBranchId"),
    activeHeadCommitId: string(item.activeHeadCommitId, "timeline.activeHeadCommitId"),
    messages: item.messages.map(message),
    turns: item.turns.map((turn, index) =>
      decodeConversationTurn(turn, `timeline.turns[${index}]`)),
  };
};

export const decodeSubmitTurnAck = (value: unknown): SubmitConversationTurnAck => {
  const result = record(value, "submitTurn");
  const turn = record(result.turn, "submitTurn.turn");
  const candidate = record(result.candidate, "submitTurn.candidate");
  const run = record(result.run, "submitTurn.run");
  const status = decodeCandidateStatus(candidate.status, "submitTurn.candidate.status");
  const runId = string(candidate.runId, "submitTurn.candidate.runId");
  if (string(run.id, "submitTurn.run.id") !== runId) throw new DecodeError("submitTurn.run.id");
  return {
    turnId: string(turn.id, "submitTurn.turn.id"),
    runId,
    status,
  };
};

export const decodeRegenerateCandidateAck = (
  value: unknown,
): RegenerateConversationCandidateAck => {
  const result = record(value, "regenerateCandidate");
  const candidate = record(result.candidate, "regenerateCandidate.candidate");
  const run = record(result.run, "regenerateCandidate.run");
  const status = decodeCandidateStatus(
    candidate.status,
    "regenerateCandidate.candidate.status",
  );
  const runId = string(candidate.runId, "regenerateCandidate.candidate.runId");
  if (string(run.id, "regenerateCandidate.run.id") !== runId) {
    throw new DecodeError("regenerateCandidate.run.id");
  }
  return {
    turnId: string(candidate.turnId, "regenerateCandidate.candidate.turnId"),
    runId,
    status,
  };
};

export const decodeConversationSelection = (value: unknown): ConversationSelectionView => {
  const selection = record(value, "conversationSelection");
  return {
    turnId: string(selection.turnId, "conversationSelection.turnId"),
    selectedRunId: string(selection.selectedRunId, "conversationSelection.selectedRunId"),
    selectedBranchId: string(selection.selectedBranchId, "conversationSelection.selectedBranchId"),
    selectedCommitId: string(selection.selectedCommitId, "conversationSelection.selectedCommitId"),
    selectedAt: number(selection.selectedAt, "conversationSelection.selectedAt"),
  };
};
