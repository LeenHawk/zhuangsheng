import { CheckCircle2, CircleAlert } from "lucide-react";

import type {
  EffectResolutionKind,
  SecretStoreStatusView,
  ToolApprovalDecisionInput,
  WaitView,
} from "@zhuangsheng/api-client";
import { Button, Card } from "@zhuangsheng/ui";

import type { HandledWaitSummary } from "./story-detail";
import { SecretUnlockCard } from "./secret-unlock-card";
import { ToolApprovalCard } from "./tool-approval-card";
import { EffectResolutionCard } from "./effect-resolution-card";

interface StoryWaitActionsProps {
  waits: WaitView[];
  handled: HandledWaitSummary[];
  secretStatus: SecretStoreStatusView | null;
  pendingWaitId: string | null;
  loadError: string | null;
  actionErrors: Record<string, string>;
  onSubmitApproval: (wait: WaitView, decisions: ToolApprovalDecisionInput[]) => Promise<void>;
  onSubmitSecretPassword: (wait: WaitView, mode: "initialize" | "unlock", password: string) => Promise<void>;
  onResolveEffect: (wait: WaitView, kind: EffectResolutionKind, reason: string) => Promise<void>;
  onReload: () => void;
}

export function StoryWaitActions(props: StoryWaitActionsProps) {
  if (props.waits.length === 0 && props.handled.length === 0 && !props.loadError) return null;
  return (
    <section className="mx-auto mt-6 max-w-3xl space-y-3" aria-label="需要处理的角色行动">
      {props.waits.map((wait) => {
        const common = {
          wait,
          pending: props.pendingWaitId === wait.id,
          error: props.actionErrors[wait.id] || null,
        };
        if (wait.request.kind === "tool_approval") {
          return <ToolApprovalCard key={wait.id} {...common} onSubmit={props.onSubmitApproval} />;
        }
        if (wait.request.kind === "secret_store_unlocked") {
          return <SecretUnlockCard key={wait.id} {...common} status={props.secretStatus} onSubmit={props.onSubmitSecretPassword} />;
        }
        if (wait.request.kind === "effect_resolution") {
          return <EffectResolutionCard key={wait.id} {...common} onSubmit={props.onResolveEffect} />;
        }
        return (
          <Card key={wait.id} className="border-warning/30 p-5">
            <div className="flex items-center gap-2 font-semibold"><CircleAlert className="size-5 text-warning" />需要进一步处理</div>
            <p className="mt-2 text-sm text-secondary">此等待类型尚不能在用户模式安全处理，请刷新状态或进入专家诊断。</p>
          </Card>
        );
      })}
      {props.handled.map((handled) => (
        <Card key={handled.waitId} className="border-success/30 p-4 text-sm text-secondary">
          <div className="flex items-center gap-2"><CheckCircle2 className="size-4 text-success" /><span>{handled.summary}</span></div>
        </Card>
      ))}
      {props.loadError && (
        <Card className="border-danger/30 p-4 text-sm text-danger">
          <p>无法读取等待状态：{props.loadError}</p>
          <Button className="mt-3" type="button" size="compact" variant="secondary" onClick={props.onReload}>重试</Button>
        </Card>
      )}
    </section>
  );
}
