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
      onReload={() => undefined}
    />);

    expect(screen.getByRole("button", { name: "确认未执行，安全重试" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("处理依据"), {
      target: { value: "provider 查询确认没有创建请求" },
    });
    fireEvent.click(screen.getByLabelText(/我理解此决定/));
    fireEvent.click(screen.getByRole("button", { name: "确认未执行，安全重试" }));
    await waitFor(() => expect(onResolveEffect).toHaveBeenCalledWith(
      wait,
      "confirm_failed_retry_safe",
      "provider 查询确认没有创建请求",
    ));
  });
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
  correlationKey: null,
  deadlineAt: null,
  status: "open",
  blockers: [],
  acceptedDeliveryId: null,
  createdAt: 1,
  resolvedAt: null,
});
