import { afterEach, describe, expect, it, vi } from "vitest";

import { HttpApiClient } from "./http-client";

describe("HttpApiClient conversation commands", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("creates a story with its versioned default run profile", async () => {
    let call: { input: RequestInfo | URL; init?: RequestInit } | null = null;
    vi.stubGlobal("crypto", { randomUUID: () => "conversation-key" });
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      call = { input, init };
      return new Response(JSON.stringify({
        id: "conversation_1",
        title: "The Archive",
        contextId: "context_1",
        activeBranchId: "branch_1",
        activeHeadCommitId: "commit_1",
        runProfile: {
          graphRevisionId: "graphrev_1",
          replyOutputKey: "reply",
          inputShape: "conversation_message_v1",
          revisionNo: 1,
        },
        createdAt: 1,
        updatedAt: 1,
      }), { status: 201 });
    });
    const defaultRun = {
      graphRevisionId: "graphrev_1",
      replyOutputKey: "reply",
      inputShape: "conversation_message_v1" as const,
    };

    await new HttpApiClient("https://roleplay.example").createConversation({
      title: "The Archive",
      defaultRun,
    }, { idempotencyKey: "conversation-key" });

    const request = call as unknown as { input: RequestInfo | URL; init: RequestInit };
    expect(request.input).toBe("https://roleplay.example/v1/conversations");
    expect(request.init.headers).toMatchObject({ "idempotency-key": "conversation-key" });
    expect(JSON.parse(request.init.body as string)).toEqual({
      title: "The Archive",
      defaultRun,
    });
  });

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
    }, { idempotencyKey: "turn-key" });

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
    expect(calls[1]?.init?.headers).toMatchObject({ "idempotency-key": "turn-key" });
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

  it("reads candidates from the canonical encoded turn route", async () => {
    let requested: RequestInfo | URL | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      requested = input;
      return new Response(JSON.stringify({
        id: "turn/1",
        conversationId: "conversation_1",
        userMessageId: "message_1",
        userCommitId: "commit_1",
        createdAt: 1,
        selectedRunId: null,
        candidates: [{
          turnId: "turn/1",
          runId: "run_1",
          branchId: "branch_1",
          baseCommitId: "commit_1",
          replyOutputKey: "reply",
          status: "ready",
          assistantMessageId: "message_2",
          candidateCommitId: "commit_2",
          projectionError: null,
          createdAt: 2,
        }],
      }), { status: 200 });
    });

    const turn = await new HttpApiClient("https://roleplay.example")
      .getTurnCandidates("turn/1");

    expect(requested).toBe("https://roleplay.example/v1/turns/turn%2F1/candidates");
    expect(turn.candidates[0]?.status).toBe("ready");
  });

  it("resolves a conflicted candidate projection through the encoded operator route", async () => {
    let call: { input: RequestInfo | URL; init?: RequestInit } | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      call = { input, init };
      return Response.json({
        turnId: "turn/1",
        runId: "run/1",
        branchId: "branch_1",
        branchHeadCommitId: "commit_2",
        status: "ready",
        assistantMessageId: "message_2",
        candidateCommitId: "commit_2",
        resolvedAt: 3,
      });
    });

    const result = await new HttpApiClient("https://roleplay.example").resolveCandidateProjection(
      "turn/1",
      "run/1",
      {
        expectedCurrentBranchHead: "commit_1",
        resolution: { type: "append_after_current", reason: "reviewed intervening diff" },
      },
      { idempotencyKey: "projection-key" },
    );

    const request = call as unknown as { input: RequestInfo | URL; init: RequestInit };
    expect(request.input).toBe(
      "https://roleplay.example/v1/turns/turn%2F1/candidates/run%2F1/projection-resolution",
    );
    expect(request.init.headers).toMatchObject({ "idempotency-key": "projection-key" });
    expect(JSON.parse(request.init.body as string)).toEqual({
      expectedCurrentBranchHead: "commit_1",
      resolution: { type: "append_after_current", reason: "reviewed intervening diff" },
    });
    expect(result.candidateCommitId).toBe("commit_2");
  });
});
