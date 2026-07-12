import { useState } from "react";
import { Eye, RefreshCw } from "lucide-react";

import type { CandidateProjectionResolution, CandidateStatus, ConversationTimelineView } from "@zhuangsheng/api-client";
import { Badge, Button, Card } from "@zhuangsheng/ui";

import { shortId } from "./story-format";

interface StoryCandidatesProps {
  timeline: ConversationTimelineView | null;
  canRegenerate: boolean;
  pending: boolean;
  error: string | null;
  onRegenerate: (turnId: string, userCommitId: string) => Promise<void>;
  onSelect: (turnId: string, runId: string) => Promise<void>;
  onResolveProjection: (
    turnId: string,
    runId: string,
    branchId: string,
    resolution: CandidateProjectionResolution,
  ) => Promise<void>;
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
  const [confirmSelection, setConfirmSelection] = useState<{
    turnId: string;
    runId: string;
    laterTurns: number;
  } | null>(null);
  const turns = props.timeline?.turns ?? [];
  const select = async (turnId: string, runId: string) => {
    try {
      await props.onSelect(turnId, runId);
      setConfirmSelection(null);
    } catch {
      // The route owner retains the authoritative command error.
    }
  };
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
                const canSelect = candidate.status === "ready" && (!selected || !latest);
                const confirming = confirmSelection?.turnId === turn.id
                  && confirmSelection.runId === candidate.runId;
                return (
                  <div key={candidate.runId} className="rounded-xl border border-default p-3">
                    <div className="flex items-center justify-between gap-2">
                      <span className="font-mono text-xs text-secondary">{shortId(candidate.runId)}</span>
                      <Badge tone={selected ? (latest ? "info" : "neutral") : tone}>{selected ? (latest ? "当前采用" : "曾采用") : label}</Badge>
                    </div>
                    {candidate.projectionError && <p className="mt-2 text-xs text-warning">{candidate.projectionError.safeMessage}</p>}
                    {candidate.status === "projection_conflicted" && (
                      <div className="mt-3 grid gap-2">
                        <Button size="compact" variant="secondary" disabled={props.pending} onClick={() => void safely(() => props.onResolveProjection(
                          turn.id,
                          candidate.runId,
                          candidate.branchId,
                          { type: "append_after_current", reason: "user reviewed and kept the advanced branch" },
                        ))}>附加回复到当前分支</Button>
                        <Button size="compact" variant="ghost" disabled={props.pending} onClick={() => void safely(() => props.onResolveProjection(
                          turn.id,
                          candidate.runId,
                          candidate.branchId,
                          { type: "abandon_projection", reason: "user chose to abandon the conflicted reply" },
                        ))}>放弃此冲突回复</Button>
                      </div>
                    )}
                    <Button className="mt-3 w-full" size="compact" variant="ghost" onClick={() => props.onInspectRun(candidate.runId)}><Eye className="size-3.5" />检查运行</Button>
                    {canSelect && !confirming && (
                      <Button
                        className="mt-3 w-full"
                        size="compact"
                        variant="secondary"
                        disabled={props.pending}
                        aria-label={`${latest ? "采用这个回复" : "从此处继续"} ${shortId(candidate.runId)}`}
                        onClick={() => latest
                          ? void select(turn.id, candidate.runId)
                          : setConfirmSelection({
                              turnId: turn.id,
                              runId: candidate.runId,
                              laterTurns: turns.length - index - 1,
                            })}
                      >
                        {latest ? "采用这个回复" : "从此处继续"}
                      </Button>
                    )}
                    {confirming && (
                      <div className="mt-3 rounded-xl border border-warning/30 bg-warning/5 p-3 text-xs">
                        <p className="text-warning">这会从该回复创建新分支；后续 {confirmSelection.laterTurns} 轮历史仍会保留。</p>
                        <div className="mt-2 flex gap-2">
                          <Button size="compact" disabled={props.pending} onClick={() => void select(turn.id, candidate.runId)}>确认从此处继续</Button>
                          <Button size="compact" variant="ghost" onClick={() => setConfirmSelection(null)}>取消</Button>
                        </div>
                      </div>
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
