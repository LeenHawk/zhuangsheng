import type { ApiErrorBody } from "./types";

export class ApiError extends Error {
  constructor(readonly status: number, readonly body: ApiErrorBody) {
    super(body.message);
    this.name = "ApiError";
  }
}

export const apiErrorFromPayload = (status: number, payload: unknown): ApiError => {
  const envelope = payload as { error?: Partial<ApiErrorBody> } | null;
  const error = envelope?.error;
  return new ApiError(status, {
    code: typeof error?.code === "string" ? error.code : "invalid_error_response",
    message: typeof error?.message === "string" ? error.message : "The server returned an invalid error.",
    retryable: error?.retryable === true,
    traceId: typeof error?.traceId === "string" ? error.traceId : "trace_unavailable",
    details: error?.details,
  });
};
