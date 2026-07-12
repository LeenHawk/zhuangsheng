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
            decidedMemoryProposalIds: [],
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

  it("resolves an unknown effect through the dedicated encoded command route", async () => {
    let call: { input: RequestInfo | URL; init?: RequestInit } | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      call = { input, init };
      return Response.json({
        resolutionId: "resolution_1",
        effectId: "effect/1",
        effectAttemptId: "attempt_1",
        waitId: "wait_1",
        kind: "abort_run",
        replayed: false,
      });
    });

    await new HttpRuntimeClient("https://roleplay.example").resolveEffectUnknown("effect/1", {
      expectedEffectAttemptId: "attempt_1",
      expectedRunControlEpoch: 2,
      kind: "abort_run",
      decision: { reason: "operator chose isolation" },
      resultObjectId: null,
      evidenceObjectId: null,
      idempotencyKey: "resolution-key",
    });

    const request = call as unknown as { input: RequestInfo | URL; init: RequestInit };
    expect(request.input).toBe("https://roleplay.example/v1/effects/effect%2F1/resolution");
    expect(request.init.headers).toMatchObject({ "idempotency-key": "resolution-key" });
    expect(JSON.parse(request.init.body as string)).not.toHaveProperty("idempotencyKey");
  });

  it("submits memory proposal decisions as their own blocker variant", async () => {
    let body: unknown;
    vi.stubGlobal("fetch", async (_input: RequestInfo | URL, init?: RequestInit) => {
      body = JSON.parse(init?.body as string);
      return Response.json({
        waitId: "wait_1", deliveryId: "delivery_1", status: "resolved",
        preparedToolCallIds: [], deniedToolCallIds: [],
        decidedMemoryProposalIds: ["proposal_1"], replayed: false,
      });
    });
    await new HttpRuntimeClient("https://roleplay.example").submitMemoryProposalDecisions("wait_1", {
      deliveryId: "delivery_1",
      decisions: [{ proposalId: "proposal_1", decision: "approve" }],
    });
    expect(body).toEqual({
      deliveryId: "delivery_1",
      response: { type: "blocker_decisions", decisions: [{
        kind: "memory_proposal", blockerId: "proposal_1", decision: "approve",
      }] },
    });
  });
});
