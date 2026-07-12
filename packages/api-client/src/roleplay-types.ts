import type { JsonObject } from "./graph-types";
import type { RolePlayCompatibilityView } from "./types";

export interface RolePlaySettingsView {
  profileVersion: 1;
  revisionId: string;
  primaryLlmNodeId: string;
  compatibility: RolePlayCompatibilityView;
  model: {
    channelId: string;
    modelId: string;
    modelName: string | null;
    operationKey: JsonObject;
  };
  generation: JsonObject | null;
  streaming: {
    enabled: boolean;
    audience: "user" | "trace" | "both" | "internal";
    persistChunks: boolean;
  } | null;
  contextPresetId: string | null;
}
