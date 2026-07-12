import { DecodeError } from "./decode-error";

export const record = (value: unknown, path: string): Record<string, unknown> => {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
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
