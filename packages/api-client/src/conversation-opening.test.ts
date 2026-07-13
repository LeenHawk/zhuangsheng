import { describe, expect, it, vi } from "vitest";

import { createOpeningConversation } from "./conversation-opening";

const run = { graphRevisionId: "revision_1", replyOutputKey: "reply", inputShape: "conversation_message_v1" as const };
const conversation = {
  id: "conversation_1", title: "Story", contextId: "context_1", activeBranchId: "branch_1",
  activeHeadCommitId: "root_commit", runProfile: { ...run, revisionNo: 1 }, createdAt: 1, updatedAt: 1,
};

describe("createOpeningConversation", () => {
  it("binds the first turn to the returned root head and stable retry keys", async () => {
    const createConversation = vi.fn(async () => conversation);
    const submitConversationTurn = vi.fn(async () => ({ turnId: "turn_1", runId: "run_1", status: "running" as const }));
    const result = await createOpeningConversation({ createConversation, submitConversationTurn }, {
      title: "Story", run, openingMessage: "  Begin here.  ",
    }, { conversation: "conversation-key", turn: "turn-key" });
    expect(createConversation).toHaveBeenCalledWith({ title: "Story", defaultRun: run }, { idempotencyKey: "conversation-key" });
    expect(submitConversationTurn).toHaveBeenCalledWith("conversation_1", {
      expectedHeadCommitId: "root_commit", userContent: [{ type: "text", text: "Begin here." }], run,
    }, { idempotencyKey: "turn-key" });
    expect(result.firstTurn.runId).toBe("run_1");
  });
});
