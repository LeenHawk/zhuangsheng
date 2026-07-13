import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import type { ArtifactView, ChannelView, ContextPresetVersionView, ContextPresetView, RolePlayGraphOptionView } from "@zhuangsheng/api-client";
import { LibraryPage } from "@zhuangsheng/domain-ui";
import { createSillyTavernWorkflow } from "@zhuangsheng/sillytavern-compat";

import { client, messageFor } from "./api";

export function LibraryRoute() {
  const navigate = useNavigate();
  const [presets, setPresets] = useState<ContextPresetView[]>([]);
  const [channels, setChannels] = useState<ChannelView[]>([]);
  const [versions, setVersions] = useState<Record<string, ContextPresetVersionView>>({});
  const [templates, setTemplates] = useState<RolePlayGraphOptionView[]>([]);
  const [artifacts, setArtifacts] = useState<ArtifactView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const resources = useRef({ presets, versions });
  resources.current = { presets, versions };
  const sillyTavern = useMemo(() => createSillyTavernWorkflow({
    presets: () => resources.current.presets,
    versions: () => resources.current.versions,
    createPreset: (name, key) => client.config.createPreset(name, key),
    publishPreset: (presetId, input, key) => client.config.publishPreset(presetId, input, key),
    createRolePlayTemplate: (name, channelId, presetId, options) =>
      client.graphs.createRolePlayTemplate(name, channelId, presetId, options),
  }), []);
  const reload = useCallback(async (signal?: AbortSignal) => {
    setLoading(true); setError(null);
    try {
      const [nextPresets, nextChannels, nextTemplates, nextArtifacts] = await Promise.all([
        client.config.listPresets(signal), client.config.listChannels(signal), client.listRolePlayGraphOptions(signal), client.artifacts.list(50, signal),
      ]);
      const heads = await Promise.all(nextPresets.flatMap((preset) => preset.headVersionId ? [client.config.getPresetVersion(preset.headVersionId, signal)] : []));
      setPresets(nextPresets); setChannels(nextChannels); setTemplates(nextTemplates); setArtifacts(nextArtifacts.items);
      setVersions(Object.fromEntries(heads.map((version) => [version.id, version])));
    } catch (cause) { if (!signal?.aborted) setError(messageFor(cause)); }
    finally { if (!signal?.aborted) setLoading(false); }
  }, []);
  useEffect(() => { const controller = new AbortController(); void reload(controller.signal); return () => controller.abort(); }, [reload]);
  return <LibraryPage presets={presets} channels={channels} versions={versions} templates={templates} artifacts={artifacts} loading={loading} error={error} onReload={() => void reload()} onOpenSettings={() => navigate("/settings")} onOpenArtifacts={() => navigate("/expert/artifacts")} contentUrl={(id) => client.artifacts.contentUrl(id)} sillyTavern={sillyTavern} />;
}
