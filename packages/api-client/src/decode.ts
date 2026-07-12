import type {
  CandidateStatus,
  ConversationListView,
  ConversationMessageView,
  ConversationRunProfile,
  ConversationTimelineView,
  ConversationView,
  LlmContentPart,
  SubmitConversationTurnAck,
} from "./types";
import { DecodeError } from "./decode-error";
import { nullableString, number, record, string } from "./decode-helpers";

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

const candidateStatuses = new Set<CandidateStatus>([
  "running", "ready", "failed", "cancelled", "projection_conflicted",
  "projection_failed", "projection_abandoned",
]);

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
    turns: item.turns.map((rawTurn, turnIndex) => {
      const path = `timeline.turns[${turnIndex}]`;
      const turn = record(rawTurn, path);
      if (!Array.isArray(turn.candidates)) throw new DecodeError(`${path}.candidates`);
      return {
        id: string(turn.id, `${path}.id`), conversationId: string(turn.conversationId, `${path}.conversationId`),
        userMessageId: string(turn.userMessageId, `${path}.userMessageId`),
        userCommitId: string(turn.userCommitId, `${path}.userCommitId`),
        createdAt: number(turn.createdAt, `${path}.createdAt`),
        selectedRunId: nullableString(turn.selectedRunId, `${path}.selectedRunId`),
        candidates: turn.candidates.map((rawCandidate, candidateIndex) => {
          const candidatePath = `${path}.candidates[${candidateIndex}]`;
          const candidate = record(rawCandidate, candidatePath);
          const status = string(candidate.status, `${candidatePath}.status`) as CandidateStatus;
          if (!candidateStatuses.has(status)) throw new DecodeError(`${candidatePath}.status`);
          const error = candidate.projectionError;
          return {
            turnId: string(candidate.turnId, `${candidatePath}.turnId`), runId: string(candidate.runId, `${candidatePath}.runId`),
            branchId: string(candidate.branchId, `${candidatePath}.branchId`), baseCommitId: string(candidate.baseCommitId, `${candidatePath}.baseCommitId`),
            replyOutputKey: string(candidate.replyOutputKey, `${candidatePath}.replyOutputKey`), status,
            assistantMessageId: nullableString(candidate.assistantMessageId, `${candidatePath}.assistantMessageId`),
            candidateCommitId: nullableString(candidate.candidateCommitId, `${candidatePath}.candidateCommitId`),
            projectionError: error === null ? null : (() => { const detail = record(error, `${candidatePath}.projectionError`); return { code: string(detail.code, `${candidatePath}.projectionError.code`), safeMessage: string(detail.safeMessage, `${candidatePath}.projectionError.safeMessage`) }; })(),
            createdAt: number(candidate.createdAt, `${candidatePath}.createdAt`),
          };
        }),
      };
    }),
  };
};

export const decodeSubmitTurnAck = (value: unknown): SubmitConversationTurnAck => {
  const result = record(value, "submitTurn");
  const turn = record(result.turn, "submitTurn.turn");
  const candidate = record(result.candidate, "submitTurn.candidate");
  const run = record(result.run, "submitTurn.run");
  const status = string(candidate.status, "submitTurn.candidate.status") as CandidateStatus;
  if (!candidateStatuses.has(status)) throw new DecodeError("submitTurn.candidate.status");
  const runId = string(candidate.runId, "submitTurn.candidate.runId");
  if (string(run.id, "submitTurn.run.id") !== runId) throw new DecodeError("submitTurn.run.id");
  return {
    turnId: string(turn.id, "submitTurn.turn.id"),
    runId,
    status,
  };
};
