import { afterEach, describe, expect, it, vi } from "vitest";

import { HttpApiClient } from "./http-client";

describe("HttpApiClient conversation commands", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("sends revision CAS and exact run specs to their canonical endpoints", async () => {
    const calls: Array<{ input: RequestInfo | URL; init?: RequestInit }> = [];
    vi.stubGlobal("crypto", { randomUUID: () => "command-key" });
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ input, init });
      const payload = calls.length === 1
        ? {
            graphRevisionId: "graphrev_1",
            replyOutputKey: "reply",
            inputShape: "conversation_message_v1",
            revisionNo: 2,
          }
        : {
            turn: { id: "turn_1" },
            candidate: { runId: "run_1", status: "running" },
            run: { id: "run_1" },
          };
      return new Response(JSON.stringify(payload), {
        status: calls.length === 1 ? 200 : 202,
        headers: { "content-type": "application/json" },
      });
    });

    const client = new HttpApiClient("https://roleplay.example");
    const run = {
      graphRevisionId: "graphrev_1",
      replyOutputKey: "reply",
      inputShape: "conversation_message_v1" as const,
    };
    await client.updateConversationRunProfile("conversation/1", {
      expectedRevisionNo: 1,
      run,
    });
    await client.submitConversationTurn("conversation/1", {
      expectedHeadCommitId: "commit_1",
      userContent: [{ type: "text", text: "Continue" }],
      run,
    });

    expect(calls.map((call) => call.input)).toEqual([
      "https://roleplay.example/v1/conversations/conversation%2F1/run-profile",
      "https://roleplay.example/v1/conversations/conversation%2F1/turns",
    ]);
    expect(JSON.parse(calls[0]?.init?.body as string)).toEqual({ expectedRevisionNo: 1, run });
    expect(JSON.parse(calls[1]?.init?.body as string)).toEqual({
      expectedHeadCommitId: "commit_1",
      userContent: [{ type: "text", text: "Continue" }],
      run,
    });
    expect(calls[0]?.init?.headers).toMatchObject({ "idempotency-key": "command-key" });
    expect(calls[1]?.init?.headers).toMatchObject({ "idempotency-key": "command-key" });
  });

  it("uses the canonical turn routes for regeneration and selection", async () => {
    const calls: Array<{ input: RequestInfo | URL; init?: RequestInit }> = [];
    vi.stubGlobal("crypto", { randomUUID: () => "candidate-key" });
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ input, init });
      const payload = calls.length === 1
        ? {
            candidate: { turnId: "turn/1", runId: "run_2", status: "running" },
            run: { id: "run_2" },
          }
        : {
            turnId: "turn/1",
            selectedRunId: "run_2",
            selectedBranchId: "branch_2",
            selectedCommitId: "commit_2",
            selectedAt: 2,
          };
      return new Response(JSON.stringify(payload), { status: calls.length === 1 ? 202 : 200 });
    });
    const client = new HttpApiClient("https://roleplay.example");
    const run = {
      graphRevisionId: "graphrev_1",
      replyOutputKey: "reply",
      inputShape: "conversation_message_v1" as const,
    };

    await client.regenerateConversationCandidate("turn/1", {
      expectedUserCommitId: "commit_user",
      run,
    });
    await client.selectConversationCandidate("turn/1", {
      selectedRunId: "run_2",
      expectedConversationHeadCommitId: "commit_1",
    });

    expect(calls.map((call) => call.input)).toEqual([
      "https://roleplay.example/v1/turns/turn%2F1/regenerations",
      "https://roleplay.example/v1/turns/turn%2F1/selection",
    ]);
    expect(JSON.parse(calls[0]?.init?.body as string)).toEqual({
      expectedUserCommitId: "commit_user",
      run,
    });
    expect(JSON.parse(calls[1]?.init?.body as string)).toEqual({
      selectedRunId: "run_2",
      expectedConversationHeadCommitId: "commit_1",
    });
  });
});
