import { useCallback, useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import type {
  ConversationTimelineView,
  ConversationView,
  RolePlayGraphOptionView,
} from "@zhuangsheng/api-client";
import { StoryDetail } from "@zhuangsheng/domain-ui";

import { client, messageFor } from "./api";
import { useStoryActions } from "./use-story-actions";
import { useStoryStreams } from "./use-story-streams";
import { useStoryWaits } from "./use-story-waits";

export function StoryRoute() {
  const { conversationId = "" } = useParams();
  const navigate = useNavigate();
  const [story, setStory] = useState<ConversationView | null>(null);
  const [timeline, setTimeline] = useState<ConversationTimelineView | null>(null);
  const [graphOptions, setGraphOptions] = useState<RolePlayGraphOptionView[]>([]);
  const [loading, setLoading] = useState(true);
  const [optionsLoading, setOptionsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [optionsError, setOptionsError] = useState<string | null>(null);
  const actions = useStoryActions({ conversationId, story, timeline, setStory, setTimeline });
  const liveCandidates = useStoryStreams(conversationId, timeline, setTimeline);
  const waits = useStoryWaits(liveCandidates);

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
  }, [reload, reloadOptions]);

  return (
    <StoryDetail
      story={story}
      timeline={timeline}
      graphOptions={graphOptions}
      loading={loading}
      optionsLoading={optionsLoading}
      pendingAction={actions.pendingAction}
      error={error}
      optionsError={optionsError}
      profileError={actions.profileError}
      turnError={actions.turnError}
      candidateError={actions.candidateError}
      liveCandidates={liveCandidates}
      waits={waits.waits}
      handledWaits={waits.handledWaits}
      secretStatus={waits.secretStatus}
      waitPendingId={waits.pendingWaitId}
      waitError={waits.waitError}
      waitActionErrors={waits.actionErrors}
      onBack={() => navigate("/stories")}
      onReload={() => void reload()}
      onReloadOptions={() => void reloadOptions()}
      onSaveRunProfile={actions.saveRunProfile}
      onSubmitMessage={actions.submitMessage}
      onRegenerateCandidate={actions.regenerateCandidate}
      onSelectCandidate={actions.selectCandidate}
      onSubmitApproval={waits.submitApproval}
      onSubmitSecretPassword={waits.submitSecretPassword}
      onReloadWaits={waits.reloadWaits}
    />
  );
}
