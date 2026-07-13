import {
  isLosslessNumber as isLibraryLosslessNumber,
  LosslessNumber,
  parse,
  stringify,
} from "lossless-json";
import { rejectDuplicateKeys } from "./json-duplicate-keys";

export { LosslessNumber };

export const MAX_JSON_BYTES = 16 * 1024 * 1024;
export const MAX_NUMBER_DIGITS = 128;
export const MAX_EXPONENT_MAGNITUDE = 1024;
export const MAX_JSON_DEPTH = 128;
export const MAX_COLLECTION_ITEMS = 100_000;
export const MAX_STRING_BYTES = 8 * 1024 * 1024;

export type JsonNumber = number | LosslessNumber;

export function parseJsonExact(text: string): unknown {
  if (new TextEncoder().encode(text).byteLength > MAX_JSON_BYTES) {
    throw new SyntaxError("JSON exceeds the 16 MiB limit");
  }
  rejectDuplicateKeys(text);
  const value = parse(text, undefined, {
    parseNumber: parseBoundedNumber,
    onDuplicateKey: ({ key }) => {
      throw new SyntaxError(`duplicate JSON key: ${key}`);
    },
  });
  validateJsonTree(value, 0, new WeakSet());
  return value;
}

export function stringifyJsonExact(value: unknown, space?: number | string): string {
  validateJsonTree(value, 0, new WeakSet(), true);
  const text = stringify(value, undefined, space);
  if (text === undefined) throw new TypeError("value is not JSON serializable");
  if (new TextEncoder().encode(text).byteLength > MAX_JSON_BYTES) {
    throw new TypeError("JSON exceeds the 16 MiB limit");
  }
  parseJsonExact(text);
  return text;
}

export function isLosslessNumber(value: unknown): value is LosslessNumber {
  return isLibraryLosslessNumber(value);
}

export function isJsonNumber(value: unknown): value is JsonNumber {
  if (typeof value === "number") {
    return Number.isFinite(value) && (!Number.isInteger(value) || Number.isSafeInteger(value));
  }
  if (!isLosslessNumber(value)) return false;
  try {
    validateNumberLexeme(value.value);
    return true;
  } catch {
    return false;
  }
}

export function jsonNumberText(value: JsonNumber): string {
  if (!isJsonNumber(value)) throw new TypeError("invalid JSON number");
  return typeof value === "number" ? String(value) : value.value;
}

function parseBoundedNumber(value: string): JsonNumber {
  validateNumberLexeme(value);
  if (/^-?(?:0|[1-9]\d*)$/.test(value)) {
    const integer = Number(value);
    if (Number.isSafeInteger(integer)) return integer;
  }
  return new LosslessNumber(value);
}

function validateNumberLexeme(value: string): void {
  const match = /^(-?)(0|[1-9]\d*)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/.exec(value);
  if (!match) throw new SyntaxError("invalid JSON number");
  const fraction = match[3] ?? "";
  const coefficient = `${match[2]}${fraction}`;
  if (coefficient.length > MAX_NUMBER_DIGITS) {
    throw new SyntaxError("JSON number exceeds the digit limit");
  }

  const explicitExponent = boundedExponent(match[4] ?? "0");
  let normalizedExponent = explicitExponent - fraction.length;
  const significant = coefficient.replace(/^0+/, "");
  if (significant === "") {
    normalizedExponent = 0;
  } else {
    const normalizedDigits = significant.replace(/0+$/, "");
    const trailingZeros = significant.length - normalizedDigits.length;
    normalizedExponent += trailingZeros + normalizedDigits.length - 1;
  }
  if (Math.abs(normalizedExponent) > MAX_EXPONENT_MAGNITUDE) {
    throw new SyntaxError("JSON number exceeds the normalized exponent limit");
  }
}

function boundedExponent(value: string): number {
  const sign = value.startsWith("-") ? -1 : 1;
  const digits = value.replace(/^[+-]?0*/, "");
  if (digits.length > 4) throw new SyntaxError("JSON number exceeds the exponent limit");
  const magnitude = digits === "" ? 0 : Number(digits);
  if (magnitude > MAX_EXPONENT_MAGNITUDE) {
    throw new SyntaxError("JSON number exceeds the exponent limit");
  }
  return sign * magnitude;
}

function validateJsonTree(
  value: unknown,
  depth: number,
  seen: WeakSet<object>,
  allowObjectUndefined = false,
): void {
  if (depth > MAX_JSON_DEPTH) throw new TypeError("JSON exceeds the depth limit");
  if (value === null || typeof value === "boolean") return;
  if (typeof value === "string") {
    validateUnicodeScalars(value);
    if (new TextEncoder().encode(value).byteLength > MAX_STRING_BYTES) {
      throw new TypeError("JSON string exceeds the byte limit");
    }
    return;
  }
  if (isJsonNumber(value)) return;
  if (typeof value === "number") throw new TypeError("unsafe or non-finite JSON number");
  if (typeof value !== "object") throw new TypeError("value is not JSON serializable");
  if (seen.has(value)) throw new TypeError("circular JSON value");

  const entries = Array.isArray(value) ? value.entries() : Object.entries(value);
  const size = Array.isArray(value) ? value.length : Object.keys(value).length;
  if (size > MAX_COLLECTION_ITEMS) throw new TypeError("JSON collection exceeds the item limit");
  if (!Array.isArray(value)) {
    const prototype = Object.getPrototypeOf(value);
    if (prototype !== Object.prototype && prototype !== null) {
      throw new TypeError("value is not a plain JSON object");
    }
  }
  seen.add(value);
  for (const [key, nested] of entries) {
    if (typeof key === "string") {
      validateUnicodeScalars(key);
      if (new TextEncoder().encode(key).byteLength > MAX_STRING_BYTES) {
        throw new TypeError("JSON string exceeds the byte limit");
      }
    }
    if (nested === undefined && allowObjectUndefined && !Array.isArray(value)) continue;
    validateJsonTree(nested, depth + 1, seen, allowObjectUndefined);
  }
  seen.delete(value);
}

function validateUnicodeScalars(value: string): void {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) throw new TypeError("unpaired JSON surrogate");
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      throw new TypeError("unpaired JSON surrogate");
    }
  }
}
