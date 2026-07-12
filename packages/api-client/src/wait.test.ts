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
  correlationKey: null,
  deadlineAt: null,
  status: "open",
  blockers: [] as Array<Record<string, unknown>>,
  acceptedDeliveryId: null,
  createdAt: 1,
  resolvedAt: null,
});
