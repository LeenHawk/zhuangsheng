import { describe, expect, it } from "vitest";

import { DecodeError, decodeConversationList, decodeTimeline } from "./decode";

const conversation = {
  id: "conversation_1",
  title: "The Archive",
  contextId: "context_1",
  activeBranchId: "branch_1",
  activeHeadCommitId: "commit_2",
  runProfile: null,
  createdAt: 1,
  updatedAt: 2,
};

const timeline = {
  conversationId: "conversation_1",
  activeBranchId: "branch_1",
  activeHeadCommitId: "commit_2",
  messages: [
    {
      id: "message_1",
      turnId: "turn_1",
      branchId: "branch_1",
      commitId: "commit_1",
      parentMessageId: null,
      role: "user",
      source: "user_input",
      content: [{ type: "text", text: "Open the archive" }],
      originRunId: null,
      createdAt: 1,
    },
  ],
  turns: [
    {
      id: "turn_1",
      conversationId: "conversation_1",
      userMessageId: "message_1",
      userCommitId: "commit_1",
      createdAt: 1,
      selectedRunId: null,
      candidates: [
        {
          turnId: "turn_1",
          runId: "run_1",
          branchId: "branch_1",
          baseCommitId: "commit_1",
          replyOutputKey: "reply",
          status: "running",
          assistantMessageId: null,
          candidateCommitId: null,
          projectionError: null,
          createdAt: 2,
        },
      ],
    },
  ],
};

describe("conversation decoders", () => {
  it("decodes the active timeline without passing raw server objects through", () => {
    expect(decodeConversationList({ items: [conversation] }).items[0]?.id).toBe("conversation_1");
    const decoded = decodeTimeline(timeline);
    expect(decoded.messages[0]?.content[0]).toEqual({ type: "text", text: "Open the archive" });
    expect(decoded.turns[0]?.candidates[0]?.status).toBe("running");
  });

  it("fails closed for an unknown candidate status", () => {
    const incompatible = structuredClone(timeline);
    incompatible.turns[0]!.candidates[0]!.status = "future_status";
    expect(() => decodeTimeline(incompatible)).toThrow(DecodeError);
  });
});
