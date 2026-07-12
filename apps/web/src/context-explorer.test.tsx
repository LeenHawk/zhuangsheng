// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ContextBranchView, ContextCommitView, MergeContextView } from "@zhuangsheng/api-client";
import { ContextExplorer } from "@zhuangsheng/domain-ui";

const branches: ContextBranchView[] = [
  { contextId: "context_1", branchId: "branch_1", forkCommitId: "commit_1", headCommitId: "commit_2", status: "active" },
  { contextId: "context_1", branchId: "branch_2", forkCommitId: "commit_1", headCommitId: "commit_3", status: "active" },
];
const commits: ContextCommitView[] = [
  commit("commit_1", "branch_1", 1, []),
  commit("commit_2", "branch_1", 2, ["commit_1"]),
  commit("commit_3", "branch_2", 2, ["commit_1"]),
];

describe("expert Context explorer", () => {
  it("uses exact historical fork and finite merge CAS inputs", async () => {
    const onFork = vi.fn(async () => undefined);
    const onMerge = vi.fn(async () => undefined);
    const common = {
      contextId: "context_1", branches, commits, selectedBranchId: "branch_1",
      selectedCommitId: "commit_2", projection: { contextId: "context_1", branchId: "branch_1", headCommitId: "commit_2", value: { lore: "current" } },
      historical: { contextId: "context_1", branchId: "branch_1", headCommitId: "commit_2", value: { lore: "historical" } },
      diff: null, snapshot: null, loading: false, pending: null, error: null,
      onBack: () => undefined, onReload: () => undefined, onSelectBranch: () => undefined,
      onSelectCommit: () => undefined, onDiff: () => undefined, onFork, onMerge,
      onSnapshot: async () => undefined,
    };
    const view = render(<ContextExplorer {...common} mergeResult={null} />);
    fireEvent.click(screen.getByRole("button", { name: "从此处 Fork" }));
    await waitFor(() => expect(onFork).toHaveBeenCalledWith(branches[0], "commit_2"));
    fireEvent.click(screen.getByRole("button", { name: "检查并合并" }));
    await waitFor(() => expect(onMerge).toHaveBeenCalledWith({
      sourceBranchId: "branch_1", targetBranchId: "branch_2",
      expectedSourceHead: "commit_2", expectedTargetHead: "commit_3",
      sourceDisposition: "keep_active", selections: [],
    }));

    view.rerender(<ContextExplorer {...common} mergeResult={conflict()} />);
    fireEvent.click(screen.getAllByRole("radio")[0]!);
    fireEvent.click(screen.getByRole("button", { name: "提交冲突选择" }));
    await waitFor(() => expect(onMerge).toHaveBeenLastCalledWith(expect.objectContaining({
      selections: [{
        conflictId: "conflict_1", path: "/lore",
        resolution: { type: "final_value", value: "base" },
      }],
    })));
  });
});

function commit(id: string, branchId: string, sequenceNo: number, parentCommitIds: string[]): ContextCommitView {
  return { id, contextId: "context_1", branchId, sequenceNo, operationId: `operation_${id}`, parentCommitIds, patchRef: null, schemaVersion: 1, policyVersion: 1, author: { kind: "user", id: "local-user" }, originRunId: null, originNodeInstanceId: null, createdAt: sequenceNo };
}

function conflict(): MergeContextView {
  return {
    contextId: "context_1", sourceBranchId: "branch_1", targetBranchId: "branch_2",
    baseCommitId: "commit_1", sourceHeadCommitId: "commit_2", targetHeadCommitId: "commit_3",
    status: "conflicted", mergeCommitId: null,
    conflicts: [{ conflictId: "conflict_1", path: "/lore", base: "base", source: "source", target: "target" }],
  };
}
