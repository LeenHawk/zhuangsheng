import {
  decodeConversation,
  decodeConversationList,
  decodeConversationSelection,
  decodeRegenerateCandidateAck,
  decodeRunProfile,
  decodeSubmitTurnAck,
  decodeTimeline,
} from "./decode";
import { decodeRolePlayGraphOptions } from "./decode-roleplay";
import { apiErrorFromPayload } from "./api-error";
import { streamRunEvents, type RunEventStreamObserver } from "./http-sse";
import type {
  ConversationListView,
  ConversationRunProfile,
  ConversationRunSpec,
  ConversationSelectionView,
  ConversationTimelineView,
  ConversationView,
  LlmContentPart,
  RegenerateConversationCandidateAck,
  RolePlayGraphOptionView,
  SubmitConversationTurnAck,
} from "./types";

export interface CreateConversationInput {
  title?: string;
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

export class HttpApiClient {
  constructor(private readonly baseUrl = "") {}

  async listConversations(signal?: AbortSignal): Promise<ConversationListView> {
    return decodeConversationList(await this.request("/v1/conversations", { signal }));
  }

  async createConversation(input: CreateConversationInput): Promise<ConversationView> {
    return decodeConversation(await this.request("/v1/conversations", {
      method: "POST",
      headers: { "content-type": "application/json", "idempotency-key": idempotencyKey() },
      body: JSON.stringify({ title: input.title?.trim() || null, defaultRun: null }),
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

  async updateConversationRunProfile(
    id: string,
    input: UpdateConversationRunProfileInput,
  ): Promise<ConversationRunProfile> {
    return decodeRunProfile(await this.request(`/v1/conversations/${encodeURIComponent(id)}/run-profile`, {
      method: "PUT",
      headers: { "content-type": "application/json", "idempotency-key": idempotencyKey() },
      body: JSON.stringify(input),
    }));
  }

  async submitConversationTurn(
    id: string,
    input: SubmitConversationTurnInput,
    signal?: AbortSignal,
  ): Promise<SubmitConversationTurnAck> {
    return decodeSubmitTurnAck(await this.request(`/v1/conversations/${encodeURIComponent(id)}/turns`, {
      method: "POST",
      headers: { "content-type": "application/json", "idempotency-key": idempotencyKey() },
      body: JSON.stringify(input),
      signal,
    }));
  }

  async regenerateConversationCandidate(
    turnId: string,
    input: RegenerateConversationCandidateInput,
    signal?: AbortSignal,
  ): Promise<RegenerateConversationCandidateAck> {
    return decodeRegenerateCandidateAck(await this.request(`/v1/turns/${encodeURIComponent(turnId)}/regenerations`, {
      method: "POST",
      headers: { "content-type": "application/json", "idempotency-key": idempotencyKey() },
      body: JSON.stringify(input),
      signal,
    }));
  }

  async selectConversationCandidate(
    turnId: string,
    input: SelectConversationCandidateInput,
  ): Promise<ConversationSelectionView> {
    return decodeConversationSelection(await this.request(`/v1/turns/${encodeURIComponent(turnId)}/selection`, {
      method: "PUT",
      headers: { "content-type": "application/json", "idempotency-key": idempotencyKey() },
      body: JSON.stringify(input),
    }));
  }

  streamRunEvents(
    runId: string,
    afterDurableSeq: number,
    signal: AbortSignal,
    observer: RunEventStreamObserver,
  ): Promise<void> {
    return streamRunEvents(this.baseUrl, runId, afterDurableSeq, signal, observer);
  }

  private async request(path: string, init: RequestInit): Promise<unknown> {
    const response = await fetch(`${this.baseUrl}${path}`, init);
    const payload: unknown = await response.json().catch(() => null);
    if (!response.ok) {
      throw apiErrorFromPayload(response.status, payload);
    }
    return payload;
  }
}

const idempotencyKey = (): string => {
  if (typeof crypto.randomUUID !== "function") {
    throw new Error("This browser cannot generate secure idempotency keys.");
  }
  return crypto.randomUUID();
};
