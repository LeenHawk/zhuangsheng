import { invoke as tauriInvoke } from "@tauri-apps/api/core";
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
  parseJsonExact,
  stringifyJsonExact,
  type TauriBridge,
  type PlatformCapabilities,
} from "@zhuangsheng/api-client";

const exactJsonOperations = new Set([
  "start_run", "get_run_outputs", "list_open_waits", "list_run_events",
  "satisfy_wait", "resolve_effect_unknown", "fork_context", "merge_context",
  "get_graph_draft", "update_graph_draft", "apply_graph", "get_graph_revision",
  "get_graph_revision_for_graph", "create_roleplay_template", "get_roleplay_settings",
  "publish_channel_revision", "get_channel_revision", "get_channel_head_revision",
  "discover_channel_models", "publish_context_preset_version", "get_context_preset_version",
  "get_context_preset_head", "preview_context_preset", "commit_context_patch",
  "preview_sillytavern_import", "test_sillytavern_regex", "apply_sillytavern_import",
  "get_working_context", "get_context_at_commit", "diff_context_commits",
  "list_memory_proposals", "propose_memory_change", "decide_memory_proposal",
  "apply_memory_proposal", "get_memory_record", "search_memory", "list_tool_descriptors",
]);

export const bridge: TauriBridge = {
  invoke: async <T,>(operation: string, payload: unknown): Promise<T> => {
    if (!exactJsonOperations.has(operation)) {
      return tauriInvoke(operation, payload as Record<string, unknown>);
    }
    const bytes = await tauriInvoke<number[]>("invoke_exact_json", {
      operation,
      payloadJson: stringifyJsonExact(payload),
    });
    const envelope = parseJsonExact(
      new TextDecoder().decode(Uint8Array.from(bytes)),
    ) as { ok?: unknown; value?: unknown; error?: unknown };
    if (envelope.ok === true) return envelope.value as T;
    throw envelope.error ?? new Error("本地 exact JSON adapter 返回了无效响应。");
  },
  listen: async (event, handler) => {
    const unlisten = await listen(event, handler);
    return unlisten;
  },
};

export const desktopPlatformCapabilities: PlatformCapabilities = {
  platform: "desktop",
  localFirst: true,
  filePicker: true,
  nativeNotifications: false,
  openExternal: async (url) => { window.open(url, "_blank", "noopener,noreferrer"); },
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
