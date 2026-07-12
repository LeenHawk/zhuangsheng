import { Eye, RefreshCw } from "lucide-react";

import type { CandidateStatus, ConversationTimelineView } from "@zhuangsheng/api-client";
import { Badge, Button, Card } from "@zhuangsheng/ui";

import { shortId } from "./story-format";

interface StoryCandidatesProps {
  timeline: ConversationTimelineView | null;
  canRegenerate: boolean;
  pending: boolean;
  error: string | null;
  onRegenerate: (turnId: string, userCommitId: string) => Promise<void>;
  onSelect: (turnId: string, runId: string) => Promise<void>;
  onInspectRun: (runId: string) => void;
}

const statusLabel: Record<CandidateStatus, readonly [string, "running" | "success" | "danger" | "neutral" | "warning"]> = {
  running: ["角色正在回应", "running"],
  ready: ["可采用", "success"],
  failed: ["运行失败", "danger"],
  cancelled: ["已取消", "neutral"],
  projection_conflicted: ["故事分支冲突", "warning"],
  projection_failed: ["无法写入故事", "danger"],
  projection_abandoned: ["已放弃", "neutral"],
};

export function StoryCandidates(props: StoryCandidatesProps) {
  const turns = props.timeline?.turns ?? [];
  return (
    <Card className="p-5">
      <h2 className="font-semibold">回复候选</h2>
      <div className="mt-3 space-y-4">
        {turns.map((turn, index) => {
          const latest = index === turns.length - 1;
          const running = turn.candidates.some((candidate) => candidate.status === "running");
          return (
            <section key={turn.id} className="space-y-2 border-b border-default pb-4 last:border-0 last:pb-0">
              <div className="flex items-center justify-between text-xs text-muted">
                <span>第 {index + 1} 轮</span><span>{turn.candidates.length} 个候选</span>
              </div>
              {turn.candidates.map((candidate) => {
                const [label, tone] = statusLabel[candidate.status];
                const selected = turn.selectedRunId === candidate.runId;
                return (
                  <div key={candidate.runId} className="rounded-xl border border-default p-3">
                    <div className="flex items-center justify-between gap-2">
                      <span className="font-mono text-xs text-secondary">{shortId(candidate.runId)}</span>
                      <Badge tone={selected ? "info" : tone}>{selected ? "当前采用" : label}</Badge>
                    </div>
                    {candidate.projectionError && <p className="mt-2 text-xs text-warning">{candidate.projectionError.safeMessage}</p>}
                    <Button className="mt-3 w-full" size="compact" variant="ghost" onClick={() => props.onInspectRun(candidate.runId)}><Eye className="size-3.5" />检查运行</Button>
                    {latest && candidate.status === "ready" && !selected && (
                      <Button
                        className="mt-3 w-full"
                        size="compact"
                        variant="secondary"
                        disabled={props.pending}
                        onClick={() => void safely(() => props.onSelect(turn.id, candidate.runId))}
                      >
                        采用这个回复
                      </Button>
                    )}
                  </div>
                );
              })}
              {latest && (
                <Button
                  className="w-full"
                  size="compact"
                  variant="ghost"
                  disabled={!props.canRegenerate || props.pending || running}
                  onClick={() => void safely(() => props.onRegenerate(turn.id, turn.userCommitId))}
                >
                  <RefreshCw className="size-3.5" />{running ? "候选生成中" : "再生成一个"}
                </Button>
              )}
            </section>
          );
        })}
        {turns.length === 0 && <p className="text-sm text-muted">暂无候选</p>}
        {props.error && <p className="text-xs text-danger">{props.error}</p>}
      </div>
    </Card>
  );
}

async function safely(action: () => Promise<void>) {
  try {
    await action();
  } catch {
    // The route owner renders the typed command error without losing candidate state.
  }
}
