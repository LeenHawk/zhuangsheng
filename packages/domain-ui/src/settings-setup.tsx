import { AlertCircle, Loader2, RefreshCw, Settings2 } from "lucide-react";

import type { ChannelView, ContextPresetPreviewView, ContextPresetView, RolePlayGraphOptionView, SecretMetadataView, SecretStoreStatusView } from "@zhuangsheng/api-client";
import { Badge, Button } from "@zhuangsheng/ui";

import { ChannelSetupCard } from "./channel-setup-card";
import { AgentTemplateSetupCard } from "./agent-template-setup-card";
import { RolePresetSetupCard } from "./role-preset-setup-card";
import { SecretSetupCard } from "./secret-setup-card";

interface Props {
  status: SecretStoreStatusView | null; secrets: SecretMetadataView[]; channels: ChannelView[]; presets: ContextPresetView[]; templates: RolePlayGraphOptionView[];
  preview: ContextPresetPreviewView | null; loading: boolean; pending: "secret" | "channel" | "preset" | "template" | "preview" | null; error: string | null; onReload: () => void;
  onStoreSecret: React.ComponentProps<typeof SecretSetupCard>["onSubmit"];
  onPublishChannel: React.ComponentProps<typeof ChannelSetupCard>["onSubmit"];
  onPublishPreset: React.ComponentProps<typeof RolePresetSetupCard>["onSubmit"];
  onPreviewPreset: React.ComponentProps<typeof RolePresetSetupCard>["onPreview"];
  onCreateTemplate: React.ComponentProps<typeof AgentTemplateSetupCard>["onSubmit"];
}

export function SettingsSetup(props: Props) {
  const ready = props.templates.length > 0;
  return (
    <div className="mx-auto max-w-5xl space-y-4">
      <header className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between"><div><Badge tone="info">用户模式设置</Badge><h1 className="mt-3 flex items-center gap-2 font-display text-3xl font-bold"><Settings2 className="size-7" />首次运行配置</h1><p className="mt-2 max-w-2xl text-secondary">安全凭据、模型连接和角色模板分别使用自己的版本与权限边界。</p></div><Badge tone={ready ? "success" : "warning"}>{ready ? "基础资源已就绪" : "还需完成配置"}</Badge></header>
      {props.loading && <div className="flex items-center gap-2 rounded-xl border border-default bg-surface p-4 text-sm text-secondary"><Loader2 className="size-4 animate-spin" />正在读取本地配置…</div>}
      {props.error && <div role="alert" className="flex items-center gap-2 rounded-xl border border-danger/25 bg-danger/5 p-3 text-sm text-danger"><AlertCircle className="size-4" /><span className="flex-1">{props.error}</span><Button size="compact" variant="secondary" onClick={props.onReload}><RefreshCw className="size-3.5" />刷新</Button></div>}
      {!props.loading && <>
        <SecretSetupCard status={props.status} secrets={props.secrets} pending={props.pending === "secret"} onSubmit={props.onStoreSecret} />
        <ChannelSetupCard channels={props.channels} secrets={props.secrets} pending={props.pending === "channel"} onSubmit={props.onPublishChannel} />
        <RolePresetSetupCard presets={props.presets} preview={props.preview} pending={props.pending === "preset"} previewPending={props.pending === "preview"} onSubmit={props.onPublishPreset} onPreview={props.onPreviewPreset} />
        <AgentTemplateSetupCard channels={props.channels} presets={props.presets} templates={props.templates} pending={props.pending === "template"} onSubmit={props.onCreateTemplate} />
      </>}
    </div>
  );
}
