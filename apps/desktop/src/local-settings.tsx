import { useCallback, useEffect, useRef, useState } from "react";

import {
  createIdempotencyKey,
  stringifyJsonExact,
  type ChannelModelDiscoveryView,
  type ChannelRevisionView,
  type ChannelView,
  type ContextPresetPreviewView,
  type ContextPresetView,
  type DiscoveredChannelModel,
  type GenerationProviderKind,
  type RolePlayGraphOptionView,
  type RolePlaySettingsView,
  type SecretMetadataView,
  type SecretStoreStatusView,
} from "@zhuangsheng/api-client";
import {
  ApplicationSettings,
  buildRolePresetSpec,
  loadApplicationPreferences,
  notifyShellStatusChanged,
  PluginManager,
  saveApplicationPreferences,
  SettingsSetup,
  type RolePresetInput,
} from "@zhuangsheng/domain-ui";

import { config, conversations, localErrorMessage, plugins, secrets } from "./bridge";

type Pending = "secret" | "secret_control" | "channel" | "preset" | "template" | "preview" | "discovery" | "model" | "settings" | null;
interface ChannelInput { name: string; baseUrl: string; providerKind: GenerationProviderKind; modelId: string; credentialSecretId: string | null; structuredOutput: boolean }
interface SecretInput { secretId: string; name: string; value: string; masterPassword: string; passwordCommandKey: string; putCommandKey: string }

