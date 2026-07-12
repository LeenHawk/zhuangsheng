import { useCallback, useEffect, useState } from "react";

import type {
  ContextPresetVersionView,
  ContextPresetView,
  RolePlayGraphOptionView,
} from "@zhuangsheng/api-client";
import { LibraryPage } from "@zhuangsheng/domain-ui";

import { config, conversations, localErrorMessage } from "./bridge";

export function LocalLibrary({ onOpenSettings, onOpenArtifacts }: {
  onOpenSettings: () => void;
  onOpenArtifacts: () => void;
}) {
  const [presets, setPresets] = useState<ContextPresetView[]>([]);
  const [versions, setVersions] = useState<Record<string, ContextPresetVersionView>>({});
  const [templates, setTemplates] = useState<RolePlayGraphOptionView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    setLoading(true); setError(null);
    try {
      const [nextPresets, nextTemplates] = await Promise.all([
        config.listPresets(), conversations.listRolePlayGraphOptions(),
      ]);
      const heads = await Promise.all(nextPresets.flatMap((preset) =>
        preset.headVersionId ? [config.getPresetVersion(preset.headVersionId)] : []));
      setPresets(nextPresets); setTemplates(nextTemplates);
      setVersions(Object.fromEntries(heads.map((version) => [version.id, version])));
    } catch (cause) { setError(localErrorMessage(cause)); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void reload(); }, [reload]);
  return <LibraryPage presets={presets} versions={versions} templates={templates} artifacts={[]} loading={loading} error={error} onReload={() => void reload()} onOpenSettings={onOpenSettings} onOpenArtifacts={onOpenArtifacts} contentUrl={() => "#"} />;
}
