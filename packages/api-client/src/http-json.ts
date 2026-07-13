import { apiErrorFromPayload } from "./api-error";
import { parseJsonExact } from "./exact-json";

export async function requestJson(
  baseUrl: string,
  path: string,
  init: RequestInit,
): Promise<unknown> {
  const response = await fetch(`${baseUrl}${path}`, init);
  const text = await response.text();
  let payload: unknown = null;
  try { payload = text === "" ? null : parseJsonExact(text); } catch { /* decoded below */ }
  if (!response.ok) throw apiErrorFromPayload(response.status, payload);
  return payload;
}
