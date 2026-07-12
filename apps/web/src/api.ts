import { ApiError, HttpApiClient } from "@zhuangsheng/api-client";

export const client = new HttpApiClient(import.meta.env.VITE_API_BASE_URL ?? "");

export function messageFor(cause: unknown): string {
  if (cause instanceof ApiError) {
    return `${cause.body.message}（${cause.body.code} · ${cause.body.traceId}）`;
  }
  return cause instanceof Error ? cause.message : "无法读取服务端响应。";
}