export function LocalSettings() {
  const [preferences, setPreferences] = useState(loadApplicationPreferences);
  const [status, setStatus] = useState<SecretStoreStatusView | null>(null);
  const [secretRefs, setSecretRefs] = useState<SecretMetadataView[]>([]);
  const [channels, setChannels] = useState<ChannelView[]>([]);
  const [presets, setPresets] = useState<ContextPresetView[]>([]);
  const [templates, setTemplates] = useState<RolePlayGraphOptionView[]>([]);
  const [preview, setPreview] = useState<ContextPresetPreviewView | null>(null);
  const [discovery, setDiscovery] = useState<ChannelModelDiscoveryView | null>(null);
  const [discoverySource, setDiscoverySource] = useState<ChannelRevisionView | null>(null);
  const [settings, setSettings] = useState<RolePlaySettingsView | null>(null);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState<Pending>(null);
  const [error, setError] = useState<string | null>(null);
  const keys = useRef(new Map<string, string>());
  const activeSecretSession = useRef<string | null>(null);
  const keyFor = (signature: string) => {
    const value = keys.current.get(signature) ?? createIdempotencyKey();
    keys.current.set(signature, value); return value;
  };
  const load = useCallback(async () => {
    setLoading(true); setError(null);
    try {
      const nextStatus = await secrets.status();
      if (nextStatus.locked) activeSecretSession.current = null;
      const [nextSecrets, nextChannels, nextPresets, nextTemplates] = await Promise.all([
        nextStatus.initialized ? secrets.list() : Promise.resolve([]),
        config.listChannels(), config.listPresets(), conversations.listRolePlayGraphOptions(),
      ]);
      setStatus(nextStatus); setSecretRefs(nextSecrets); setChannels(nextChannels);
      setPresets(nextPresets); setTemplates(nextTemplates);
    } catch (cause) { setError(localErrorMessage(cause)); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void load(); }, [load]);
  const action = async <T,>(kind: NonNullable<Pending>, run: () => Promise<T>) => {
    setPending(kind); setError(null);
    try { return await run(); }
    catch (cause) { setError(localErrorMessage(cause)); throw cause; }
    finally { setPending(null); }
  };
  const storeSecret = (input: SecretInput) => action("secret", async () => {
    if (!status) return;
    const password = { masterPassword: input.masterPassword, idempotencyKey: input.passwordCommandKey };
    const session = status.initialized ? await secrets.unlock(password) : await secrets.initialize(password);
    activeSecretSession.current = session.sessionId;
    const stored = await secrets.put({
      secretId: input.secretId, name: input.name, kind: "api_key", value: input.value,
      sessionId: session.sessionId, idempotencyKey: input.putCommandKey,
    });
    setStatus({ initialized: true, storeId: session.storeId, formatVersion: session.formatVersion, locked: false });
    setSecretRefs((items) => [...items.filter((item) => item.secretRef.id !== stored.secretRef.id), stored]);
    notifyShellStatusChanged();
  });
  const unlockSecretStore = (masterPassword: string, idempotencyKey: string) => action("secret_control", async () => {
    const session = await secrets.unlock({ masterPassword, idempotencyKey });
    activeSecretSession.current = session.sessionId;
    setStatus({ initialized: true, storeId: session.storeId, formatVersion: session.formatVersion, locked: false });
    notifyShellStatusChanged();
  });
  const lockSecretStore = (idempotencyKey: string) => action("secret_control", async () => {
    await secrets.lock({ expectedSessionId: activeSecretSession.current, idempotencyKey });
    activeSecretSession.current = null;
    setStatus((current) => current ? { ...current, locked: true } : current);
    notifyShellStatusChanged();
  });
  const changeSecretStorePassword = (currentPassword: string, newPassword: string, unlockKey: string, changeKey: string) => action("secret_control", async () => {
    const unlocked = await secrets.unlock({ masterPassword: currentPassword, idempotencyKey: unlockKey });
    const session = await secrets.changePassword({ currentPassword, newPassword, sessionId: unlocked.sessionId, idempotencyKey: changeKey });
    activeSecretSession.current = session.sessionId;
    setStatus({ initialized: true, storeId: session.storeId, formatVersion: session.formatVersion, locked: false });
    notifyShellStatusChanged();
  });
  const publishChannel = (input: ChannelInput) => action("channel", async () => {
    const signature = `channel:${stringifyJsonExact(input)}`;
    let channel = channels.find((item) => item.name === input.name && item.headRevisionId === null);
    channel ??= await config.createChannel(input.name, keyFor(`${signature}:create`));
    await config.publishChannel(channel.id, {
      expectedHeadRevisionId: null, baseUrl: input.baseUrl, providerKind: input.providerKind,
      modelId: input.modelId, credentialSecretId: input.credentialSecretId,
      allowLoopbackHttp: /^http:\/\/(localhost|127\.0\.0\.1)/.test(input.baseUrl),
      allowUnauthenticated: input.credentialSecretId === null,
      structuredOutput: input.structuredOutput,
    }, keyFor(`${signature}:publish`));
    await load();
  });
  const publishPreset = (input: RolePresetInput) => action("preset", async () => {
    const signature = `preset:${stringifyJsonExact(input)}`;
    let preset = presets.find((item) => item.name === input.name && item.headVersionId === null);
    preset ??= await config.createPreset(input.name, keyFor(`${signature}:create`));
    await config.publishPreset(preset.id, {
      expectedHeadVersionId: null, spec: buildRolePresetSpec(input),
    }, keyFor(`${signature}:publish`));
    setPreview(null); await load();
  });
  const createTemplate = (input: { name: string; channelId: string; presetId: string }) => action("template", async () => {
    const result = await config.createRolePlayTemplate(input.name, input.channelId, input.presetId, {
      idempotencyKey: keyFor(`template:${stringifyJsonExact(input)}`),
    });
    setTemplates(await conversations.listRolePlayGraphOptions()); return result;
  });
  const previewPreset = (preset: ContextPresetView) => void action("preview", async () => {
    if (preset.headVersionId) setPreview(await config.previewPreset(preset.id, preset.headVersionId));
  });
  const discoverModels = (channel: ChannelView) => void action("discovery", async () => {
    if (!channel.headRevisionId) return;
    const found = await config.discoverModels(channel.id, { revisionId: channel.headRevisionId });
    setDiscovery(found); setDiscoverySource(await config.getChannelRevision(found.channelRevisionId));
  });
  const publishModel = (model: DiscoveredChannelModel, structured: boolean) => action("model", async () => {
    if (!discovery || !discoverySource) return;
    await config.publishDiscoveredModel(discovery.channelId, discoverySource, discovery, model, structured);
    setDiscovery(null); setDiscoverySource(null); await load();
  });
  const inspect = (template: RolePlayGraphOptionView) => void action("settings", async () => {
    setSettings(await config.getRolePlaySettings(template.revisionId));
  });
  const savePreferences = (value: typeof preferences) => {
    saveApplicationPreferences(value); setPreferences(value);
  };
  return <div className="mx-auto max-w-5xl space-y-6 pb-24"><ApplicationSettings value={preferences} onSave={savePreferences} /><PluginManager client={plugins} secrets={secretRefs} /><SettingsSetup status={status} secrets={secretRefs} channels={channels} presets={presets} templates={templates} preview={preview} discovery={discovery} rolePlaySettings={settings} loading={loading} pending={pending} error={error} onReload={() => void load()} onStoreSecret={storeSecret} onUnlockSecretStore={unlockSecretStore} onLockSecretStore={lockSecretStore} onChangeSecretStorePassword={changeSecretStorePassword} onPublishChannel={publishChannel} onPublishPreset={publishPreset} onPreviewPreset={previewPreset} onCreateTemplate={createTemplate} onDiscoverModels={discoverModels} onPublishDiscoveredModel={publishModel} onInspectTemplate={inspect} /></div>;
}
