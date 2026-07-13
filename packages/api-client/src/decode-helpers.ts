import { DecodeError } from "./decode-error";
import { isJsonNumber } from "./exact-json";
import type { JsonObject, JsonValue } from "./graph-types";

export const record = (value: unknown, path: string): Record<string, unknown> => {
  if (typeof value !== "object" || value === null || Array.isArray(value) || isJsonNumber(value)) {
    throw new DecodeError(path);
  }
  return value as Record<string, unknown>;
};

export const string = (value: unknown, path: string): string => {
  if (typeof value !== "string") throw new DecodeError(path);
  return value;
};

export const number = (value: unknown, path: string): number => {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    throw new DecodeError(path);
  }
  return value;
};

export const boolean = (value: unknown, path: string): boolean => {
  if (typeof value !== "boolean") throw new DecodeError(path);
  return value;
};

export const nullableString = (value: unknown, path: string): string | null =>
  value === null ? null : string(value, path);

export const stringArray = (value: unknown, path: string): string[] => {
  if (!Array.isArray(value)) throw new DecodeError(path);
  return value.map((item, index) => string(item, `${path}[${index}]`));
};

export const jsonValue = (value: unknown, path: string): JsonValue => {
  if (value === null || typeof value === "string" || typeof value === "boolean") return value;
  if (isJsonNumber(value)) return value;
  if (Array.isArray(value)) {
    return value.map((item, index) => jsonValue(item, `${path}[${index}]`));
  }
  const item = record(value, path);
  return Object.fromEntries(
    Object.entries(item).map(([key, nested]) => [key, jsonValue(nested, `${path}.${key}`)]),
  );
};

export const jsonObject = (value: unknown, path: string): JsonObject => {
  const decoded = jsonValue(value, path);
  if (decoded === null || Array.isArray(decoded) || typeof decoded !== "object" || isJsonNumber(decoded)) {
    throw new DecodeError(path);
  }
  return decoded;
};
