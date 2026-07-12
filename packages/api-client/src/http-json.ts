import { apiErrorFromPayload } from "./api-error";

export async function requestJson(
  baseUrl: string,
  path: string,
  init: RequestInit,
): Promise<unknown> {
  const response = await fetch(`${baseUrl}${path}`, init);
  const payload: unknown = await response.json().catch(() => null);
  if (!response.ok) throw apiErrorFromPayload(response.status, payload);
  return payload;
}
