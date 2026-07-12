import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Settings2 } from "lucide-react";

import type {
  ConversationRunSpec,
  ConversationView,
  RolePlayCompatibilityView,
  RolePlayGraphOptionView,
} from "@zhuangsheng/api-client";
import { Badge, Button, Card } from "@zhuangsheng/ui";

interface StorySettingsProps {
  story: ConversationView | null;
  options: RolePlayGraphOptionView[];
  loading: boolean;
  pending: boolean;
  error: string | null;
  commandError: string | null;
  onReload: () => void;
  onSave: (run: ConversationRunSpec) => Promise<void>;
}

export function StorySettings(props: StorySettingsProps) {
  const available = useMemo(
    () => props.options.filter((option) => option.replyOutputKeys.length > 0),
    [props.options],
  );
  const [revisionId, setRevisionId] = useState("");
  const [replyOutputKey, setReplyOutputKey] = useState("");

  useEffect(() => {
    const current = available.find((option) => option.revisionId === props.story?.runProfile?.graphRevisionId);
    const next = current ?? (props.story?.runProfile ? undefined : available[0]);
    setRevisionId(next?.revisionId ?? "");
    setReplyOutputKey(
      next?.replyOutputKeys.includes(props.story?.runProfile?.replyOutputKey ?? "")
        ? props.story?.runProfile?.replyOutputKey ?? ""
        : next?.replyOutputKeys[0] ?? "",
    );
  }, [available, props.story?.id, props.story?.runProfile]);

  const selected = available.find((option) => option.revisionId === revisionId);
  const currentProfile = props.story?.runProfile;
  const changed = Boolean(
    selected &&
    (currentProfile?.graphRevisionId !== revisionId || currentProfile?.replyOutputKey !== replyOutputKey),
  );
  const selectGraph = (nextRevisionId: string) => {
    const option = available.find((item) => item.revisionId === nextRevisionId);
    setRevisionId(nextRevisionId);
    setReplyOutputKey(option?.replyOutputKeys[0] ?? "");
  };
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!selected || !replyOutputKey || !changed) return;
    try {
      await props.onSave({
        graphRevisionId: selected.revisionId,
        replyOutputKey,
        inputShape: "conversation_message_v1",
      });
    } catch {
      // The command owner keeps the current selection and renders the typed API error.
    }
  };

  return (
    <Card className="p-5">
      <div className="flex items-center gap-2 font-semibold">
        <Settings2 className="size-4 text-accent" />故事运行设置
      </div>
      {props.story?.runProfile && !available.some((option) => option.revisionId === props.story?.runProfile?.graphRevisionId) && (
        <p className="mt-3 rounded-xl bg-elevated p-3 text-xs text-secondary">
          当前故事固定在旧 revision。选择新模板前，它仍会用于后续消息。
        </p>
      )}
      <form className="mt-4 space-y-4" onSubmit={submit}>
        <label className="block text-xs font-semibold text-secondary">
          Agent 模板
          <select
            className="mt-1.5 min-h-10 w-full rounded-xl border border-default bg-canvas px-3 text-sm text-primary outline-none focus:border-accent focus:ring-2 focus:ring-accent/20"
            value={revisionId}
            onChange={(event) => selectGraph(event.target.value)}
            disabled={props.loading || props.pending}
          >
            <option value="">选择可运行模板</option>
            {available.map((option) => (
              <option key={option.revisionId} value={option.revisionId}>
                {option.graphName} · r{option.revisionNo} · {modeLabel(option.compatibility.mode)}
              </option>
            ))}
          </select>
        </label>
        {selected && selected.replyOutputKeys.length > 1 && (
          <label className="block text-xs font-semibold text-secondary">
            回复输出
            <select
              className="mt-1.5 min-h-10 w-full rounded-xl border border-default bg-canvas px-3 text-sm text-primary"
              value={replyOutputKey}
              onChange={(event) => setReplyOutputKey(event.target.value)}
              disabled={props.pending}
            >
              {selected.replyOutputKeys.map((key) => <option key={key}>{key}</option>)}
            </select>
          </label>
        )}
        {selected && <CompatibilitySummary compatibility={selected.compatibility} />}
        {props.loading && <p className="text-xs text-muted">正在读取服务端兼容性投影…</p>}
        {!props.loading && available.length === 0 && (
          <p className="text-xs text-warning">还没有符合 Conversation 输入/回复合同的 applied Graph。</p>
        )}
        {props.error && (
          <div className="text-xs text-danger">
            <p>{props.error}</p>
            <Button className="mt-2" type="button" size="compact" variant="secondary" onClick={props.onReload}>重试</Button>
          </div>
        )}
        {props.commandError && <p className="text-xs text-danger">{props.commandError}</p>}
        <Button className="w-full" type="submit" size="compact" disabled={!changed || props.pending}>
          {props.pending ? "保存中…" : "用于后续消息"}
        </Button>
      </form>
    </Card>
  );
}

function CompatibilitySummary({ compatibility }: { compatibility: RolePlayCompatibilityView }) {
  const tone = compatibility.mode === "editable" ? "success" : compatibility.mode === "partial" ? "warning" : "neutral";
  const reasons = compatibility.mode === "partial" ? compatibility.lockedReasons : compatibility.mode === "expert_only" ? compatibility.reasons : [];
  return (
    <div className="rounded-xl border border-default p-3 text-xs text-secondary">
      <Badge tone={tone}>{modeLabel(compatibility.mode)}</Badge>
      <p className="mt-2">{compatibilityText(compatibility)}</p>
      {reasons.length > 0 && (
        <ul className="mt-2 list-disc space-y-1 pl-4 text-muted">
          {reasons.map((reason) => <li key={reason}>{reasonText(reason)}</li>)}
        </ul>
      )}
    </div>
  );
}

const modeLabel = (mode: RolePlayCompatibilityView["mode"]) =>
  mode === "editable" ? "用户模式兼容" : mode === "partial" ? "含高级设置" : "专家配置";

const compatibilityText = (value: RolePlayCompatibilityView) => {
  if (value.mode === "editable") return "常用模型、生成与上下文字段可由用户模式无损映射。";
  if (value.mode === "partial") return `只调整已识别字段；${value.lockedReasons.length} 项高级配置会原样保留。`;
  return "可以用于故事运行，但模板内容需要在专家模式中编辑。";
};

const reasonText = (reason: string) => ({
  conversation_contract_incompatible: "输入或回复合同不兼容",
  primary_llm_node_not_unique: "无法识别唯一的主生成节点",
  custom_coordination_nodes: "包含自定义协调节点",
  tool_permissions_require_expert: "包含工具权限配置",
  memory_binding_requires_expert: "包含记忆绑定",
  context_preset_profile_unavailable: "ContextPreset 尚无可分析版本",
  unknown_context_items: "包含用户模式不识别的上下文条目",
}[reason] ?? `高级配置：${reason}`);
