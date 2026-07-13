import { useCallback, useEffect, useRef, useState } from "react";

import { createIdempotencyKey, type ChannelModelDiscoveryView, type ChannelRevisionView, type ChannelView, type ContextPresetPreviewView, type ContextPresetView, type DiscoveredChannelModel, type GenerationProviderKind, type RolePlayGraphOptionView, type RolePlaySettingsView, type SecretMetadataView, type SecretStoreStatusView } from "@zhuangsheng/api-client";

import { client, messageFor } from "./api";
import { buildRolePresetSpec, type RolePresetInput } from "./role-preset-spec";

export interface ChannelSetupInput {
  name: string;
  baseUrl: string;
  providerKind: GenerationProviderKind;
  modelId: string;
  credentialSecretId: string | null;
  structuredOutput: boolean;
}

export interface SecretSetupInput {
  secretId: string;
  name: string;
  value: string;
  masterPassword: string;
  passwordCommandKey: string;
  putCommandKey: string;
}

export function useInitialSetup() {
  const [status, setStatus] = useState<SecretStoreStatusView | null>(null);
  const [secrets, setSecrets] = useState<SecretMetadataView[]>([]);
  const [channels, setChannels] = useState<ChannelView[]>([]);
  const [presets, setPresets] = useState<ContextPresetView[]>([]);
  const [templates, setTemplates] = useState<RolePlayGraphOptionView[]>([]);
  const [preview, setPreview] = useState<ContextPresetPreviewView | null>(null);
  const [discovery, setDiscovery] = useState<ChannelModelDiscoveryView | null>(null);
  const [discoverySource, setDiscoverySource] = useState<ChannelRevisionView | null>(null);
  const [rolePlaySettings, setRolePlaySettings] = useState<RolePlaySettingsView | null>(null);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState<"secret" | "secret_control" | "channel" | "preset" | "template" | "preview" | "discovery" | "model" | "settings" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const commandKeys = useRef(new Map<string, string>());
  const activeSecretSession = useRef<string | null>(null);

  const load = useCallback(async (signal?: AbortSignal) => {
    setLoading(true); setError(null); setDiscovery(null); setDiscoverySource(null); setRolePlaySettings(null);
    try {
      const nextStatus = await client.secrets.status(signal);
      if (nextStatus.locked) activeSecretSession.current = null;
      const [nextSecrets, nextChannels, nextPresets, nextTemplates] = await Promise.all([
        nextStatus.initialized ? client.secrets.list(signal) : Promise.resolve([]),
        client.config.listChannels(signal), client.config.listPresets(signal),
        client.listRolePlayGraphOptions(signal),
      ]);
      setStatus(nextStatus); setSecrets(nextSecrets); setChannels(nextChannels); setPresets(nextPresets); setTemplates(nextTemplates);
    } catch (cause) { if (!signal?.aborted) setError(messageFor(cause)); }
    finally { if (!signal?.aborted) setLoading(false); }
  }, []);

  useEffect(() => { const controller = new AbortController(); void load(controller.signal); return () => controller.abort(); }, [load]);
  const keyFor = (signature: string) => {
    const existing = commandKeys.current.get(signature);
    if (existing) return existing;
    const key = createIdempotencyKey(); commandKeys.current.set(signature, key); return key;
  };
  const done = (signature: string) => commandKeys.current.delete(signature);

  const storeSecret = async (input: SecretSetupInput) => {
    if (!status) return;
    setPending("secret"); setError(null);
    try {
      const passwordInput = { masterPassword: input.masterPassword, idempotencyKey: input.passwordCommandKey };
      const session = status.initialized ? await client.secrets.unlock(passwordInput) : await client.secrets.initialize(passwordInput);
      activeSecretSession.current = session.sessionId;
      const secret = await client.secrets.put({ secretId: input.secretId, name: input.name, kind: "api_key", value: input.value, sessionId: session.sessionId, idempotencyKey: input.putCommandKey });
      setStatus({ initialized: true, storeId: session.storeId, formatVersion: session.formatVersion, locked: false });
      setSecrets((items) => [...items.filter((item) => item.secretRef.id !== secret.secretRef.id), secret]);
    } catch (cause) { setError(messageFor(cause)); throw cause; }
    finally { setPending(null); }
  };

  const unlockSecretStore = async (masterPassword: string, idempotencyKey: string) => {
    setPending("secret_control"); setError(null);
    try {
      const session = await client.secrets.unlock({ masterPassword, idempotencyKey });
      activeSecretSession.current = session.sessionId;
      setStatus({ initialized: true, storeId: session.storeId, formatVersion: session.formatVersion, locked: false });
    } catch (cause) { setError(messageFor(cause)); throw cause; }
    finally { setPending(null); }
  };

  const lockSecretStore = async (idempotencyKey: string) => {
    setPending("secret_control"); setError(null);
    try {
      await client.secrets.lock({ expectedSessionId: activeSecretSession.current, idempotencyKey });
      activeSecretSession.current = null;
      setStatus((current) => current ? { ...current, locked: true } : current);
    } catch (cause) { setError(messageFor(cause)); throw cause; }
    finally { setPending(null); }
  };

  const changeSecretStorePassword = async (currentPassword: string, newPassword: string, unlockKey: string, changeKey: string) => {
    setPending("secret_control"); setError(null);
    try {
      const unlocked = await client.secrets.unlock({ masterPassword: currentPassword, idempotencyKey: unlockKey });
      const session = await client.secrets.changePassword({ currentPassword, newPassword, sessionId: unlocked.sessionId, idempotencyKey: changeKey });
      activeSecretSession.current = session.sessionId;
      setStatus({ initialized: true, storeId: session.storeId, formatVersion: session.formatVersion, locked: false });
    } catch (cause) { setError(messageFor(cause)); throw cause; }
    finally { setPending(null); }
  };

  const publishChannel = async (input: ChannelSetupInput) => {
    const signature = `channel:${JSON.stringify(input)}`;
    setPending("channel"); setError(null);
    try {
      let channel = channels.find((item) => item.name === input.name && item.headRevisionId === null);
      if (!channel) {
        channel = await client.config.createChannel(input.name, keyFor(`${signature}:create`));
        setChannels((items) => [...items, channel as ChannelView]);
      }
      const revision = await client.config.publishChannel(channel.id, {
        expectedHeadRevisionId: null, baseUrl: input.baseUrl, providerKind: input.providerKind, modelId: input.modelId,
        credentialSecretId: input.credentialSecretId, allowLoopbackHttp: input.baseUrl.startsWith("http://127.0.0.1") || input.baseUrl.startsWith("http://localhost"),
        allowUnauthenticated: input.credentialSecretId === null,
        structuredOutput: input.structuredOutput,
      }, keyFor(`${signature}:publish`));
      done(`${signature}:create`); done(`${signature}:publish`);
      setChannels((items) => items.map((item) => item.id === channel?.id ? { ...item, headRevisionId: revision.id, updatedAt: revision.createdAt } : item));
    } catch (cause) { setError(messageFor(cause)); throw cause; }
    finally { setPending(null); }
  };

  const publishRolePreset = async (input: RolePresetInput) => {
    const signature = `preset:${JSON.stringify(input)}`;
    setPending("preset"); setError(null);
    try {
      let preset = presets.find((item) => item.name === input.name && item.headVersionId === null);
      if (!preset) {
        preset = await client.config.createPreset(input.name, keyFor(`${signature}:create`));
        setPresets((items) => [...items, preset as ContextPresetView]);
      }
      const version = await client.config.publishPreset(preset.id, { expectedHeadVersionId: null, spec: buildRolePresetSpec(input) }, keyFor(`${signature}:publish`));
      setPreview(null);
      done(`${signature}:create`); done(`${signature}:publish`);
      setPresets((items) => items.map((item) => item.id === preset?.id ? { ...item, headVersionId: version.id, updatedAt: version.createdAt } : item));
    } catch (cause) { setError(messageFor(cause)); throw cause; }
    finally { setPending(null); }
  };

  const createTemplate = async (input: { name: string; channelId: string; presetId: string }) => {
    const signature = `template:${JSON.stringify(input)}`;
    setPending("template"); setError(null);
    try {
      const revision = await client.graphs.createRolePlayTemplate(input.name, input.channelId, input.presetId, { idempotencyKey: keyFor(signature) });
      done(signature);
      const options = await client.listRolePlayGraphOptions();
      setTemplates(options);
      return revision;
    } catch (cause) { setError(messageFor(cause)); throw cause; }
    finally { setPending(null); }
  };

  const previewPreset = async (preset: ContextPresetView) => {
    if (!preset.headVersionId) return;
    setPending("preview"); setError(null);
    try { setPreview(await client.config.previewPreset(preset.id, preset.headVersionId)); }
    catch (cause) { setError(messageFor(cause)); }
    finally { setPending(null); }
  };

  const discoverModels = async (channel: ChannelView) => {
    if (!channel.headRevisionId) return;
    setPending("discovery"); setError(null);
    try {
      const found = await client.config.discoverModels(channel.id, {
        revisionId: channel.headRevisionId,
      });
      const source = await client.config.getChannelRevision(found.channelRevisionId);
      setDiscovery(found); setDiscoverySource(source);
    } catch (cause) { setError(messageFor(cause)); }
    finally { setPending(null); }
  };

  const publishDiscoveredModel = async (
    model: DiscoveredChannelModel,
    structuredOutput: boolean,
  ) => {
    if (!discovery || !discoverySource) return;
    const signature = `channel-model:${discovery.channelRevisionId}:${model.id}`;
    setPending("model"); setError(null);
    try {
      const revision = await client.config.publishDiscoveredModel(
        discovery.channelId,
        discoverySource,
        discovery,
        model,
        structuredOutput,
        keyFor(signature),
      );
      done(signature); setDiscovery(null); setDiscoverySource(null);
      setChannels((items) => items.map((item) => item.id === revision.channelId
        ? { ...item, headRevisionId: revision.id, updatedAt: revision.createdAt }
        : item));
    } catch (cause) { setError(messageFor(cause)); throw cause; }
    finally { setPending(null); }
  };

  const inspectTemplate = async (template: RolePlayGraphOptionView) => {
    if (!template.primaryLlmNodeId) return;
    setPending("settings"); setError(null);
    try { setRolePlaySettings(await client.graphs.getRolePlaySettings(template.revisionId)); }
    catch (cause) { setError(messageFor(cause)); }
    finally { setPending(null); }
  };

  return { status, secrets, channels, presets, templates, preview, discovery, rolePlaySettings, loading, pending, error, reload: () => void load(), storeSecret, unlockSecretStore, lockSecretStore, changeSecretStorePassword, publishChannel, publishRolePreset, previewPreset: (preset: ContextPresetView) => void previewPreset(preset), createTemplate, discoverModels: (channel: ChannelView) => void discoverModels(channel), publishDiscoveredModel, inspectTemplate: (template: RolePlayGraphOptionView) => void inspectTemplate(template) };
}
