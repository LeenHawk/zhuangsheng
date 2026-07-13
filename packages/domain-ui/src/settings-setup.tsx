import { AlertCircle, Loader2, RefreshCw, Settings2 } from "lucide-react";

import type { ChannelModelDiscoveryView, ChannelView, ContextPresetPreviewView, ContextPresetView, DiscoveredChannelModel, RolePlayGraphOptionView, RolePlaySettingsView, SecretMetadataView, SecretStoreStatusView } from "@zhuangsheng/api-client";
import { Badge, Button } from "@zhuangsheng/ui";

import { ChannelSetupCard } from "./channel-setup-card";
import { AgentTemplateSetupCard } from "./agent-template-setup-card";
import { RolePresetSetupCard } from "./role-preset-setup-card";
import { SecretSetupCard } from "./secret-setup-card";
import { SecretStoreControls } from "./secret-store-controls";

interface Props {
  status: SecretStoreStatusView | null; secrets: SecretMetadataView[]; channels: ChannelView[]; presets: ContextPresetView[]; templates: RolePlayGraphOptionView[];
  preview: ContextPresetPreviewView | null; discovery: ChannelModelDiscoveryView | null; rolePlaySettings: RolePlaySettingsView | null; loading: boolean; pending: "secret" | "secret_control" | "channel" | "preset" | "template" | "preview" | "discovery" | "model" | "settings" | null; error: string | null; onReload: () => void;
  onStoreSecret: React.ComponentProps<typeof SecretSetupCard>["onSubmit"];
  onUnlockSecretStore: React.ComponentProps<typeof SecretStoreControls>["onUnlock"];
  onLockSecretStore: React.ComponentProps<typeof SecretStoreControls>["onLock"];
  onChangeSecretStorePassword: React.ComponentProps<typeof SecretStoreControls>["onChangePassword"];
  onPublishChannel: React.ComponentProps<typeof ChannelSetupCard>["onSubmit"];
  onPublishPreset: React.ComponentProps<typeof RolePresetSetupCard>["onSubmit"];
  onPreviewPreset: React.ComponentProps<typeof RolePresetSetupCard>["onPreview"];
  onCreateTemplate: React.ComponentProps<typeof AgentTemplateSetupCard>["onSubmit"];
  onDiscoverModels: (channel: ChannelView) => void;
  onPublishDiscoveredModel: (model: DiscoveredChannelModel, structuredOutput: boolean) => Promise<void>;
  onInspectTemplate: (template: RolePlayGraphOptionView) => void;
}

export function SettingsSetup(props: Props) {
  const ready = props.templates.length > 0;
  return (
    <div className="space-y-4">
      <header className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between"><div><Badge tone="info">作用域：模型连接与共享创作模板</Badge><h1 className="mt-3 flex items-center gap-2 font-display text-3xl font-bold"><Settings2 className="size-7" />Models & Connections</h1><p className="mt-2 max-w-2xl text-secondary">安全凭据、模型连接和角色模板分别使用自己的版本与权限边界；新版本不改写历史 Run。</p></div><Badge tone={ready ? "success" : "warning"}>{ready ? "基础资源已就绪" : "还需完成配置"}</Badge></header>
      {props.loading && <div className="flex items-center gap-2 rounded-xl border border-default bg-surface p-4 text-sm text-secondary"><Loader2 className="size-4 animate-spin" />正在读取本地配置…</div>}
      {props.error && <div role="alert" className="flex items-center gap-2 rounded-xl border border-danger/25 bg-danger/5 p-3 text-sm text-danger"><AlertCircle className="size-4" /><span className="flex-1">{props.error}</span><Button size="compact" variant="secondary" onClick={props.onReload}><RefreshCw className="size-3.5" />刷新</Button></div>}
      {!props.loading && <>
        <SecretSetupCard status={props.status} secrets={props.secrets} pending={props.pending === "secret"} onSubmit={props.onStoreSecret} />
        {props.status?.initialized && <SecretStoreControls status={props.status} pending={props.pending === "secret_control"} onUnlock={props.onUnlockSecretStore} onLock={props.onLockSecretStore} onChangePassword={props.onChangeSecretStorePassword} />}
        <ChannelSetupCard channels={props.channels} secrets={props.secrets} discovery={props.discovery} publishPending={props.pending === "channel" || props.pending === "model"} discoveryPending={props.pending === "discovery"} onSubmit={props.onPublishChannel} onDiscover={props.onDiscoverModels} onPublishDiscovered={props.onPublishDiscoveredModel} />
        <RolePresetSetupCard presets={props.presets} preview={props.preview} pending={props.pending === "preset"} previewPending={props.pending === "preview"} onSubmit={props.onPublishPreset} onPreview={props.onPreviewPreset} />
        <AgentTemplateSetupCard channels={props.channels} presets={props.presets} templates={props.templates} settings={props.rolePlaySettings} pending={props.pending === "template"} settingsPending={props.pending === "settings"} onSubmit={props.onCreateTemplate} onInspect={props.onInspectTemplate} />
      </>}
    </div>
  );
}
