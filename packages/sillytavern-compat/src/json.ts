import {
  isJsonNumber,
  jsonNumberText,
  parseJsonExact,
  stringifyJsonExact,
  type JsonObject,
  type JsonValue,
} from "@zhuangsheng/api-client";

export function object(value: JsonValue | undefined): JsonObject | null {
  return value !== null && value !== undefined && !Array.isArray(value)
    && typeof value === "object" && !isJsonNumber(value) ? value : null;
}

export function string(value: JsonValue | undefined): string | null {
  return typeof value === "string" ? value : null;
}

export function boolean(value: JsonValue | undefined, fallback = false): boolean {
  return typeof value === "boolean" ? value : fallback;
}

export function number(value: JsonValue | undefined): number | null {
  if (!isJsonNumber(value)) return null;
  const decoded = Number(jsonNumberText(value));
  return Number.isFinite(decoded) ? decoded : null;
}

export function integer(value: JsonValue | undefined): number | null {
  const decoded = number(value);
  return decoded !== null && Number.isSafeInteger(decoded) ? decoded : null;
}

export function cloneJson<T extends JsonValue>(value: T): T {
  return parseJsonExact(stringifyJsonExact(value)) as T;
}

export async function canonicalHash(value: JsonValue): Promise<string> {
  const bytes = new TextEncoder().encode(stringifyJsonExact(sortJson(value)));
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return `sha256:${Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

function sortJson(value: JsonValue): JsonValue {
  if (Array.isArray(value)) return value.map(sortJson);
  const record = object(value);
  if (!record) return value;
  return Object.fromEntries(Object.keys(record).sort().map((key) => [key, sortJson(record[key]!)]));
}
