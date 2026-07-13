import { describe, expect, it } from "vitest";

import {
  DecodeError,
  decodeConversationList,
  decodeSubmitTurnAck,
  decodeTimeline,
} from "./decode";
import { decodeRolePlayGraphOptions } from "./decode-roleplay";
import { decodeTurnCandidates } from "./decode-turn";

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
    const list = decodeConversationList({
      items: [conversation],
      attention: [{
        conversationId: "conversation_1", runId: "run_1", waitId: "wait_1",
        kind: "tool_approval", createdAt: 1_700_000_000_001,
      }],
    });
    expect(list.items[0]?.id).toBe("conversation_1");
    expect(list.attention[0]?.kind).toBe("tool_approval");
    const decoded = decodeTimeline(timeline);
    expect(decoded.messages[0]?.content[0]).toEqual({ type: "text", text: "Open the archive" });
    expect(decoded.turns[0]?.candidates[0]?.status).toBe("running");
  });

  it("fails closed for an unknown candidate status", () => {
    const incompatible = structuredClone(timeline);
    incompatible.turns[0]!.candidates[0]!.status = "future_status";
    expect(() => decodeTimeline(incompatible)).toThrow(DecodeError);
  });

  it("decodes the canonical turn candidate detail independently", () => {
    expect(decodeTurnCandidates(timeline.turns[0]).candidates[0]?.runId).toBe("run_1");
    expect(() => decodeTurnCandidates({ ...timeline.turns[0], candidates: "invalid" }))
      .toThrow(DecodeError);
  });

  it("decodes role play compatibility without inspecting raw graph documents", () => {
    const options = decodeRolePlayGraphOptions([
      {
        graphId: "graph_1",
        graphName: "Archive Role",
        revisionId: "graphrev_1",
        revisionNo: 2,
        replyOutputKeys: ["reply"],
        primaryLlmNodeId: "generate",
        compatibility: {
          mode: "partial",
          profileVersion: 1,
          editableFields: ["model"],
          lockedReasons: ["custom_coordination_nodes"],
        },
      },
    ]);
    expect(options[0]?.compatibility.mode).toBe("partial");
    expect(() =>
      decodeRolePlayGraphOptions([
        {
          ...options[0],
          compatibility: { mode: "editable", profileVersion: 2, editableFields: [] },
        },
      ]),
    ).toThrow(DecodeError);
  });

  it("checks that a submitted candidate and run identify the same durable run", () => {
    expect(
      decodeSubmitTurnAck({
        turn: { id: "turn_1" },
        candidate: { runId: "run_1", status: "running" },
        run: { id: "run_1" },
      }),
    ).toEqual({ turnId: "turn_1", runId: "run_1", status: "running" });
    expect(() =>
      decodeSubmitTurnAck({
        turn: { id: "turn_1" },
        candidate: { runId: "run_1", status: "running" },
        run: { id: "run_2" },
      }),
    ).toThrow(DecodeError);
  });
});
