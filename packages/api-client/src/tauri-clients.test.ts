import { describe, expect, it } from "vitest";

import { TauriConversationClient } from "./tauri-conversation-client";
import { TauriArtifactClient } from "./tauri-artifact-client";
import { TauriContextClient } from "./tauri-context-client";
import { TauriMemoryClient } from "./tauri-memory-client";
import { TauriRuntimeClient } from "./tauri-runtime-client";
import type { TauriBridge } from "./transport";

describe("Tauri application clients", () => {
  it("maps a conversation turn to the same normalized ack as HTTP", async () => {
    const calls: Array<{ operation: string; payload: unknown }> = [];
    const client = new TauriConversationClient(bridge(calls, {
      turn: { id: "turn_1" },
      candidate: { runId: "run_1", status: "running" },
      run: { id: "run_1" },
    }));
    const result = await client.submitConversationTurn("conversation_1", {
      expectedHeadCommitId: "commit_1",
      userContent: [{ type: "text", text: "hello" }],
      run: { graphRevisionId: "revision_1", replyOutputKey: "reply", inputShape: "conversation_message_v1" },
    }, { idempotencyKey: "turn-key" });
    expect(result).toEqual({ turnId: "turn_1", runId: "run_1", status: "running" });
    expect(calls[0]?.operation).toBe("submit_conversation_turn");
    expect(calls[0]?.payload).toMatchObject({ command: {
      conversationId: "conversation_1",
      expectedHeadCommitId: "commit_1",
      idempotencyKey: "turn-key",
    } });
  });

  it("submits wait and effect DTOs without caller-controlled actor fields", async () => {
    const calls: Array<{ operation: string; payload: unknown }> = [];
    const runtime = new TauriRuntimeClient(bridge(calls, (operation: string) => operation === "satisfy_wait"
      ? {
          waitId: "wait_1", deliveryId: "delivery_1", status: "resolved",
          preparedToolCallIds: [], deniedToolCallIds: [], decidedMemoryProposalIds: [], replayed: false,
        }
      : {
          resolutionId: "resolution_1", effectId: "effect_1", effectAttemptId: "attempt_1",
          waitId: "wait_1", kind: "abort_run", replayed: false,
        }));
    await runtime.submitHumanResponse("wait_1", { deliveryId: "delivery_1", value: { answer: 42 } });
    await runtime.resolveEffectUnknown("effect_1", {
      expectedEffectAttemptId: "attempt_1", expectedRunControlEpoch: 2,
      kind: "abort_run", decision: { reason: "checked" }, resultObjectId: null,
      evidenceObjectId: null, idempotencyKey: "effect-command-1",
    });
    expect(calls[0]?.payload).toEqual({ input: {
      waitId: "wait_1", deliveryId: "delivery_1",
      response: { kind: "value", value: { answer: 42 } },
    } });
    expect(JSON.stringify(calls)).not.toContain("actorKind");
    expect(JSON.stringify(calls)).not.toContain("resolutionId");
  });

  it("does not expose memory actor selection to the IPC caller", async () => {
    const calls: Array<{ operation: string; payload: unknown }> = [];
    const memory = new TauriMemoryClient(bridge(calls, {
      id: "proposal_1", scopeId: "roleplay", memoryId: "memory_1",
      expectedHeadCommitId: null, changeType: "create", contentRef: null,
      proposedContent: { schemaVersion: 1, text: "lore", tags: [], attributes: {} },
      reason: "remember", evidenceRefs: [], requestedBy: { kind: "user", id: "local-user" },
      schemaVersion: 1, policyVersion: 1, originRunId: null, originNodeInstanceId: null,
      appliedCommitId: null, status: "awaiting_review", createdAt: 1, updatedAt: 1,
    }));
    await memory.propose({
      scopeId: "roleplay", memoryId: null, expectedHeadCommitId: null,
      change: { type: "create", content: { schemaVersion: 1, text: "lore", tags: [], attributes: {} } },
      reason: "remember", evidenceRefs: [], idempotencyKey: "memory-command-1",
    });
    expect(JSON.stringify(calls[0]?.payload)).not.toContain("requestedBy");
    expect(JSON.stringify(calls[0]?.payload)).not.toContain("actor");
  });

  it("normalizes context patch authors at the transport boundary", async () => {
    const calls: Array<{ operation: string; payload: unknown }> = [];
    const contexts = new TauriContextClient(bridge(calls, {
      id: "commit_2", contextId: "context_1", branchId: "branch_1", sequenceNo: 2,
      operationId: "operation_1", parentCommitIds: ["commit_1"], patchRef: "object_1",
      schemaVersion: 1, policyVersion: 1, author: { kind: "user", id: "local-user" },
      originRunId: null, originNodeInstanceId: null, createdAt: 2,
    }));
    await contexts.commitPatch("context_1", "branch_1", {
      baseCommitId: "commit_1", operationId: "operation_1",
      ops: [{ op: "add", path: "/fact", value: true }],
      schemaVersion: 1, policyVersion: 1,
    });
    expect(calls[0]?.payload).toMatchObject({ command: { patch: {
      aggregateId: "context_1", lineageKey: "branch_1",
      author: { kind: "user", id: "local-user" },
    } } });
  });

  it("keeps native artifact staging and completion as two explicit phases", async () => {
    const calls: Array<{ operation: string; payload: unknown }> = [];
    const artifacts = new TauriArtifactClient({
      invoke: async <T>(operation: string, payload: unknown) => {
        calls.push({ operation, payload });
        return {
          stagingId: "staging_1",
          status: operation === "create_artifact_staging" ? "uploading" : "validated",
          lifecycleGeneration: operation === "create_artifact_staging" ? 1 : 2,
          byteSize: operation === "create_artifact_staging" ? null : 2,
          contentHash: operation === "create_artifact_staging" ? null : `sha256:${"a".repeat(64)}`,
          validatedMediaType: operation === "create_artifact_staging" ? null : "text/plain",
        } as T;
      },
      listen: async () => () => undefined,
    });
    const result = await artifacts.upload({
      object: new Blob(["\u0001\u0002"], { type: "text/plain" }),
      name: "note.txt", classification: "private", retention: { type: "pinned" },
    });
    expect(result.status).toBe("validated");
    expect(calls.map((call) => call.operation)).toEqual([
      "create_artifact_staging", "complete_artifact_staging",
    ]);
    expect(calls[1]?.payload).toMatchObject({ input: { bytes: [1, 2] } });
  });
});

const bridge = (
  calls: Array<{ operation: string; payload: unknown }>,
  result: unknown | ((operation: string) => unknown),
): TauriBridge => ({
  invoke: async <T>(operation: string, payload: unknown) => {
    calls.push({ operation, payload });
    return (typeof result === "function" ? result(operation) : result) as T;
  },
  listen: async () => () => undefined,
});
