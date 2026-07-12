import { useCallback, useEffect, useRef, useState } from "react";

import { createIdempotencyKey, type ChannelView, type ContextPresetView, type GenerationProviderKind, type JsonObject, type RolePlayGraphOptionView, type SecretMetadataView, type SecretStoreStatusView } from "@zhuangsheng/api-client";

import { client, messageFor } from "./api";

export interface ChannelSetupInput {
  name: string;
  baseUrl: string;
  providerKind: GenerationProviderKind;
  modelId: string;
  credentialSecretId: string | null;
  structuredOutput: boolean;
}

export interface RolePresetInput {
  name: string;
  characterName: string;
  identity: string;
  personality: string;
  speakingStyle: string;
  boundaries: string;
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
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState<"secret" | "channel" | "preset" | "template" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const commandKeys = useRef(new Map<string, string>());

  const load = useCallback(async (signal?: AbortSignal) => {
    setLoading(true); setError(null);
    try {
      const nextStatus = await client.secrets.status(signal);
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
      const secret = await client.secrets.put({ secretId: input.secretId, name: input.name, kind: "api_key", value: input.value, sessionId: session.sessionId, idempotencyKey: input.putCommandKey });
      setStatus({ initialized: true, storeId: session.storeId, formatVersion: session.formatVersion, locked: false });
      setSecrets((items) => [...items.filter((item) => item.secretRef.id !== secret.secretRef.id), secret]);
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

  return { status, secrets, channels, presets, templates, loading, pending, error, reload: () => void load(), storeSecret, publishChannel, publishRolePreset, createTemplate };
}

export function buildRolePresetSpec(input: RolePresetInput): JsonObject {
  const sections: Array<[string, string]> = [["角色", input.characterName], ["身份", input.identity], ["性格与目标", input.personality], ["说话风格", input.speakingStyle], ["内容边界", input.boundaries]];
  const text = sections.filter(([, value]) => value.trim()).map(([label, value]) => `${label}：${value.trim()}`).join("\n");
  return {
    mode: "chat",
    items: [
      { id: "character", name: input.characterName, enabled: true, requestedRole: "system", source: { type: "literal", text }, position: { type: "start" }, order: 0, priority: 100, insertionDepth: 0, budget: { required: true }, overflow: null },
      { id: "history", name: "Conversation history", enabled: true, requestedRole: "context", source: { type: "history", bindingId: "history", strategy: { type: "all" } }, position: { type: "history" }, order: 0, priority: 90, insertionDepth: 0, budget: { required: false }, overflow: { type: "keep_recent", count: null } },
    ],
    budget: null,
    postProcess: [],
    preview: { content: "metadata_only", count: "local" },
  };
}
