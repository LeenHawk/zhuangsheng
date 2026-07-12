import { LockKeyhole } from "lucide-react";

import type { RolePlaySettingsView } from "@zhuangsheng/api-client";
import { Badge } from "@zhuangsheng/ui";

export function RolePlaySettingsPanel({ settings }: { settings: RolePlaySettingsView }) {
  const locked = settings.compatibility.mode === "partial"
    ? settings.compatibility.lockedReasons
    : [];
  return (
    <div className="mt-3 grid gap-2 border-t border-default pt-3 text-xs md:grid-cols-2">
      <Setting label="模型" value={settings.model.modelName || settings.model.modelId} />
      <Setting label="Channel" value={settings.model.channelId} />
      <Setting label="主 LLM 节点" value={settings.primaryLlmNodeId} />
      <Setting label="ContextPreset" value={settings.contextPresetId || "内联 / 未绑定"} />
      <Setting label="生成参数" value={settings.generation ? JSON.stringify(settings.generation) : "默认"} />
      <Setting label="流式输出" value={settings.streaming ? `${settings.streaming.enabled ? "启用" : "停用"} · ${settings.streaming.audience}` : "默认"} />
      {locked.length > 0 && <div className="md:col-span-2"><div className="flex items-center gap-1.5 text-secondary"><LockKeyhole className="size-3.5" />受保护的专家设置</div><div className="mt-1 flex flex-wrap gap-1">{locked.map((reason) => <Badge key={reason} tone="warning">{lockedLabel(reason)}</Badge>)}</div></div>}
    </div>
  );
}

function Setting({ label, value }: { label: string; value: string }) {
  return <div className="min-w-0 rounded-lg bg-canvas px-3 py-2"><span className="text-muted">{label}</span><p className="mt-1 break-all text-secondary">{value}</p></div>;
}

const lockedLabel = (reason: string) => ({
  custom_coordination_nodes: "自定义协调节点",
  tool_permissions_require_expert: "工具权限",
  memory_binding_requires_expert: "Memory binding",
  unknown_context_items: "未知 Context 项",
  context_preset_profile_unavailable: "ContextPreset 不可分析",
}[reason] ?? reason);
