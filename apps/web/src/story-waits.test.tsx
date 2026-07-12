// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { WaitView } from "@zhuangsheng/api-client";
import { StoryWaitActions } from "@zhuangsheng/domain-ui";

describe("StoryWaitActions", () => {
  it("requires an explicit decision for every tool approval blocker", async () => {
    const onSubmitApproval = vi.fn(async () => undefined);
    const wait = approvalWait();
    render(<StoryWaitActions
      waits={[wait]}
      handled={[]}
      secretStatus={null}
      pendingWaitId={null}
      loadError={null}
      actionErrors={{}}
      onSubmitApproval={onSubmitApproval}
      onSubmitMemoryProposals={async () => undefined}
      onSubmitSecretPassword={async () => undefined}
      onResolveEffect={async () => undefined}
      onSubmitHumanResponse={async () => undefined}
      onReload={() => undefined}
    />);

    expect(screen.getByRole("button", { name: "提交全部决定" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "允许一次" }));
    fireEvent.click(screen.getByRole("button", { name: "提交全部决定" }));
    await waitFor(() => expect(onSubmitApproval).toHaveBeenCalledWith(wait, [{
      toolCallId: "tool_call_1",
      callDigest: "sha256:call",
      decision: "approve",
      reason: undefined,
    }]));
  });

  it("uses a dedicated write-only password form for a secret unlock wait", async () => {
    const onSubmitSecretPassword = vi.fn(async () => undefined);
    const wait = secretWait();
    render(<StoryWaitActions
      waits={[wait]}
      handled={[]}
      secretStatus={{ initialized: true, storeId: "store_1", formatVersion: 1, locked: true }}
      pendingWaitId={null}
      loadError={null}
      actionErrors={{}}
      onSubmitApproval={async () => undefined}
      onSubmitMemoryProposals={async () => undefined}
      onSubmitSecretPassword={onSubmitSecretPassword}
      onResolveEffect={async () => undefined}
      onSubmitHumanResponse={async () => undefined}
      onReload={() => undefined}
    />);

    fireEvent.change(screen.getByLabelText("主密码"), {
      target: { value: "correct horse battery staple" },
    });
    fireEvent.click(screen.getByRole("button", { name: "解锁并继续" }));
    await waitFor(() => expect(onSubmitSecretPassword).toHaveBeenCalledWith(
      wait,
      "unlock",
      "correct horse battery staple",
    ));
  });

  it("shows inspectable memory proposal content before requiring every decision", async () => {
    const onSubmitMemoryProposals = vi.fn(async () => undefined);
    const wait = memoryProposalWait();
    const view = render(<StoryWaitActions
      waits={[wait]}
      handled={[]}
      secretStatus={null}
      pendingWaitId={null}
      loadError={null}
      actionErrors={{}}
      onSubmitApproval={async () => undefined}
      onSubmitMemoryProposals={onSubmitMemoryProposals}
      onSubmitSecretPassword={async () => undefined}
      onResolveEffect={async () => undefined}
      onSubmitHumanResponse={async () => undefined}
      onReload={() => undefined}
    />);
    const card = within(view.container);
    expect(card.getByText("Alice prefers green tea")).toBeInTheDocument();
    const submit = card.getByRole("button", { name: "提交全部决定" });
    expect(submit).toBeDisabled();
    fireEvent.click(card.getByRole("button", { name: "批准提案" }));
    fireEvent.click(submit);
    await waitFor(() => expect(onSubmitMemoryProposals).toHaveBeenCalledWith(wait, [{
      proposalId: "proposal_1", decision: "approve",
    }]));
  });

  it("requires an audited decision before resolving an unknown effect", async () => {
    const onResolveEffect = vi.fn(async () => undefined);
    const wait = effectWait();
    render(<StoryWaitActions
      waits={[wait]}
      handled={[]}
      secretStatus={null}
      pendingWaitId={null}
      loadError={null}
      actionErrors={{}}
      onSubmitApproval={async () => undefined}
      onSubmitMemoryProposals={async () => undefined}
      onSubmitSecretPassword={async () => undefined}
      onResolveEffect={onResolveEffect}
      onSubmitHumanResponse={async () => undefined}
      onReload={() => undefined}
    />);

    expect(screen.getByRole("button", { name: "确认未执行，安全重试" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "确认已成功" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("处理依据"), {
      target: { value: "provider 查询确认没有创建请求" },
    });
    fireEvent.click(screen.getByLabelText(/我理解此决定/));
    fireEvent.click(screen.getByRole("button", { name: "确认未执行，安全重试" }));
    await waitFor(() => expect(onResolveEffect).toHaveBeenCalledWith(
      wait,
      {
        kind: "confirm_failed_retry_safe",
        reason: "provider 查询确认没有创建请求",
        resultObjectId: null,
        evidenceObjectId: null,
      },
    ));
    fireEvent.change(screen.getByLabelText("结果 Object ID"), {
      target: { value: "object_result_1" },
    });
    fireEvent.change(screen.getByLabelText(/Evidence Object ID/), {
      target: { value: "object_evidence_1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "确认已成功" }));
    await waitFor(() => expect(onResolveEffect).toHaveBeenCalledWith(
      wait,
      {
        kind: "confirm_succeeded",
        reason: "provider 查询确认没有创建请求",
        resultObjectId: "object_result_1",
        evidenceObjectId: "object_evidence_1",
      },
    ));
  });

  it("submits a schema-bound human response instead of using the story composer", async () => {
    const onSubmitHumanResponse = vi.fn(async () => undefined);
    const wait = humanWait();
    render(<StoryWaitActions
      waits={[wait]} handled={[]} secretStatus={null} pendingWaitId={null}
      loadError={null} actionErrors={{}}
      onSubmitApproval={async () => undefined}
      onSubmitMemoryProposals={async () => undefined}
      onSubmitSecretPassword={async () => undefined}
      onResolveEffect={async () => undefined}
      onSubmitHumanResponse={onSubmitHumanResponse}
      onReload={() => undefined}
    />);
    const submit = screen.getByRole("button", { name: "提交回应" });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "left" } });
    fireEvent.click(submit);
    await waitFor(() => expect(onSubmitHumanResponse).toHaveBeenCalledWith(wait, { choice: "left" }));
  });
});

const humanWait = (): WaitView => ({
  ...baseWait(),
  request: { kind: "human_response", title: "选择道路", description: "此选择会恢复原运行", payload: { schemaVersion: 1, kind: "human_response" } },
  responseSchema: {
    schemaVersion: 1,
    dialect: "https://json-schema.org/draft/2020-12/schema",
    validationProfileVersion: 1,
    formatPolicyVersion: 1,
    document: {
      type: "object",
      properties: { choice: { type: "string", title: "道路", enum: ["left", "right"] } },
      required: ["choice"],
      additionalProperties: false,
    },
    limits: {},
  },
  responseSchemaCompilation: {
    canonicalDocumentHash: "sha256:document", schemaHash: "sha256:schema",
    canonicalSource: "{}", compiledPayload: "{}", compiledPayloadHash: "sha256:compiled",
    compilerId: "zhuangsheng-json-schema", compilerVersion: "0.1.0", payloadFormatVersion: 1,
  },
});

const approvalWait = (): WaitView => ({
  ...baseWait(),
  kind: "approval",
  deadlineAt: Date.now() + 60_000,
  request: {
    kind: "tool_approval",
    modelCallId: "call_1",
    calls: [{
      toolCallId: "tool_call_1",
      callDigest: "sha256:call",
      riskSummary: "向外部工具发送当前选择的消息",
      expiresAt: Date.now() + 60_000,
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

const memoryProposalWait = (): WaitView => ({
  ...baseWait(),
  kind: "approval",
  request: {
    kind: "memory_proposal_review",
    modelCallId: "model_call_1",
    proposals: [{
      proposalId: "proposal_1",
      toolCallId: "tool_call_1",
      proposal: {
        id: "proposal_1", scopeId: "roleplay", memoryId: "memory_1",
        expectedHeadCommitId: null, changeType: "create", contentRef: "object_2",
        proposedContent: { schemaVersion: 1, text: "Alice prefers green tea", tags: ["preference"], attributes: {} },
        reason: "The conversation established a stable preference", evidenceRefs: ["message_1"],
        requestedBy: { kind: "node", id: "node_1" }, schemaVersion: 1, policyVersion: 1,
        originRunId: "run_1", originNodeInstanceId: "node_1", appliedCommitId: null,
        status: "awaiting_review", createdAt: 1, updatedAt: 1,
      },
    }],
  },
  blockers: [{ kind: "memory_proposal", id: "proposal_1", order: 0, status: "open", decisionRef: null }],
});

const secretWait = (): WaitView => ({
  ...baseWait(),
  kind: "secret_store_unlocked",
  request: {
    kind: "secret_store_unlocked",
    reason: "provider_credential_required",
    channelId: "channel_1",
  },
});

const effectWait = (): WaitView => ({
  ...baseWait(),
  kind: "effect_resolution",
  request: {
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
});

const baseWait = (): WaitView => ({
  id: "wait_1",
  runId: "run_1",
  nodeInstanceId: "node_1",
  attemptId: "attempt_1",
  kind: "human_response",
  requestRef: "object_1",
  request: { kind: "unsupported" },
  responseSchema: null,
  responseSchemaCompilation: null,
  correlationKey: null,
  deadlineAt: null,
  status: "open",
  blockers: [],
  acceptedDeliveryId: null,
  createdAt: 1,
  resolvedAt: null,
});
