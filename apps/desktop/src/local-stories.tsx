import { useCallback, useEffect, useState } from "react";

import type {
  CandidateProjectionResolution,
  ConversationRunSpec,
  ConversationTimelineView,
  ConversationView,
  RolePlayGraphOptionView,
} from "@zhuangsheng/api-client";
import { StoryDetail, StoryList } from "@zhuangsheng/domain-ui";

import { bridge, conversations, localErrorMessage } from "./bridge";
import { useLocalWaits } from "./local-waits";

export function LocalStories({ initialStoryId, onStoryOpened, onInspectRun, onConfigure }: {
  initialStoryId: string | null;
  onStoryOpened: () => void;
  onInspectRun: (runId: string, storyId: string) => void;
  onConfigure: () => void;
}) {
  const [selected, setSelected] = useState<string | null>(initialStoryId);
  useEffect(() => { if (initialStoryId) onStoryOpened(); }, [initialStoryId, onStoryOpened]);
  return selected
    ? <LocalStory id={selected} onBack={() => setSelected(null)} onInspectRun={(runId) => onInspectRun(runId, selected)} />
    : <LocalStoryList onOpen={setSelected} onConfigure={onConfigure} />;
}

function LocalStoryList({ onOpen, onConfigure }: { onOpen: (id: string) => void; onConfigure: () => void }) {
  const [stories, setStories] = useState<ConversationView[]>([]);
  const [templates, setTemplates] = useState<RolePlayGraphOptionView[]>([]);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    setLoading(true); setError(null);
    try {
      const [list, options] = await Promise.all([
        conversations.listConversations(), conversations.listRolePlayGraphOptions(),
      ]);
      setStories(list.items); setTemplates(options);
    } catch (cause) { setError(localErrorMessage(cause)); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void reload(); }, [reload]);
  const create = async (title: string | undefined, defaultRun: ConversationRunSpec) => {
    setPending(true); setError(null);
    try { onOpen((await conversations.createConversation({ title, defaultRun })).id); }
    catch (cause) { setError(localErrorMessage(cause)); throw cause; }
    finally { setPending(false); }
  };
  return <StoryList stories={stories} templates={templates} loading={loading} pending={pending} error={error} onReload={() => void reload()} onCreate={create} onOpen={onOpen} onConfigure={onConfigure} />;
}

function LocalStory({ id, onBack, onInspectRun }: { id: string; onBack: () => void; onInspectRun: (id: string) => void }) {
  const [story, setStory] = useState<ConversationView | null>(null);
  const [timeline, setTimeline] = useState<ConversationTimelineView | null>(null);
  const [options, setOptions] = useState<RolePlayGraphOptionView[]>([]);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState<"profile" | "turn" | "regenerate" | "selection" | "projection" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const reload = useCallback(async (showLoading = false) => {
    if (showLoading) setLoading(true);
    try {
      const [nextStory, nextTimeline, nextOptions] = await Promise.all([
        conversations.getConversation(id), conversations.getTimeline(id), conversations.listRolePlayGraphOptions(),
      ]);
      setStory(nextStory); setTimeline(nextTimeline); setOptions(nextOptions); setError(null);
    } catch (cause) { setError(localErrorMessage(cause)); }
    finally { if (showLoading) setLoading(false); }
  }, [id]);
  useEffect(() => { void reload(true); }, [reload]);
  const running = timeline?.turns.some((turn) => turn.candidates.some((candidate) => candidate.status === "running")) ?? false;
  const runIds = timeline?.turns.flatMap((turn) => turn.candidates
    .filter((candidate) => candidate.status === "running")
    .map((candidate) => candidate.runId)) ?? [];
  const waits = useLocalWaits(runIds);
  useEffect(() => {
    if (!running) return;
    let active = true;
    void bridge.listen("zhuangsheng://run-events", () => { if (active) void reload(); }).then((unlisten) => {
      if (!active) unlisten(); else cleanup = unlisten;
    });
    let cleanup: () => void = () => {};
    return () => { active = false; cleanup(); };
  }, [reload, running]);
  const act = async (kind: NonNullable<typeof pending>, command: () => Promise<unknown>) => {
    setPending(kind); setActionError(null);
    try { await command(); await reload(); }
    catch (cause) { setActionError(localErrorMessage(cause)); throw cause; }
    finally { setPending(null); }
  };
  const run = story?.runProfile;
  const runSpec = (): ConversationRunSpec => {
    if (!run) throw new Error("故事运行设置尚未就绪。");
    return { graphRevisionId: run.graphRevisionId, replyOutputKey: run.replyOutputKey, inputShape: "conversation_message_v1" };
  };
  const resolveProjection = async (turnId: string, runId: string, branchId: string, resolution: CandidateProjectionResolution) => {
    if (!story) throw new Error("故事尚未就绪。");
    await act("projection", async () => {
      const branch = (await conversations.listContextBranches(story.contextId)).find((item) => item.branchId === branchId && item.status === "active");
      if (!branch) throw new Error("候选分支已变化，请刷新后重试。");
      await conversations.resolveCandidateProjection(turnId, runId, { expectedCurrentBranchHead: branch.headCommitId, resolution });
    });
  };
  return <StoryDetail story={story} timeline={timeline} graphOptions={options} loading={loading} optionsLoading={loading} pendingAction={pending} error={error} optionsError={null} profileError={actionError} turnError={actionError} candidateError={actionError} liveCandidates={[]} waits={waits.waits} handledWaits={waits.handledWaits} secretStatus={waits.secretStatus} waitPendingId={waits.pendingWaitId} waitError={waits.waitError} waitActionErrors={waits.actionErrors} onBack={onBack} onReload={() => void reload(true)} onReloadOptions={() => void reload()} onSaveRunProfile={(next) => act("profile", () => conversations.updateConversationRunProfile(id, { expectedRevisionNo: story?.runProfile?.revisionNo ?? 0, run: next }))} onSubmitMessage={(text) => act("turn", () => conversations.submitConversationTurn(id, { expectedHeadCommitId: timeline?.activeHeadCommitId ?? "", userContent: [{ type: "text", text }], run: runSpec() }))} onRegenerateCandidate={(turnId, userCommitId) => act("regenerate", () => conversations.regenerateConversationCandidate(turnId, { expectedUserCommitId: userCommitId, run: runSpec() }))} onSelectCandidate={(turnId, runId) => act("selection", () => conversations.selectConversationCandidate(turnId, { selectedRunId: runId, expectedConversationHeadCommitId: timeline?.activeHeadCommitId ?? "" }))} onResolveCandidateProjection={resolveProjection} onSubmitApproval={waits.submitApproval} onSubmitMemoryProposals={waits.submitMemoryProposals} onSubmitHumanResponse={waits.submitHumanResponse} onSubmitSecretPassword={waits.submitSecretPassword} onResolveEffect={waits.resolveEffect} onReloadWaits={waits.reloadWaits} onInspectRun={onInspectRun} />;
}
