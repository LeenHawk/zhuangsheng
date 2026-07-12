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
      candidateError: null,
      liveCandidates: [],
      waits: [],
      handledWaits: [],
      secretStatus: null,
      waitPendingId: null,
      waitError: null,
      waitActionErrors: {},
      onBack: () => undefined,
      onReload: () => undefined,
      onReloadOptions: () => undefined,
      onSaveRunProfile,
      onSubmitMessage,
      onRegenerateCandidate: async () => undefined,
      onSelectCandidate: async () => undefined,
      onResolveCandidateProjection: async () => undefined,
      onSubmitApproval: async () => undefined,
      onSubmitMemoryProposals: async () => undefined,
      onSubmitSecretPassword: async () => undefined,
      onResolveEffect: async () => undefined,
      onSubmitHumanResponse: async () => undefined,
      onReloadWaits: () => undefined,
      onInspectRun: () => undefined,
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

  it("confirms historical branch selection and keeps regenerate on the latest turn", async () => {
    const onRegenerateCandidate = vi.fn(async () => undefined);
    const onSelectCandidate = vi.fn(async () => undefined);
    const onResolveCandidateProjection = vi.fn(async () => undefined);
    render(<StoryDetail
      story={{
        ...story,
        runProfile: {
          graphRevisionId: option.revisionId,
          replyOutputKey: "reply",
          inputShape: "conversation_message_v1",
          revisionNo: 1,
        },
      }}
      timeline={{
        conversationId: story.id,
        activeBranchId: "branch_selected",
        activeHeadCommitId: "commit_selected",
        messages: [],
        turns: [{
          id: "turn_1",
          conversationId: story.id,
          userMessageId: "message_1",
          userCommitId: "commit_user",
          createdAt: 1,
          selectedRunId: "run_1",
          candidates: [
            {
              turnId: "turn_1", runId: "run_1", branchId: "branch_1",
              baseCommitId: "commit_user", replyOutputKey: "reply", status: "ready",
              assistantMessageId: "message_a", candidateCommitId: "commit_selected",
              projectionError: null, createdAt: 2,
            },
            {
              turnId: "turn_1", runId: "run_2", branchId: "branch_2",
              baseCommitId: "commit_user", replyOutputKey: "reply", status: "ready",
              assistantMessageId: "message_b", candidateCommitId: "commit_other",
              projectionError: null, createdAt: 3,
            },
          ],
        }, {
          id: "turn_2",
          conversationId: story.id,
          userMessageId: "message_2",
          userCommitId: "commit_user_2",
          createdAt: 4,
          selectedRunId: "run_3",
          candidates: [{
            turnId: "turn_2", runId: "run_3", branchId: "branch_3",
            baseCommitId: "commit_user_2", replyOutputKey: "reply", status: "ready",
            assistantMessageId: "message_c", candidateCommitId: "commit_selected",
            projectionError: null, createdAt: 5,
          }, {
            turnId: "turn_2", runId: "run_4", branchId: "branch_4",
            baseCommitId: "commit_user_2", replyOutputKey: "reply", status: "projection_conflicted",
            assistantMessageId: null, candidateCommitId: null,
            projectionError: { code: "candidate_head_mismatch", safeMessage: "故事分支已经前移" },
            createdAt: 6,
          }],
        }],
      }}
      graphOptions={[option]}
      loading={false}
      optionsLoading={false}
      pendingAction={null}
      error={null}
      optionsError={null}
      profileError={null}
      turnError={null}
      candidateError={null}
      liveCandidates={[{
        runId: "run_live",
        connection: "reconnecting",
        text: "月光落在档案封面上。",
        truncated: false,
        error: null,
        refreshVersion: 0,
      }]}
      waits={[]}
      handledWaits={[]}
      secretStatus={null}
      waitPendingId={null}
      waitError={null}
      waitActionErrors={{}}
      onBack={() => undefined}
      onReload={() => undefined}
      onReloadOptions={() => undefined}
      onSaveRunProfile={async () => undefined}
      onSubmitMessage={async () => undefined}
      onRegenerateCandidate={onRegenerateCandidate}
      onSelectCandidate={onSelectCandidate}
      onResolveCandidateProjection={onResolveCandidateProjection}
      onSubmitApproval={async () => undefined}
      onSubmitMemoryProposals={async () => undefined}
      onSubmitSecretPassword={async () => undefined}
      onResolveEffect={async () => undefined}
      onSubmitHumanResponse={async () => undefined}
      onReloadWaits={() => undefined}
      onInspectRun={() => undefined}
    />);

    fireEvent.click(screen.getByRole("button", { name: "从此处继续 run_2" }));
    expect(onSelectCandidate).not.toHaveBeenCalled();
    expect(screen.getByText(/后续 1 轮历史仍会保留/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "确认从此处继续" }));
    await waitFor(() => expect(onSelectCandidate).toHaveBeenCalledWith("turn_1", "run_2"));
    fireEvent.click(screen.getByRole("button", { name: "再生成一个" }));
    await waitFor(() => expect(onRegenerateCandidate).toHaveBeenCalledWith("turn_2", "commit_user_2"));
    fireEvent.click(screen.getByRole("button", { name: "附加回复到当前分支" }));
    await waitFor(() => expect(onResolveCandidateProjection).toHaveBeenCalledWith(
      "turn_2",
      "run_4",
      "branch_4",
      { type: "append_after_current", reason: "user reviewed and kept the advanced branch" },
    ));
    expect(screen.getByText("未提交实时预览")).toBeInTheDocument();
    expect(screen.getByText("月光落在档案封面上。")).toBeInTheDocument();
    expect(screen.getByText("正在恢复连接")).toBeInTheDocument();
  });
});
