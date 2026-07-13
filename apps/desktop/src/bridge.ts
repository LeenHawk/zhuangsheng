import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import {
  TauriConversationClient,
  TauriArtifactClient,
  TauriConfigClient,
  TauriContextClient,
  TauriGraphClient,
  TauriRuntimeClient,
  TauriMemoryClient,
  TauriSecretClient,
  TauriToolClient,
  TauriTransport,
  type TauriBridge,
} from "@zhuangsheng/api-client";

export const bridge: TauriBridge = {
  invoke: (operation, payload) => invoke(operation, payload as Record<string, unknown>),
  listen: async (event, handler) => {
    const unlisten = await listen(event, handler);
    return unlisten;
  },
};

export const transport = new TauriTransport(bridge);
export const conversations = new TauriConversationClient(bridge);
export const artifacts = new TauriArtifactClient(bridge);
export const config = new TauriConfigClient(bridge);
export const contexts = new TauriContextClient(bridge);
export const graphs = new TauriGraphClient(bridge);
export const runtime = new TauriRuntimeClient(bridge);
export const memory = new TauriMemoryClient(bridge);
export const secrets = new TauriSecretClient(bridge);
export const tools = new TauriToolClient(bridge);

export const localErrorMessage = (cause: unknown) => {
  if (cause && typeof cause === "object") {
    const error = cause as { code?: unknown; message?: unknown };
    if (typeof error.message === "string") {
      return typeof error.code === "string" ? `${error.message}（${error.code}）` : error.message;
    }
  }
  return cause instanceof Error ? cause.message : "本地 adapter 无法完成请求。";
};
