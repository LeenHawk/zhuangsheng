// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
      onSubmitSecretPassword={async () => undefined}
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
      onSubmitSecretPassword={onSubmitSecretPassword}
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

const secretWait = (): WaitView => ({
  ...baseWait(),
  kind: "secret_store_unlocked",
  request: {
    kind: "secret_store_unlocked",
    reason: "provider_credential_required",
    channelId: "channel_1",
  },
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
