import { describe, expect, it } from "vitest";

import {
  isLosslessNumber,
  parseJsonExact,
  stringifyJsonExact,
} from "./exact-json";

describe("bounded exact JSON", () => {
  it("round-trips unsafe integers, long decimals, and exponent values without Number", () => {
    const source = "{\"unsafeInteger\":9007199254740993,\"decimal\":1.2345678901234567890123456789,\"exponent\":12345678901234567890e-17}";
    const value = parseJsonExact(source) as Record<string, unknown>;

    expect(isLosslessNumber(value.unsafeInteger)).toBe(true);
    expect(isLosslessNumber(value.decimal)).toBe(true);
    expect(isLosslessNumber(value.exponent)).toBe(true);
    expect(stringifyJsonExact(value)).toBe(source);
  });

  it("keeps protocol-safe integers as regular numbers", () => {
    expect(parseJsonExact("{\"sequence\":42}"))
      .toEqual({ sequence: 42 });
  });

  it("rejects duplicate keys and bounded-number violations", () => {
    expect(() => parseJsonExact("{\"a\":1,\"a\":2}"))
      .toThrow(/duplicate JSON key/);
    expect(() => parseJsonExact(`{\"n\":${"1".repeat(129)}}`))
      .toThrow(/digit limit/);
    expect(() => parseJsonExact("{\"n\":1e1025}"))
      .toThrow(/exponent limit/);
    expect(() => parseJsonExact("{\"n\":0e1025}"))
      .toThrow(/exponent limit/);
    expect(() => parseJsonExact("{\"n\":0.0e-1024}"))
      .not.toThrow();
    expect(() => stringifyJsonExact({ n: 9_007_199_254_740_992 }))
      .toThrow(/unsafe or non-finite/);
    expect(() => stringifyJsonExact({ n: Number.NaN }))
      .toThrow(/unsafe or non-finite/);
  });
});
