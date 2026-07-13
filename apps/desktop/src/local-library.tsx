import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  ContextPresetVersionView,
  ContextPresetView,
  ChannelView,
  ArtifactView,
  RolePlayGraphOptionView,
} from "@zhuangsheng/api-client";
import { LibraryPage } from "@zhuangsheng/domain-ui";
import { createSillyTavernWorkflow } from "@zhuangsheng/sillytavern-compat";

import { artifacts, config, conversations, localErrorMessage } from "./bridge";

export function LocalLibrary({ onOpenSettings, onOpenArtifacts }: {
  onOpenSettings: () => void;
  onOpenArtifacts: () => void;
}) {
  const [presets, setPresets] = useState<ContextPresetView[]>([]);
  const [channels, setChannels] = useState<ChannelView[]>([]);
  const [versions, setVersions] = useState<Record<string, ContextPresetVersionView>>({});
  const [templates, setTemplates] = useState<RolePlayGraphOptionView[]>([]);
  const [artifactItems, setArtifactItems] = useState<ArtifactView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const resources = useRef({ presets, versions });
  resources.current = { presets, versions };
  const sillyTavern = useMemo(() => createSillyTavernWorkflow({
    presets: () => resources.current.presets,
    versions: () => resources.current.versions,
    createPreset: (name, key) => config.createPreset(name, key),
    publishPreset: (presetId, input, key) => config.publishPreset(presetId, input, key),
    createRolePlayTemplate: (name, channelId, presetId, options) =>
      config.createRolePlayTemplate(name, channelId, presetId, options),
  }), []);
  const reload = useCallback(async () => {
    setLoading(true); setError(null);
    try {
      const [nextPresets, nextChannels, nextTemplates, nextArtifacts] = await Promise.all([
        config.listPresets(), config.listChannels(), conversations.listRolePlayGraphOptions(), artifacts.list(),
      ]);
      const heads = await Promise.all(nextPresets.flatMap((preset) =>
        preset.headVersionId ? [config.getPresetVersion(preset.headVersionId)] : []));
      setPresets(nextPresets); setChannels(nextChannels); setTemplates(nextTemplates);
      setArtifactItems(nextArtifacts.items);
      setVersions(Object.fromEntries(heads.map((version) => [version.id, version])));
    } catch (cause) { setError(localErrorMessage(cause)); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void reload(); }, [reload]);
  return <LibraryPage presets={presets} channels={channels} versions={versions} templates={templates} artifacts={artifactItems} loading={loading} error={error} onReload={() => void reload()} onOpenSettings={onOpenSettings} onOpenArtifacts={onOpenArtifacts} contentUrl={() => "#"} onDownloadArtifact={(id) => artifacts.downloadToBrowser(id)} sillyTavern={sillyTavern} />;
}
