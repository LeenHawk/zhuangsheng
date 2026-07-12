import {
  decodeConversation,
  decodeConversationList,
  decodeConversationSelection,
  decodeRegenerateCandidateAck,
  decodeRunProfile,
  decodeSubmitTurnAck,
  decodeTimeline,
} from "./decode";
import { DecodeError } from "./decode-error";
import { decodeRolePlayGraphOptions } from "./decode-roleplay";
import { decodeCandidateProjectionResolution, decodeTurnCandidates } from "./decode-turn";
import { requestJson } from "./http-json";
import { createIdempotencyKey } from "./idempotency";
import type {
  CandidateProjectionResolutionView,
  ConversationListView,
  ConversationRunProfile,
  ConversationRunSpec,
  ConversationSelectionView,
  ConversationTimelineView,
  ConversationTurnView,
  ConversationView,
  LlmContentPart,
  RegenerateConversationCandidateAck,
  ResolveCandidateProjectionInput,
  RolePlayGraphOptionView,
  SubmitConversationTurnAck,
} from "./types";

export interface CreateConversationInput {
  title?: string;
  defaultRun?: ConversationRunSpec;
}

export interface UpdateConversationRunProfileInput {
  expectedRevisionNo: number;
  run: ConversationRunSpec;
}

export interface SubmitConversationTurnInput {
  expectedHeadCommitId: string;
  userContent: LlmContentPart[];
  run: ConversationRunSpec;
}

export interface RegenerateConversationCandidateInput {
  expectedUserCommitId: string;
  run: ConversationRunSpec;
}

export interface SelectConversationCandidateInput {
  selectedRunId: string;
  expectedConversationHeadCommitId: string;
}

export interface ConversationCommandOptions {
  idempotencyKey?: string;
  signal?: AbortSignal;
}

export class HttpConversationClient {
  constructor(private readonly baseUrl = "") {}

  async listConversations(signal?: AbortSignal): Promise<ConversationListView> {
    return decodeConversationList(await this.request("/v1/conversations", { signal }));
  }

  async createConversation(input: CreateConversationInput): Promise<ConversationView> {
    return decodeConversation(await this.request("/v1/conversations", {
      method: "POST",
      headers: { "content-type": "application/json", "idempotency-key": createIdempotencyKey() },
      body: JSON.stringify({ title: input.title?.trim() || null, defaultRun: input.defaultRun ?? null }),
    }));
  }

  async getConversation(id: string, signal?: AbortSignal): Promise<ConversationView> {
    return decodeConversation(await this.request(`/v1/conversations/${encodeURIComponent(id)}`, { signal }));
  }

  async getTimeline(id: string, signal?: AbortSignal): Promise<ConversationTimelineView> {
    return decodeTimeline(await this.request(`/v1/conversations/${encodeURIComponent(id)}/turns`, { signal }));
  }

  async listRolePlayGraphOptions(signal?: AbortSignal): Promise<RolePlayGraphOptionView[]> {
    return decodeRolePlayGraphOptions(await this.request("/v1/roleplay/graph-options", { signal }));
  }

  async getTurnCandidates(turnId: string, signal?: AbortSignal): Promise<ConversationTurnView> {
    return decodeTurnCandidates(await this.request(
      `/v1/turns/${encodeURIComponent(turnId)}/candidates`,
      { signal },
    ));
  }

  async updateConversationRunProfile(
    id: string,
    input: UpdateConversationRunProfileInput,
  ): Promise<ConversationRunProfile> {
    return decodeRunProfile(await this.request(`/v1/conversations/${encodeURIComponent(id)}/run-profile`, {
      method: "PUT",
      headers: this.commandHeaders(),
      body: JSON.stringify(input),
    }));
  }

  async submitConversationTurn(
    id: string,
    input: SubmitConversationTurnInput,
    signal?: AbortSignal,
  ): Promise<SubmitConversationTurnAck> {
    return decodeSubmitTurnAck(await this.request(`/v1/conversations/${encodeURIComponent(id)}/turns`, {
      method: "POST", headers: this.commandHeaders(), body: JSON.stringify(input), signal,
    }));
  }

  async regenerateConversationCandidate(
    turnId: string,
    input: RegenerateConversationCandidateInput,
    signal?: AbortSignal,
  ): Promise<RegenerateConversationCandidateAck> {
    return decodeRegenerateCandidateAck(await this.request(`/v1/turns/${encodeURIComponent(turnId)}/regenerations`, {
      method: "POST", headers: this.commandHeaders(), body: JSON.stringify(input), signal,
    }));
  }

  async selectConversationCandidate(
    turnId: string,
    input: SelectConversationCandidateInput,
  ): Promise<ConversationSelectionView> {
    return decodeConversationSelection(await this.request(`/v1/turns/${encodeURIComponent(turnId)}/selection`, {
      method: "PUT", headers: this.commandHeaders(), body: JSON.stringify(input),
    }));
  }

  async resolveCandidateProjection(
    turnId: string,
    runId: string,
    input: ResolveCandidateProjectionInput,
    options: ConversationCommandOptions = {},
  ): Promise<CandidateProjectionResolutionView> {
    const result = decodeCandidateProjectionResolution(await this.request(
      `/v1/turns/${encodeURIComponent(turnId)}/candidates/${encodeURIComponent(runId)}/projection-resolution`,
      {
        method: "POST",
        headers: this.commandHeaders(options.idempotencyKey),
        body: JSON.stringify(input),
        signal: options.signal,
      },
    ));
    if (result.turnId !== turnId || result.runId !== runId) {
      throw new DecodeError("candidateProjectionResolution");
    }
    return result;
  }

  private commandHeaders(key = createIdempotencyKey()): Record<string, string> {
    return { "content-type": "application/json", "idempotency-key": key };
  }

  private request(path: string, init: RequestInit): Promise<unknown> {
    return requestJson(this.baseUrl, path, init);
  }
}
