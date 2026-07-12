import { afterEach, describe, expect, it, vi } from "vitest";

import { HttpRuntimeClient } from "./http-runtime-client";
import { HttpSecretClient } from "./http-secret-client";

describe("runtime action clients", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("uses delivery idempotency for approval and explicit keys for secret commands", async () => {
    const calls: Array<{ input: RequestInfo | URL; init?: RequestInit }> = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ input, init });
      const payload = calls.length === 1
        ? {
            waitId: "wait/1",
            deliveryId: "delivery_1",
            status: "resolved",
            preparedToolCallIds: ["tool_1"],
            deniedToolCallIds: [],
            replayed: false,
          }
        : {
            storeId: "store_1",
            formatVersion: 1,
            sessionId: "session_1",
            expiresAt: 100,
          };
      return new Response(JSON.stringify(payload), { status: 200 });
    });
    const runtime = new HttpRuntimeClient("https://roleplay.example");
    const secrets = new HttpSecretClient("https://roleplay.example");

    await runtime.submitToolApproval("wait/1", {
      deliveryId: "delivery_1",
      decisions: [{
        toolCallId: "tool_1",
        callDigest: "sha256:call",
        decision: "approve",
      }],
    });
    await secrets.unlock({ masterPassword: "correct horse battery staple", idempotencyKey: "unlock_1" });

    expect(calls.map((call) => call.input)).toEqual([
      "https://roleplay.example/v1/waits/wait%2F1/responses",
      "https://roleplay.example/v1/secret-store/unlock",
    ]);
    expect(JSON.parse(calls[0]?.init?.body as string)).toEqual({
      deliveryId: "delivery_1",
      response: {
        type: "blocker_decisions",
        decisions: [{
          kind: "tool_call",
          blockerId: "tool_1",
          callDigest: "sha256:call",
          decision: "approve",
          reason: null,
        }],
      },
    });
    expect(calls[0]?.init?.headers).toEqual({ "content-type": "application/json" });
    expect(calls[1]?.init?.headers).toMatchObject({ "idempotency-key": "unlock_1" });
  });
});
