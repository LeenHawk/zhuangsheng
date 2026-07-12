import { ApiError, DecodeError, HttpApiClient } from "@zhuangsheng/api-client";

export const client = new HttpApiClient(import.meta.env.VITE_API_BASE_URL ?? "");

export function messageFor(cause: unknown): string {
  if (cause instanceof ApiError) {
    if (cause.body.code === "not_found") return `资源不存在或已被归档（${cause.body.traceId}）`;
    if (cause.body.code.includes("permission") || cause.body.code === "unauthenticated") return `当前会话没有所需权限（${cause.body.traceId}）`;
    if (cause.body.code.includes("conflict")) return `服务端版本已变化，请刷新后比较再重试（${cause.body.traceId}）`;
    return `${cause.body.message}（${cause.body.code} · ${cause.body.traceId}）`;
  }
  if (cause instanceof DecodeError) return `客户端与服务端投影不兼容（${cause.path}），关键操作已暂停。`;
  if (cause instanceof TypeError) return "当前离线或服务不可达；已显示的数据不会被清空。";
  return cause instanceof Error ? cause.message : "无法读取服务端响应。";
}
