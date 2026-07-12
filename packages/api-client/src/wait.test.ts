import { describe, expect, it } from "vitest";

import { DecodeError } from "./decode-error";
import { decodeSecretStoreStatus } from "./decode-secret";
import { decodeOpenWaits } from "./decode-waits";

describe("wait and secret decoders", () => {
  it("binds approval calls to the exact open blocker set", () => {
    const decoded = decodeOpenWaits([approvalWait()], "run_1");
    expect(decoded[0]?.request).toEqual({
      kind: "tool_approval",
      modelCallId: "call_1",
      calls: [{
        toolCallId: "tool_call_1",
        callDigest: "sha256:call",
        riskSummary: "Send the selected message to the external tool",
        expiresAt: 100,
      }],
    });
    const incompatible = approvalWait();
    incompatible.blockers[0]!.id = "different_call";
    expect(() => decodeOpenWaits([incompatible], "run_1")).toThrow(DecodeError);
  });

  it("recognizes a dedicated secret unlock wait without exposing secret material", () => {
    const wait = {
      ...baseWait(),
      kind: "secret_store_unlocked",
      request: {
        schemaVersion: 1,
        kind: "secret_store_unlocked",
        reason: "provider_credential_required",
        channelId: "channel_1",
      },
      blockers: [],
    };
    expect(decodeOpenWaits([wait], "run_1")[0]?.request).toEqual({
      kind: "secret_store_unlocked",
      reason: "provider_credential_required",
      channelId: "channel_1",
    });
  });

  it("binds inspectable memory proposals to their exact blocker set", () => {
    const wait = {
      ...baseWait(),
      kind: "approval",
      request: {
        schemaVersion: 1,
        kind: "memory_proposal_review",
        modelCallId: "call_1",
        proposals: [{ proposalId: "proposal_1", toolCallId: "tool_1", proposal: proposal() }],
      },
      blockers: [{ kind: "memory_proposal", id: "proposal_1", order: 0, status: "open", decisionRef: null }],
    };
    expect(decodeOpenWaits([wait], "run_1")[0]?.request).toMatchObject({
      kind: "memory_proposal_review",
      proposals: [{ proposalId: "proposal_1", proposal: { reason: "Observed preference" } }],
    });
    wait.blockers[0]!.id = "different_proposal";
    expect(() => decodeOpenWaits([wait], "run_1")).toThrow(DecodeError);
  });

  it("binds effect-resolution details to the exact effect blocker", () => {
    const wait = {
      ...baseWait(),
      kind: "effect_resolution",
      request: {
        schemaVersion: 1,
        kind: "effect_resolution",
        effectId: "effect_1",
        effectAttemptId: "effectattempt_1",
        ownerKind: "model_call",
        ownerId: "modelcall_1",
        classification: "idempotent",
        allowedResolutions: ["confirm_succeeded", "confirm_failed_retry_safe", "abort_run"],
      },
      blockers: [{
        kind: "effect", id: "effect_1", order: 0, status: "open", decisionRef: null,
      }],
    };
    const request = decodeOpenWaits([wait], "run_1")[0]?.request;
    expect(request).toMatchObject({
      kind: "effect_resolution",
      effectId: "effect_1",
      effectAttemptId: "effectattempt_1",
      classification: "idempotent",
    });
    wait.blockers[0]!.id = "other_effect";
    expect(() => decodeOpenWaits([wait], "run_1")).toThrow(DecodeError);
  });

  it("decodes the exact schema binding for a human response wait", () => {
    const wait = {
      ...baseWait(),
      request: { schemaVersion: 1, kind: "human_response", title: "Choose", description: "Pick one" },
      responseSchema: {
        schemaVersion: 1,
        dialect: "https://json-schema.org/draft/2020-12/schema",
        validationProfileVersion: 1,
        formatPolicyVersion: 1,
        document: { type: "string", enum: ["left", "right"] },
        limits: { maxSchemaBytes: 1024 },
      },
      responseSchemaCompilation: {
        canonicalDocumentHash: "sha256:document", schemaHash: "sha256:schema",
        canonicalSource: "{}", compiledPayload: "{}", compiledPayloadHash: "sha256:compiled",
        compilerId: "zhuangsheng-json-schema", compilerVersion: "0.1.0", payloadFormatVersion: 1,
      },
    };
    const decoded = decodeOpenWaits([wait], "run_1")[0];
    expect(decoded?.request).toMatchObject({ kind: "human_response", title: "Choose" });
    expect(decoded?.responseSchema?.document).toEqual({ type: "string", enum: ["left", "right"] });
    expect(decoded?.responseSchemaCompilation?.schemaHash).toBe("sha256:schema");
  });

  it("enforces initialized and locked status invariants", () => {
    expect(decodeSecretStoreStatus({
      initialized: true,
      storeId: "secretstore_1",
      formatVersion: 1,
      locked: true,
    }).locked).toBe(true);
    expect(() => decodeSecretStoreStatus({
      initialized: false,
      storeId: null,
      formatVersion: null,
      locked: false,
    })).toThrow(DecodeError);
  });
});

const approvalWait = () => ({
  ...baseWait(),
  kind: "approval",
  request: {
    schemaVersion: 1,
    kind: "tool_approval",
    modelCallId: "call_1",
    calls: [{
      toolCallId: "tool_call_1",
      callDigest: "sha256:call",
      riskSummary: "Send the selected message to the external tool",
      expiresAt: 100,
    }],
  },
  blockers: [{
    kind: "tool_call",
    id: "tool_call_1",
    order: 0,
    status: "open",
    decisionRef: null,
  }],
});

const baseWait = () => ({
  id: "wait_1",
  runId: "run_1",
  nodeInstanceId: "node_1",
  attemptId: "attempt_1",
  kind: "human_response",
  requestRef: "object_1",
  request: { schemaVersion: 1, kind: "unknown" },
  responseSchema: null,
  responseSchemaCompilation: null,
  correlationKey: null,
  deadlineAt: null,
  status: "open",
  blockers: [] as Array<Record<string, unknown>>,
  acceptedDeliveryId: null,
  createdAt: 1,
  resolvedAt: null,
});

const proposal = () => ({
  id: "proposal_1", scopeId: "roleplay", memoryId: "memory_1",
  expectedHeadCommitId: null, changeType: "create", contentRef: "object_2",
  proposedContent: { schemaVersion: 1, text: "Alice prefers tea", tags: ["preference"], attributes: {} },
  reason: "Observed preference", evidenceRefs: ["message_1"],
  requestedBy: { kind: "node", id: "node_1" }, schemaVersion: 1, policyVersion: 1,
  originRunId: "run_1", originNodeInstanceId: "node_1", appliedCommitId: null,
  status: "awaiting_review", createdAt: 1, updatedAt: 1,
});
