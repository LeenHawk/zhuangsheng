import { useState, type Dispatch, type SetStateAction } from "react";

import type {
  ConversationRunProfile,
  ConversationRunSpec,
  ConversationTimelineView,
  ConversationView,
  SubmitConversationTurnAck,
} from "@zhuangsheng/api-client";

import { client, messageFor } from "./api";

type PendingAction = "profile" | "turn" | "regenerate" | "selection" | null;

interface StoryActionsInput {
  conversationId: string;
  story: ConversationView | null;
  timeline: ConversationTimelineView | null;
  setStory: Dispatch<SetStateAction<ConversationView | null>>;
  setTimeline: Dispatch<SetStateAction<ConversationTimelineView | null>>;
}

export function useStoryActions(input: StoryActionsInput) {
  const [pendingAction, setPendingAction] = useState<PendingAction>(null);
  const [profileError, setProfileError] = useState<string | null>(null);
  const [turnError, setTurnError] = useState<string | null>(null);
  const [candidateError, setCandidateError] = useState<string | null>(null);
  const saveRunProfile = async (run: ConversationRunSpec) => {
    setPendingAction("profile");
    setProfileError(null);
    try {
      const profile = await client.updateConversationRunProfile(input.conversationId, {
        expectedRevisionNo: input.story?.runProfile?.revisionNo ?? 0,
        run,
      });
      input.setStory((current) => current ? { ...current, runProfile: profile } : current);
    } catch (cause) {
      setProfileError(messageFor(cause));
      throw cause;
    } finally {
      setPendingAction(null);
    }
  };

  const runCandidate = async (
    kind: "turn" | "regenerate",
    command: () => Promise<SubmitConversationTurnAck>,
  ) => {
    const setError = kind === "turn" ? setTurnError : setCandidateError;
    setPendingAction(kind);
    setError(null);
    try {
      await command();
      try {
        input.setTimeline(await client.getTimeline(input.conversationId));
      } catch (cause) {
        setError(`命令已保存，但状态刷新失败：${messageFor(cause)}`);
      }
    } catch (cause) {
      setError(messageFor(cause));
      throw cause;
    } finally {
      setPendingAction(null);
    }
  };

  const submitMessage = async (text: string) => {
    if (!input.story?.runProfile || !input.timeline) throw new Error("故事运行设置尚未就绪。");
    const profile = input.story.runProfile;
    const head = input.timeline.activeHeadCommitId;
    await runCandidate("turn", () => client.submitConversationTurn(input.conversationId, {
      expectedHeadCommitId: head,
      userContent: [{ type: "text", text }],
      run: runSpec(profile),
    }));
  };

  const regenerateCandidate = async (turnId: string, userCommitId: string) => {
    if (!input.story?.runProfile) throw new Error("故事运行设置尚未就绪。");
    const profile = input.story.runProfile;
    await runCandidate("regenerate", () => client.regenerateConversationCandidate(turnId, {
      expectedUserCommitId: userCommitId,
      run: runSpec(profile),
    }));
  };

  const selectCandidate = async (turnId: string, runId: string) => {
    if (!input.timeline) throw new Error("故事时间线尚未就绪。");
    setPendingAction("selection");
    setCandidateError(null);
    try {
      const selection = await client.selectConversationCandidate(turnId, {
        selectedRunId: runId,
        expectedConversationHeadCommitId: input.timeline.activeHeadCommitId,
      });
      input.setStory((current) => current ? {
        ...current,
        activeBranchId: selection.selectedBranchId,
        activeHeadCommitId: selection.selectedCommitId,
      } : current);
      try {
        input.setTimeline(await client.getTimeline(input.conversationId));
      } catch (cause) {
        setCandidateError(`候选已切换，但时间线刷新失败：${messageFor(cause)}`);
      }
    } catch (cause) {
      setCandidateError(messageFor(cause));
      try {
        const [story, timeline] = await Promise.all([
          client.getConversation(input.conversationId),
          client.getTimeline(input.conversationId),
        ]);
        input.setStory(story);
        input.setTimeline(timeline);
      } catch {
        // Keep the original selection error; the page-level refresh remains available.
      }
      throw cause;
    } finally {
      setPendingAction(null);
    }
  };

  return {
    pendingAction,
    profileError,
    turnError,
    candidateError,
    saveRunProfile,
    submitMessage,
    regenerateCandidate,
    selectCandidate,
  };
}

const runSpec = ({ graphRevisionId, replyOutputKey }: ConversationRunProfile): ConversationRunSpec => ({
  graphRevisionId,
  replyOutputKey,
  inputShape: "conversation_message_v1",
});
