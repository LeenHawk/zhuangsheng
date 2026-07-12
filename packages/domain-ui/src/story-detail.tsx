import { ArrowLeft, RefreshCw } from "lucide-react";

import type {
  ConversationRunSpec,
  ConversationTimelineView,
  ConversationView,
  RolePlayGraphOptionView,
  RunStreamConnectionState,
} from "@zhuangsheng/api-client";
import { Button, Card } from "@zhuangsheng/ui";

import { StoryComposer } from "./story-composer";
import { shortId } from "./story-format";
import { StoryMessages } from "./story-messages";
import { StorySidebar } from "./story-sidebar";

export interface StoryDetailProps {
  story: ConversationView | null;
  timeline: ConversationTimelineView | null;
  graphOptions: RolePlayGraphOptionView[];
  loading: boolean;
  optionsLoading: boolean;
  pendingAction: "profile" | "turn" | "regenerate" | "selection" | null;
  error: string | null;
  optionsError: string | null;
  profileError: string | null;
  turnError: string | null;
  candidateError: string | null;
  liveCandidates: StoryLiveCandidate[];
  onBack: () => void;
  onReload: () => void;
  onReloadOptions: () => void;
  onSaveRunProfile: (run: ConversationRunSpec) => Promise<void>;
  onSubmitMessage: (text: string) => Promise<void>;
  onRegenerateCandidate: (turnId: string, userCommitId: string) => Promise<void>;
  onSelectCandidate: (turnId: string, runId: string) => Promise<void>;
}

export interface StoryLiveCandidate {
  runId: string;
  connection: RunStreamConnectionState;
  text: string;
  truncated: boolean;
  error: string | null;
}

export function StoryDetail(props: StoryDetailProps) {
  const { story, timeline } = props;
  return (
    <div className="mx-auto grid max-w-7xl gap-6 pb-24 lg:grid-cols-[minmax(0,1fr)_320px]">
      <section className="min-w-0">
        <header className="flex items-center gap-3">
          <Button variant="ghost" size="icon" onClick={props.onBack} aria-label="返回故事列表">
            <ArrowLeft className="size-5" />
          </Button>
          <div className="min-w-0">
            <h1 className="truncate font-display text-2xl font-bold">{story?.title || "未命名故事"}</h1>
            <p className="mt-0.5 text-xs text-muted">
              active ancestry · {timeline ? shortId(timeline.activeHeadCommitId) : "加载中"}
            </p>
          </div>
          <Button className="ml-auto" variant="secondary" size="compact" onClick={props.onReload}>
            <RefreshCw className="size-3.5" />刷新
          </Button>
        </header>
        {props.error && <Card className="mt-5 border-danger/30 p-4 text-sm text-danger">{props.error}</Card>}
        <StoryMessages timeline={timeline} loading={props.loading} liveCandidates={props.liveCandidates} />
        <StoryComposer
          enabled={story?.runProfile !== null && story?.runProfile !== undefined}
          pending={props.pendingAction !== null || hasRunningCandidate(timeline)}
          error={props.turnError}
          onSubmit={props.onSubmitMessage}
        />
      </section>
      <StorySidebar {...props} />
    </div>
  );
}

const hasRunningCandidate = (timeline: ConversationTimelineView | null) =>
  timeline?.turns.some((turn) => turn.candidates.some((candidate) => candidate.status === "running")) ?? false;
