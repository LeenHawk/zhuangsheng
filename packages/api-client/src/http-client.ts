import {
  decodeConversation,
  decodeConversationList,
  decodeRunProfile,
  decodeSubmitTurnAck,
  decodeTimeline,
} from "./decode";
import { decodeRolePlayGraphOptions } from "./decode-roleplay";
import type {
  ApiErrorBody,
  ConversationListView,
  ConversationRunProfile,
  ConversationRunSpec,
  ConversationTimelineView,
  ConversationView,
  LlmContentPart,
  RolePlayGraphOptionView,
  SubmitConversationTurnAck,
} from "./types";

export class ApiError extends Error {
  constructor(readonly status: number, readonly body: ApiErrorBody) {
    super(body.message);
    this.name = "ApiError";
  }
}

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

  private async request(path: string, init: RequestInit): Promise<unknown> {
    const response = await fetch(`${this.baseUrl}${path}`, init);
    const payload: unknown = await response.json().catch(() => null);
    if (!response.ok) {
      const envelope = payload as { error?: Partial<ApiErrorBody> } | null;
      const error = envelope?.error;
      throw new ApiError(response.status, {
        code: typeof error?.code === "string" ? error.code : "invalid_error_response",
        message: typeof error?.message === "string" ? error.message : "The server returned an invalid error.",
        retryable: error?.retryable === true,
        traceId: typeof error?.traceId === "string" ? error.traceId : "trace_unavailable",
        details: error?.details,
      });
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
