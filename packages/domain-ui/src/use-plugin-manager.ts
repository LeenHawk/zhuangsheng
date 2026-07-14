import { useCallback, useEffect, useState } from "react";

import type {
  InspectGitPluginInput,
  PluginCandidateView,
  PluginClient,
  PluginInstallationView,
  PluginUpdatePolicy,
} from "@zhuangsheng/api-client";
import { notifyPluginsChanged } from "@zhuangsheng/ui-extension-host";

export function usePluginManager(client: PluginClient) {
  const [installations, setInstallations] = useState<PluginInstallationView[]>([]);
  const [candidate, setCandidate] = useState<PluginCandidateView | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setLoading(true); setError(null);
    try { setInstallations(await client.list()); }
    catch (cause) { setError(message(cause)); }
    finally { setLoading(false); }
  }, [client]);
  useEffect(() => { void reload(); }, [reload]);

  const action = async <T,>(key: string, run: () => Promise<T>): Promise<T | undefined> => {
    setBusy(key); setError(null); setNotice(null);
    try { return await run(); }
    catch (cause) { setError(message(cause)); return undefined; }
    finally { setBusy(null); }
  };

  const inspect = (input: InspectGitPluginInput) => action("inspect", async () => {
    const value = await client.inspect(input); setCandidate(value); return value;
  });

  const activate = (policy: PluginUpdatePolicy) => candidate && action("activate", async () => {
    await client.activate(candidate, policy); setCandidate(null); await reload();
    notifyPluginsChanged(); setNotice("插件版本已激活。");
  });

  const configure = (item: PluginInstallationView, enabled: boolean, policy: PluginUpdatePolicy) =>
    action(`configure:${item.pluginId}`, async () => {
      const updated = await client.configure(item.pluginId, enabled, policy);
      setInstallations((items) => items.map((current) => current.pluginId === updated.pluginId ? updated : current));
      notifyPluginsChanged(); setNotice("插件运行策略已更新。");
    });

  const checkUpdate = (pluginId: string) => action(`update:${pluginId}`, async () => {
    const value = await client.checkUpdate(pluginId); setCandidate(value);
    setNotice(value ? "发现候选版本，请检查权限后确认。" : "当前已经是该 Git ref 的最新 commit。");
  });

  const rollback = (item: PluginInstallationView, targetVersionId: string) =>
    action(`rollback:${item.pluginId}`, async () => {
      const updated = await client.rollback(item.pluginId, targetVersionId, item.activeVersion.id);
      setInstallations((items) => items.map((current) => current.pluginId === updated.pluginId ? updated : current));
      notifyPluginsChanged(); setNotice("已原子切换到所选历史版本。");
    });

  return { installations, candidate, loading, busy, error, notice, reload, inspect, activate, configure, checkUpdate, rollback, clearCandidate: () => setCandidate(null) };
}

const message = (cause: unknown) => cause instanceof Error ? cause.message : "插件操作失败。";
