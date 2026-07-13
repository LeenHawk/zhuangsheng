import { createContext, useContext, type ReactNode } from "react";

import type { PlatformCapabilities } from "@zhuangsheng/api-client";

const PlatformContext = createContext<PlatformCapabilities | null>(null);

export function PlatformCapabilitiesProvider({
  value,
  children,
}: {
  value: PlatformCapabilities;
  children: ReactNode;
}) {
  return <PlatformContext.Provider value={value}>{children}</PlatformContext.Provider>;
}

export function usePlatformCapabilities(): PlatformCapabilities {
  const capabilities = useContext(PlatformContext);
  if (!capabilities) throw new Error("PlatformCapabilitiesProvider is missing");
  return capabilities;
}
