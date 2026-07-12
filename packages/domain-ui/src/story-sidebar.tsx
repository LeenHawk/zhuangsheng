import { GitBranch } from "lucide-react";

import { Card } from "@zhuangsheng/ui";

import type { StoryDetailProps } from "./story-detail";
import { StoryCandidates } from "./story-candidates";
import { StorySettings } from "./story-settings";

export function StorySidebar(props: StoryDetailProps) {
  const { timeline } = props;
  return (
    <aside className="space-y-4 lg:sticky lg:top-24 lg:self-start">
      <StorySettings
        story={props.story}
        options={props.graphOptions}
        loading={props.optionsLoading}
        pending={props.pendingAction !== null}
        error={props.optionsError}
        commandError={props.profileError}
        onReload={props.onReloadOptions}
        onSave={props.onSaveRunProfile}
      />
      <Card className="p-5">
        <div className="flex items-center gap-2 font-semibold"><GitBranch className="size-4 text-accent" />当前故事分支</div>
        <dl className="mt-4 space-y-3 text-xs">
          <div><dt className="text-muted">Branch</dt><dd className="mt-1 break-all font-mono text-secondary">{timeline?.activeBranchId || "—"}</dd></div>
          <div><dt className="text-muted">Head</dt><dd className="mt-1 break-all font-mono text-secondary">{timeline?.activeHeadCommitId || "—"}</dd></div>
        </dl>
      </Card>
      <StoryCandidates
        timeline={timeline}
        canRegenerate={props.story?.runProfile != null}
        pending={props.pendingAction === "regenerate" || props.pendingAction === "selection"}
        error={props.candidateError}
        onRegenerate={props.onRegenerateCandidate}
        onSelect={props.onSelectCandidate}
        onInspectRun={props.onInspectRun}
      />
    </aside>
  );
}
