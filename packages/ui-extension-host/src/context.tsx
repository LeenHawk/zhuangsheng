import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import type { PluginClient, PluginInstallationView } from "@zhuangsheng/api-client";

import { loadPluginRendererPreference, PLUGIN_PREFERENCE_EVENT } from "./preference";
import { UiExtensionSandbox } from "./sandbox";
import type { AvailablePluginRenderer, PluginRenderRequest, UiNode } from "./types";

interface LoadedRenderer extends AvailablePluginRenderer {
  roles: Array<"user" | "assistant">;
  sandbox: UiExtensionSandbox;
}

interface PluginHostValue {
  available: AvailablePluginRenderer[];
  loading: boolean;
  error: string | null;
  render: (request: Omit<PluginRenderRequest, "rendererId" | "mode" | "platform">) => Promise<UiNode[] | null>;
}

const PluginHostContext = createContext<PluginHostValue>({
  available: [], loading: false, error: null, render: async () => null,
});

export function PluginHostProvider({
  client, mode, platform, children,
}: {
  client: PluginClient;
  mode: "user" | "expert";
  platform: "web" | "desktop" | "mobile";
  children: ReactNode;
}) {
  const loaded = useRef<LoadedRenderer[]>([]);
  const generation = useRef(0);
  const [available, setAvailable] = useState<AvailablePluginRenderer[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [preference, setPreference] = useState(loadPluginRendererPreference);

  useEffect(() => {
    let disposed = false;
    const reload = async () => {
      const currentGeneration = ++generation.current;
      setLoading(true); setError(null);
      try {
        const installations = await client.list();
        const next = await loadRenderers(client, installations.filter((item) => item.enabled));
        if (disposed || currentGeneration !== generation.current) {
          dispose(next); return;
        }
        dispose(loaded.current); loaded.current = next;
        setAvailable(next.map(({ sandbox: _sandbox, roles: _roles, ...item }) => item));
      } catch (cause) {
        if (!disposed && currentGeneration === generation.current) {
          setError(cause instanceof Error ? cause.message : "无法加载 UI 插件");
        }
      } finally {
        if (!disposed && currentGeneration === generation.current) setLoading(false);
      }
    };
    const onPreference = (event: Event) => setPreference((event as CustomEvent<string | null>).detail);
    const onPluginsChanged = () => { void reload(); };
    window.addEventListener(PLUGIN_PREFERENCE_EVENT, onPreference);
    window.addEventListener("zhuangsheng:plugins-changed", onPluginsChanged);
    void reload();
    return () => {
      disposed = true; generation.current += 1;
      window.removeEventListener(PLUGIN_PREFERENCE_EVENT, onPreference);
      window.removeEventListener("zhuangsheng:plugins-changed", onPluginsChanged);
      dispose(loaded.current); loaded.current = [];
    };
  }, [client]);

  const render = useCallback<PluginHostValue["render"]>(async (request) => {
    if (preference === "native") return null;
    const compatible = loaded.current.filter((item) =>
      item.slot === request.slot && (item.roles.length === 0 || item.roles.includes(request.message.role)));
    const renderer = preference === null
      ? compatible[0]
      : compatible.find((item) => item.key === preference);
    if (!renderer) return null;
    try {
      return await renderer.sandbox.render({ ...request, rendererId: renderer.rendererId, mode, platform });
    } catch {
      return null;
    }
  }, [mode, platform, preference]);

  const value = useMemo(() => ({ available, loading, error, render }), [available, error, loading, render]);
  return <PluginHostContext.Provider value={value}>{children}</PluginHostContext.Provider>;
}

export const usePluginHost = (): PluginHostValue => useContext(PluginHostContext);

async function loadRenderers(client: PluginClient, installations: PluginInstallationView[]): Promise<LoadedRenderer[]> {
  const results = await Promise.allSettled(installations.map(async (installation) => {
    const entrypoint = await client.getEntrypoint(installation.pluginId);
    if (entrypoint.pluginId !== installation.pluginId || entrypoint.versionId !== installation.activeVersion.id) {
      throw new Error("plugin entrypoint identity mismatch");
    }
    const sandbox = await UiExtensionSandbox.create(entrypoint.code);
    return installation.activeVersion.manifest.renderers.map((renderer): LoadedRenderer => ({
      key: `${installation.pluginId}:${renderer.id}`,
      pluginId: installation.pluginId, pluginName: installation.activeVersion.manifest.name,
      rendererId: renderer.id, slot: renderer.slot, priority: renderer.priority,
      roles: renderer.roles, sandbox,
    }));
  }));
  const loaded = results.flatMap((result) => result.status === "fulfilled" ? result.value : []);
  return loaded.sort((left, right) => right.priority - left.priority || left.key.localeCompare(right.key));
}

function dispose(renderers: LoadedRenderer[]): void {
  for (const sandbox of new Set(renderers.map((item) => item.sandbox))) sandbox.dispose();
}
