import { StrictMode, useCallback, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { TauriTransport, type RunListView, type RunView } from "@zhuangsheng/api-client";
import { AppShell, RunList } from "@zhuangsheng/domain-ui";

import "../../web/src/styles.css";

const transport = new TauriTransport({
  invoke: (operation, payload) => invoke(operation, payload as Record<string, unknown>),
  listen: async (event, handler) => {
    const unlisten = await listen(event, handler);
    return unlisten;
  },
});

function DesktopApp() {
  const [runs, setRuns] = useState<RunView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    setLoading(true); setError(null);
    try {
      const result = await transport.query<RunListView>({ operation: "list_recent_runs", payload: { limit: 50 } });
      setRuns(result.items);
    } catch (cause) { setError(cause instanceof Error ? cause.message : "本地 adapter 不可用"); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void reload(); }, [reload]);
  return <AppShell mode="expert" section="runs" onModeChange={() => undefined} onSectionChange={() => undefined}><RunList runs={runs} loading={loading} error={error} onReload={() => void reload()} onOpen={() => undefined} /></AppShell>;
}

const root = document.getElementById("root");
if (!root) throw new Error("Application root is missing");
createRoot(root).render(<StrictMode><DesktopApp /></StrictMode>);
