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
import { decodeTurnCandidates } from "./decode-turn";
import { requestJson } from "./http-json";
import { HttpGraphClient } from "./http-graph-client";
import { HttpConfigClient } from "./http-config-client";
import { HttpRuntimeClient } from "./http-runtime-client";
import { HttpMemoryClient } from "./http-memory-client";
import { HttpArtifactClient } from "./http-artifact-client";
import { HttpSecretClient } from "./http-secret-client";
import { HttpContextClient } from "./http-context-client";
import { createIdempotencyKey } from "./idempotency";
import type {
  ConversationListView,
  ConversationRunProfile,
  ConversationRunSpec,
  ConversationSelectionView,
  ConversationTimelineView,
  ConversationTurnView,
  ConversationView,
  LlmContentPart,
  RegenerateConversationCandidateAck,
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

export class HttpApiClient {
  readonly runtime: HttpRuntimeClient;
  readonly secrets: HttpSecretClient;
  readonly graphs: HttpGraphClient;
  readonly config: HttpConfigClient;
  readonly memory: HttpMemoryClient;
  readonly artifacts: HttpArtifactClient;
  readonly contexts: HttpContextClient;

  constructor(private readonly baseUrl = "") {
    this.runtime = new HttpRuntimeClient(baseUrl);
    this.secrets = new HttpSecretClient(baseUrl);
    this.graphs = new HttpGraphClient(baseUrl);
    this.config = new HttpConfigClient(baseUrl);
    this.memory = new HttpMemoryClient(baseUrl);
    this.artifacts = new HttpArtifactClient(baseUrl);
    this.contexts = new HttpContextClient(baseUrl);
  }

  async listConversations(signal?: AbortSignal): Promise<ConversationListView> {
    return decodeConversationList(await this.request("/v1/conversations", { signal }));
  }

  async createConversation(input: CreateConversationInput): Promise<ConversationView> {
    return decodeConversation(await this.request("/v1/conversations", {
      method: "POST",
      headers: { "content-type": "application/json", "idempotency-key": createIdempotencyKey() },
      body: JSON.stringify({
        title: input.title?.trim() || null,
        defaultRun: input.defaultRun ?? null,
      }),
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
      headers: { "content-type": "application/json", "idempotency-key": createIdempotencyKey() },
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
      headers: { "content-type": "application/json", "idempotency-key": createIdempotencyKey() },
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
      headers: { "content-type": "application/json", "idempotency-key": createIdempotencyKey() },
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
      headers: { "content-type": "application/json", "idempotency-key": createIdempotencyKey() },
      body: JSON.stringify(input),
    }));
  }

  private async request(path: string, init: RequestInit): Promise<unknown> {
    return requestJson(this.baseUrl, path, init);
  }
}
