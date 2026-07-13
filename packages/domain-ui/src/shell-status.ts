import { useCallback, useEffect, useState } from "react";

import type { SecretStoreStatusView } from "@zhuangsheng/api-client";

import type { AppShellStatus } from "./app-shell";

const refreshEvent = "zhuangsheng:shell-status-refresh";

export function notifyShellStatusChanged() {
  window.dispatchEvent(new Event(refreshEvent));
}

export function useAppShellStatus(
  loadSecretStore: () => Promise<SecretStoreStatusView>,
  localFirst: boolean,
): AppShellStatus {
  const [secretStore, setSecretStore] = useState<SecretStoreStatusView | null>(null);
  const [connection, setConnection] = useState<AppShellStatus["connection"]>(
    localFirst ? "unknown" : navigator.onLine ? "unknown" : "offline",
  );
  const refresh = useCallback(async () => {
    if (!localFirst && !navigator.onLine) {
      setConnection("offline");
      return;
    }
    try {
      setSecretStore(await loadSecretStore());
      setConnection("online");
    } catch {
      setConnection(localFirst ? "unknown" : "offline");
    }
  }, [loadSecretStore, localFirst]);

  useEffect(() => {
    const refreshStatus = () => void refresh();
    const markOffline = () => { if (!localFirst) setConnection("offline"); };
    void refresh();
    window.addEventListener("online", refreshStatus);
    window.addEventListener("offline", markOffline);
    window.addEventListener("focus", refreshStatus);
    window.addEventListener(refreshEvent, refreshStatus);
    const interval = window.setInterval(refreshStatus, 30_000);
    return () => {
      window.removeEventListener("online", refreshStatus);
      window.removeEventListener("offline", markOffline);
      window.removeEventListener("focus", refreshStatus);
      window.removeEventListener(refreshEvent, refreshStatus);
      window.clearInterval(interval);
    };
  }, [localFirst, refresh]);

  return { connection, secretStore };
}
