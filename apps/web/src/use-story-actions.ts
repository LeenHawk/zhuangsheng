import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";

import type {
  ConversationRunProfile,
  ConversationRunSpec,
  ConversationTimelineView,
  ConversationView,
  SubmitConversationTurnAck,
} from "@zhuangsheng/api-client";

import { client, messageFor } from "./api";
import { isAbort, pollTimeline } from "./timeline-poll";

type PendingAction = "profile" | "turn" | "regenerate" | "selection" | null;

interface StoryActionsInput {
  conversationId: string;
  story: ConversationView | null;
  timeline: ConversationTimelineView | null;
  setStory: Dispatch<SetStateAction<ConversationView | null>>;
  setTimeline: Dispatch<SetStateAction<ConversationTimelineView | null>>;
}

export function useStoryActions(input: StoryActionsInput) {
  const polling = useRef<AbortController | null>(null);
  const [pendingAction, setPendingAction] = useState<PendingAction>(null);
  const [profileError, setProfileError] = useState<string | null>(null);
  const [turnError, setTurnError] = useState<string | null>(null);
  const [candidateError, setCandidateError] = useState<string | null>(null);
  useEffect(() => {
    polling.current?.abort();
    return () => polling.current?.abort();
  }, [input.conversationId]);

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
    command: (signal: AbortSignal) => Promise<SubmitConversationTurnAck>,
  ) => {
    polling.current?.abort();
    const controller = new AbortController();
    polling.current = controller;
    const setError = kind === "turn" ? setTurnError : setCandidateError;
    setPendingAction(kind);
    setError(null);
    try {
      const ack = await command(controller.signal);
      try {
        await pollTimeline(input.conversationId, ack.runId, controller.signal, input.setTimeline);
      } catch (cause) {
        if (!isAbort(cause)) setError(`命令已保存，但状态刷新失败：${messageFor(cause)}`);
      }
    } catch (cause) {
      if (isAbort(cause)) return;
      setError(messageFor(cause));
      throw cause;
    } finally {
      if (polling.current === controller) {
        polling.current = null;
        setPendingAction(null);
      }
    }
  };

  const submitMessage = async (text: string) => {
    if (!input.story?.runProfile || !input.timeline) throw new Error("故事运行设置尚未就绪。");
    const profile = input.story.runProfile;
    const head = input.timeline.activeHeadCommitId;
    await runCandidate("turn", (signal) => client.submitConversationTurn(input.conversationId, {
      expectedHeadCommitId: head,
      userContent: [{ type: "text", text }],
      run: runSpec(profile),
    }, signal));
  };

  const regenerateCandidate = async (turnId: string, userCommitId: string) => {
    if (!input.story?.runProfile) throw new Error("故事运行设置尚未就绪。");
    const profile = input.story.runProfile;
    await runCandidate("regenerate", (signal) => client.regenerateConversationCandidate(turnId, {
      expectedUserCommitId: userCommitId,
      run: runSpec(profile),
    }, signal));
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
