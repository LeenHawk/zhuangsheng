import { useCallback, useEffect, useRef, useState } from "react";

import {
  createIdempotencyKey,
  createOpeningConversation,
  stringifyJsonExact,
  type CandidateProjectionResolution,
  type ConversationAttentionView,
  type ConversationRunSpec,
  type ConversationTimelineView,
  type ConversationView,
  type RolePlayGraphOptionView,
  type RolePlaySettingsView,
  type SecretStoreStatusView,
} from "@zhuangsheng/api-client";
import { notifyShellStatusChanged, StoryDetail, StoryList } from "@zhuangsheng/domain-ui";

import { bridge, config, conversations, localErrorMessage, secrets } from "./bridge";
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
  const [attention, setAttention] = useState<ConversationAttentionView[]>([]);
  const [templates, setTemplates] = useState<RolePlayGraphOptionView[]>([]);
  const [templateSettings, setTemplateSettings] = useState<Record<string, RolePlaySettingsView | null>>({});
  const [secretStatus, setSecretStatus] = useState<SecretStoreStatusView | null>(null);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    setLoading(true); setError(null);
    try {
      const [list, options, status] = await Promise.all([
        conversations.listConversations(), conversations.listRolePlayGraphOptions(), secrets.status(),
      ]);
      setStories(list.items); setAttention(list.attention); setTemplates(options); setSecretStatus(status);
      const details = await Promise.allSettled(options.map((template) => config.getRolePlaySettings(template.revisionId)));
      setTemplateSettings(Object.fromEntries(options.map((template, index) => [template.revisionId, details[index]?.status === "fulfilled" ? details[index].value : null])));
    } catch (cause) { setError(localErrorMessage(cause)); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void reload(); }, [reload]);
  const createKeys = useRef<{ signature: string; conversation: string; turn: string } | null>(null);
  const create = async (title: string | undefined, defaultRun: ConversationRunSpec, openingMessage: string) => {
    const signature = stringifyJsonExact({ title: title ?? null, defaultRun, openingMessage });
    if (createKeys.current?.signature !== signature) createKeys.current = { signature, conversation: createIdempotencyKey(), turn: createIdempotencyKey() };
    const keys = createKeys.current;
    setPending(true); setError(null);
    try {
      const { conversation: story } = await createOpeningConversation(conversations, {
        title, run: defaultRun, openingMessage,
      }, keys);
      createKeys.current = null; onOpen(story.id);
    }
    catch (cause) { setError(localErrorMessage(cause)); throw cause; }
    finally { setPending(false); }
  };
  const unlock = async (masterPassword: string, idempotencyKey: string) => {
    const session = await secrets.unlock({ masterPassword, idempotencyKey });
    setSecretStatus({ initialized: true, storeId: session.storeId, formatVersion: session.formatVersion, locked: false });
    notifyShellStatusChanged();
  };
  return <StoryList stories={stories} attention={attention} templates={templates} templateSettings={templateSettings} secretStatus={secretStatus} loading={loading} pending={pending} error={error} onReload={() => void reload()} onCreate={create} onUnlockSecretStore={unlock} onOpen={onOpen} onConfigure={onConfigure} />;
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
