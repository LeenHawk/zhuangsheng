import { afterEach, describe, expect, it, vi } from "vitest";

import { decodeMemoryProposalList } from "./decode-memory";
import { HttpMemoryClient } from "./http-memory-client";

const proposal = {
  id: "proposal_1", scopeId: "story_1", memoryId: "memory_1", expectedHeadCommitId: null,
  changeType: "create", contentRef: "object_1", proposedContent: { schemaVersion: 1, text: "Alice likes tea", tags: ["preference"], attributes: {} },
  reason: "Remember a stated preference", evidenceRefs: ["message_1"], requestedBy: { kind: "user", id: "user_1" },
  schemaVersion: 1, policyVersion: 1, originRunId: null, originNodeInstanceId: null, appliedCommitId: null,
  status: "awaiting_review", createdAt: 1, updatedAt: 2,
};

describe("memory API", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("decodes review content and stable pagination cursor", () => {
    expect(decodeMemoryProposalList({ proposals: [proposal], nextCursor: { updatedAt: 2, id: "proposal_1" } })).toMatchObject({
      proposals: [{ proposedContent: { text: "Alice likes tea" }, status: "awaiting_review" }],
      nextCursor: { updatedAt: 2, id: "proposal_1" },
    });
  });

  it("sends proposal policy material and idempotency separately", async () => {
    let call: { input: RequestInfo | URL; init?: RequestInit } | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      call = { input, init };
      return new Response(JSON.stringify(proposal), { status: 201 });
    });
    await new HttpMemoryClient("https://memory.example").propose({
      scopeId: "story_1", memoryId: null, expectedHeadCommitId: null,
      change: { type: "create", content: { schemaVersion: 1, text: "Alice likes tea", tags: ["preference"], attributes: {} } },
      reason: "Remember a stated preference", evidenceRefs: ["message_1"], idempotencyKey: "proposal-key",
    });
    const request = call as unknown as { input: RequestInfo | URL; init: RequestInit };
    const body = JSON.parse(request.init.body as string);
    expect(request.init.headers).toMatchObject({ "idempotency-key": "proposal-key" });
    expect(body).toMatchObject({ requestedBy: { kind: "user", id: "local-user" }, schemaVersion: 1, policyVersion: 1 });
    expect(body).not.toHaveProperty("idempotencyKey");
  });

  it("loads an exact memory record without searching a mutable projection", async () => {
    let requested: RequestInfo | URL | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      requested = input;
      return Response.json({
        id: "memory/1", scopeId: "story_1", status: "active",
        headCommitId: "commit_1", contentRef: "object_1",
        content: proposal.proposedContent, createdAt: 1, updatedAt: 2,
      });
    });
    const record = await new HttpMemoryClient("https://memory.example").get("memory/1");
    expect(requested).toBe("https://memory.example/v1/memories/memory%2F1");
    expect(record.content?.text).toBe("Alice likes tea");
  });
});
