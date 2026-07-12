import { GitBranch } from "lucide-react";

import { Badge, Card } from "@zhuangsheng/ui";

import type { StoryDetailProps } from "./story-detail";
import { shortId } from "./story-format";
import { StorySettings } from "./story-settings";

const statusLabel = {
  running: ["角色正在回应", "running"],
  ready: ["已完成", "success"],
  failed: ["运行失败", "danger"],
  cancelled: ["已取消", "neutral"],
  projection_conflicted: ["回复与故事分支冲突", "warning"],
  projection_failed: ["回复无法写入故事", "danger"],
  projection_abandoned: ["已放弃此回复", "neutral"],
} as const;

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
      <Card className="p-5">
        <h2 className="font-semibold">回复候选</h2>
        <div className="mt-3 space-y-2">
          {timeline?.turns.flatMap((turn) => turn.candidates).map((candidate) => {
            const [label, tone] = statusLabel[candidate.status];
            return (
              <div key={candidate.runId} className="rounded-xl border border-default p-3">
                <div className="flex items-center justify-between gap-2">
                  <span className="font-mono text-xs text-secondary">{shortId(candidate.runId)}</span>
                  <Badge tone={tone}>{label}</Badge>
                </div>
                {candidate.projectionError && <p className="mt-2 text-xs text-warning">{candidate.projectionError.safeMessage}</p>}
              </div>
            );
          })}
          {timeline && timeline.turns.length === 0 && <p className="text-sm text-muted">暂无候选</p>}
        </div>
      </Card>
    </aside>
  );
}
