import {
  decodeConversation,
  decodeConversationList,
  decodeConversationSelection,
  decodeRegenerateCandidateAck,
  decodeRunProfile,
  decodeSubmitTurnAck,
  decodeTimeline,
} from "./decode";
import { decodeContextBranches } from "./decode-context";
import { DecodeError } from "./decode-error";
import { decodeRolePlayGraphOptions } from "./decode-roleplay";
import { decodeCandidateProjectionResolution, decodeTurnCandidates } from "./decode-turn";
import { createIdempotencyKey } from "./idempotency";
import type { TauriBridge } from "./transport";
import type {
  ConversationCommandOptions,
  CreateConversationInput,
  RegenerateConversationCandidateInput,
  SelectConversationCandidateInput,
  SubmitConversationTurnInput,
  UpdateConversationRunProfileInput,
} from "./http-conversation-client";
import type {
  CandidateProjectionResolutionView,
  ConversationListView,
  ConversationRunProfile,
  ConversationSelectionView,
  ConversationTimelineView,
  ConversationTurnView,
  ConversationView,
  RegenerateConversationCandidateAck,
  ResolveCandidateProjectionInput,
  RolePlayGraphOptionView,
  SubmitConversationTurnAck,
} from "./types";
import type { ContextBranchView } from "./context-types";

export class TauriConversationClient {
  constructor(private readonly bridge: TauriBridge) {}

  async listConversations(): Promise<ConversationListView> {
    return decodeConversationList(await this.bridge.invoke("list_conversations", {}));
  }

  async createConversation(input: CreateConversationInput): Promise<ConversationView> {
    return decodeConversation(await this.bridge.invoke("create_conversation", { command: {
      title: input.title?.trim() || null,
      defaultRun: input.defaultRun ?? null,
      idempotencyKey: createIdempotencyKey(),
    } }));
  }

  async getConversation(id: string): Promise<ConversationView> {
    return decodeConversation(await this.bridge.invoke("get_conversation", { conversationId: id }));
  }

  async getTimeline(id: string): Promise<ConversationTimelineView> {
    return decodeTimeline(await this.bridge.invoke("get_conversation_timeline", { conversationId: id }));
  }

  async listRolePlayGraphOptions(): Promise<RolePlayGraphOptionView[]> {
    return decodeRolePlayGraphOptions(await this.bridge.invoke("list_roleplay_graph_options", {}));
  }

  async getTurnCandidates(turnId: string): Promise<ConversationTurnView> {
    return decodeTurnCandidates(await this.bridge.invoke("get_turn_candidates", { turnId }));
  }

  async updateConversationRunProfile(
    id: string,
    input: UpdateConversationRunProfileInput,
  ): Promise<ConversationRunProfile> {
    return decodeRunProfile(await this.bridge.invoke("update_conversation_run_profile", { command: {
      conversationId: id,
      expectedRevisionNo: input.expectedRevisionNo,
      run: input.run,
      idempotencyKey: createIdempotencyKey(),
    } }));
  }

  async submitConversationTurn(
    id: string,
    input: SubmitConversationTurnInput,
  ): Promise<SubmitConversationTurnAck> {
    return decodeSubmitTurnAck(await this.bridge.invoke("submit_conversation_turn", { command: {
      conversationId: id,
      ...input,
      idempotencyKey: createIdempotencyKey(),
    } }));
  }

  async regenerateConversationCandidate(
    turnId: string,
    input: RegenerateConversationCandidateInput,
  ): Promise<RegenerateConversationCandidateAck> {
    return decodeRegenerateCandidateAck(await this.bridge.invoke("regenerate_conversation_candidate", { command: {
      turnId,
      ...input,
      idempotencyKey: createIdempotencyKey(),
    } }));
  }

  async selectConversationCandidate(
    turnId: string,
    input: SelectConversationCandidateInput,
  ): Promise<ConversationSelectionView> {
    return decodeConversationSelection(await this.bridge.invoke("select_conversation_candidate", { command: {
      turnId,
      ...input,
      idempotencyKey: createIdempotencyKey(),
    } }));
  }

  async resolveCandidateProjection(
    turnId: string,
    runId: string,
    input: ResolveCandidateProjectionInput,
    options: ConversationCommandOptions = {},
  ): Promise<CandidateProjectionResolutionView> {
    const result = decodeCandidateProjectionResolution(await this.bridge.invoke("resolve_candidate_projection", { command: {
      turnId,
      runId,
      ...input,
      idempotencyKey: options.idempotencyKey ?? createIdempotencyKey(),
    } }));
    if (result.turnId !== turnId || result.runId !== runId) {
      throw new DecodeError("candidateProjectionResolution");
    }
    return result;
  }

  async listContextBranches(contextId: string): Promise<ContextBranchView[]> {
    return decodeContextBranches(await this.bridge.invoke("list_context_branches", { contextId }));
  }
}
