// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ConversationView, RolePlayGraphOptionView } from "@zhuangsheng/api-client";
import { StoryDetail, type StoryDetailProps } from "@zhuangsheng/domain-ui";

const story: ConversationView = {
  id: "conversation_1",
  title: "月下档案馆",
  contextId: "context_1",
  activeBranchId: "branch_1",
  activeHeadCommitId: "commit_1",
  runProfile: null,
  createdAt: 1,
  updatedAt: 1,
};

const option: RolePlayGraphOptionView = {
  graphId: "graph_1",
  graphName: "守夜人",
  revisionId: "graphrev_1",
  revisionNo: 3,
  replyOutputKeys: ["reply"],
  primaryLlmNodeId: "generate",
  compatibility: {
    mode: "partial",
    profileVersion: 1,
    editableFields: ["model"],
    lockedReasons: ["custom_coordination_nodes"],
  },
};

describe("StoryDetail", () => {
  it("saves an exact run profile before enabling the durable message composer", async () => {
    const onSaveRunProfile = vi.fn(async () => undefined);
    const onSubmitMessage = vi.fn(async () => undefined);
    const props: Omit<StoryDetailProps, "story"> = {
      timeline: {
        conversationId: story.id,
        activeBranchId: story.activeBranchId,
        activeHeadCommitId: story.activeHeadCommitId,
        messages: [],
        turns: [],
      },
      graphOptions: [option],
      loading: false,
      optionsLoading: false,
      pendingAction: null,
      error: null,
      optionsError: null,
      profileError: null,
      turnError: null,
      onBack: () => undefined,
      onReload: () => undefined,
      onReloadOptions: () => undefined,
      onSaveRunProfile,
      onSubmitMessage,
    };
    const view = render(<StoryDetail {...props} story={story} />);

    expect(screen.getByRole("textbox", { name: "继续故事" })).toBeDisabled();
    expect(screen.getByText("含高级设置")).toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "用于后续消息" }));
    await waitFor(() => expect(onSaveRunProfile).toHaveBeenCalledWith({
      graphRevisionId: "graphrev_1",
      replyOutputKey: "reply",
      inputShape: "conversation_message_v1",
    }));

    view.rerender(<StoryDetail {...props} story={{
      ...story,
      runProfile: {
        graphRevisionId: "graphrev_1",
        replyOutputKey: "reply",
        inputShape: "conversation_message_v1",
        revisionNo: 1,
      },
    }} />);
    const composer = screen.getByRole("textbox", { name: "继续故事" });
    fireEvent.change(composer, { target: { value: "打开最后一卷档案。" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(onSubmitMessage).toHaveBeenCalledWith("打开最后一卷档案。"));
  });
});
