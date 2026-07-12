import { decodeConversation, decodeConversationList, decodeTimeline } from "./decode";
import type {
  ApiErrorBody,
  ConversationListView,
  ConversationTimelineView,
  ConversationView,
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
