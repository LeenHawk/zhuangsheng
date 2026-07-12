// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { MemoryProposalView, MemoryRecordView } from "@zhuangsheng/api-client";
import { MemoryPage } from "@zhuangsheng/domain-ui";

describe("MemoryPage", () => {
  it("creates a replace proposal and preserves unknown canonical attributes", async () => {
    const onPropose = vi.fn(async () => undefined);
    renderPage({ records: [record], onPropose });
    fireEvent.click(screen.getByRole("button", { name: "更正" }));
    fireEvent.change(screen.getByLabelText("记忆内容"), { target: { value: "Alice prefers green tea" } });
    fireEvent.change(screen.getByLabelText("证据引用（每行或逗号分隔）"), { target: { value: "message_2" } });
    fireEvent.click(screen.getByRole("button", { name: "创建 proposal" }));
    await waitFor(() => expect(onPropose).toHaveBeenCalledWith({
      memoryId: "memory_1",
      expectedHeadCommitId: "commit_1",
      change: { type: "replace_content", content: { schemaVersion: 1, text: "Alice prefers green tea", tags: ["preference"], attributes: { source: "story" } } },
      reason: "更正不准确或过时的记忆",
      evidenceRefs: ["message_2"],
    }));
  });

  it("keeps review, reject and Apply as distinct commands", () => {
    const onDecide = vi.fn(); const onApply = vi.fn(); const onLoadMore = vi.fn();
    renderPage({ proposals: [proposal, { ...proposal, id: "proposal_2", status: "approved" }], hasMore: true, onDecide, onApply, onLoadMore });
    fireEvent.click(screen.getAllByRole("button", { name: "批准" })[0]!);
    expect(onDecide).toHaveBeenCalledWith(proposal, "approve");
    fireEvent.click(screen.getByRole("button", { name: "拒绝" }));
    expect(onDecide).toHaveBeenCalledWith(proposal, "reject");
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(onApply).toHaveBeenCalledWith(expect.objectContaining({ id: "proposal_2", status: "approved" }));
    fireEvent.click(screen.getByRole("button", { name: "加载更多 proposal" }));
    expect(onLoadMore).toHaveBeenCalledOnce();
  });
});

const record: MemoryRecordView = { id: "memory_1", scopeId: "roleplay", status: "active", headCommitId: "commit_1", contentRef: "object_1", content: { schemaVersion: 1, text: "Alice likes tea", tags: ["preference"], attributes: { source: "story" } }, createdAt: 1, updatedAt: 2 };
const proposal: MemoryProposalView = { id: "proposal_1", scopeId: "roleplay", memoryId: "memory_1", expectedHeadCommitId: "commit_1", changeType: "replace_content", contentRef: "object_2", proposedContent: { schemaVersion: 1, text: "Alice prefers green tea", tags: ["preference"], attributes: {} }, reason: "New evidence", evidenceRefs: ["message_2"], requestedBy: { kind: "user", id: "user_1" }, schemaVersion: 1, policyVersion: 1, originRunId: null, originNodeInstanceId: null, appliedCommitId: null, status: "awaiting_review", createdAt: 1, updatedAt: 2 };

function renderPage(overrides: Partial<React.ComponentProps<typeof MemoryPage>>) {
  const props: React.ComponentProps<typeof MemoryPage> = { scopeId: "roleplay", records: [], proposals: [], hasMore: false, loading: false, pending: false, error: null, onScopeChange: () => undefined, onReload: () => undefined, onLoadMore: () => undefined, onPropose: async () => undefined, onDecide: () => undefined, onApply: () => undefined, ...overrides };
  return render(<MemoryPage {...props} />);
}
