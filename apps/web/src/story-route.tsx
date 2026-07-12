import { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import type {
  ConversationRunProfile,
  ConversationRunSpec,
  ConversationTimelineView,
  ConversationView,
  RolePlayGraphOptionView,
} from "@zhuangsheng/api-client";
import { StoryDetail } from "@zhuangsheng/domain-ui";

import { client, messageFor } from "./api";

export function StoryRoute() {
  const { conversationId = "" } = useParams();
  const navigate = useNavigate();
  const polling = useRef<AbortController | null>(null);
  const [story, setStory] = useState<ConversationView | null>(null);
  const [timeline, setTimeline] = useState<ConversationTimelineView | null>(null);
  const [graphOptions, setGraphOptions] = useState<RolePlayGraphOptionView[]>([]);
  const [loading, setLoading] = useState(true);
  const [optionsLoading, setOptionsLoading] = useState(true);
  const [pendingAction, setPendingAction] = useState<"profile" | "turn" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [optionsError, setOptionsError] = useState<string | null>(null);
  const [profileError, setProfileError] = useState<string | null>(null);
  const [turnError, setTurnError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [nextStory, nextTimeline] = await Promise.all([
        client.getConversation(conversationId),
        client.getTimeline(conversationId),
      ]);
      setStory(nextStory);
      setTimeline(nextTimeline);
    } catch (cause) {
      setError(messageFor(cause));
    } finally {
      setLoading(false);
    }
  }, [conversationId]);
  const reloadOptions = useCallback(async () => {
    setOptionsLoading(true);
    setOptionsError(null);
    try {
      setGraphOptions(await client.listRolePlayGraphOptions());
    } catch (cause) {
      setOptionsError(messageFor(cause));
    } finally {
      setOptionsLoading(false);
    }
  }, []);
  useEffect(() => {
    void reload();
    void reloadOptions();
    return () => polling.current?.abort();
  }, [reload, reloadOptions]);

  const saveRunProfile = async (run: ConversationRunSpec) => {
    setPendingAction("profile");
    setProfileError(null);
    try {
      const profile = await client.updateConversationRunProfile(conversationId, {
        expectedRevisionNo: story?.runProfile?.revisionNo ?? 0,
        run,
      });
      setStory((current) => current ? { ...current, runProfile: profile } : current);
    } catch (cause) {
      setProfileError(messageFor(cause));
      throw cause;
    } finally {
      setPendingAction(null);
    }
  };

  const submitMessage = async (text: string) => {
    if (!story?.runProfile || !timeline) throw new Error("故事运行设置尚未就绪。");
    polling.current?.abort();
    const controller = new AbortController();
    polling.current = controller;
    setPendingAction("turn");
    setTurnError(null);
    try {
      const ack = await client.submitConversationTurn(conversationId, {
        expectedHeadCommitId: timeline.activeHeadCommitId,
        userContent: [{ type: "text", text }],
        run: runSpec(story.runProfile),
      }, controller.signal);
      try {
        await pollTimeline(conversationId, ack.runId, controller.signal, setTimeline);
      } catch (cause) {
        if (!isAbort(cause)) setTurnError(`消息已保存，但状态刷新失败：${messageFor(cause)}`);
      }
    } catch (cause) {
      if (isAbort(cause)) return;
      setTurnError(messageFor(cause));
      throw cause;
    } finally {
      if (polling.current === controller) {
        polling.current = null;
        setPendingAction(null);
      }
    }
  };

  return (
    <StoryDetail
      story={story}
      timeline={timeline}
      graphOptions={graphOptions}
      loading={loading}
      optionsLoading={optionsLoading}
      pendingAction={pendingAction}
      error={error}
      optionsError={optionsError}
      profileError={profileError}
      turnError={turnError}
      onBack={() => navigate("/stories")}
      onReload={() => void reload()}
      onReloadOptions={() => void reloadOptions()}
      onSaveRunProfile={saveRunProfile}
      onSubmitMessage={submitMessage}
    />
  );
}

const runSpec = ({ graphRevisionId, replyOutputKey }: ConversationRunProfile): ConversationRunSpec => ({
  graphRevisionId,
  replyOutputKey,
  inputShape: "conversation_message_v1",
});

async function pollTimeline(
  conversationId: string,
  runId: string,
  signal: AbortSignal,
  update: (timeline: ConversationTimelineView) => void,
) {
  for (let attempt = 0; attempt < 21; attempt += 1) {
    if (attempt > 0) await delay(500, signal);
    const next = await client.getTimeline(conversationId, signal);
    if (signal.aborted) return;
    update(next);
    const candidate = next.turns.flatMap((turn) => turn.candidates).find((item) => item.runId === runId);
    if (!candidate || candidate.status !== "running") return;
  }
}

const delay = (milliseconds: number, signal: AbortSignal) =>
  new Promise<void>((resolve, reject) => {
    if (signal.aborted) {
      reject(new DOMException("Aborted", "AbortError"));
      return;
    }
    const abort = () => {
      window.clearTimeout(timeout);
      reject(new DOMException("Aborted", "AbortError"));
    };
    const timeout = window.setTimeout(() => {
      signal.removeEventListener("abort", abort);
      resolve();
    }, milliseconds);
    signal.addEventListener("abort", abort, { once: true });
  });

const isAbort = (cause: unknown) => cause instanceof DOMException && cause.name === "AbortError";
