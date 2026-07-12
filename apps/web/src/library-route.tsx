import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";

import type { ArtifactView, ContextPresetVersionView, ContextPresetView, RolePlayGraphOptionView } from "@zhuangsheng/api-client";
import { LibraryPage } from "@zhuangsheng/domain-ui";

import { client, messageFor } from "./api";

export function LibraryRoute() {
  const navigate = useNavigate();
  const [presets, setPresets] = useState<ContextPresetView[]>([]);
  const [versions, setVersions] = useState<Record<string, ContextPresetVersionView>>({});
  const [templates, setTemplates] = useState<RolePlayGraphOptionView[]>([]);
  const [artifacts, setArtifacts] = useState<ArtifactView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async (signal?: AbortSignal) => {
    setLoading(true); setError(null);
    try {
      const [nextPresets, nextTemplates, nextArtifacts] = await Promise.all([
        client.config.listPresets(signal), client.listRolePlayGraphOptions(signal), client.artifacts.list(50, signal),
      ]);
      const heads = await Promise.all(nextPresets.flatMap((preset) => preset.headVersionId ? [client.config.getPresetVersion(preset.headVersionId, signal)] : []));
      setPresets(nextPresets); setTemplates(nextTemplates); setArtifacts(nextArtifacts.items);
      setVersions(Object.fromEntries(heads.map((version) => [version.id, version])));
    } catch (cause) { if (!signal?.aborted) setError(messageFor(cause)); }
    finally { if (!signal?.aborted) setLoading(false); }
  }, []);
  useEffect(() => { const controller = new AbortController(); void reload(controller.signal); return () => controller.abort(); }, [reload]);
  return <LibraryPage presets={presets} versions={versions} templates={templates} artifacts={artifacts} loading={loading} error={error} onReload={() => void reload()} onOpenSettings={() => navigate("/settings")} onOpenArtifacts={() => navigate("/expert/artifacts")} contentUrl={(id) => client.artifacts.contentUrl(id)} />;
}
