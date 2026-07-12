import { DecodeError } from "./decode-error";
import { boolean, jsonValue, nullableString, number, record, string, stringArray } from "./decode-helpers";
import type { JsonObject } from "./graph-types";
import type { RolePlaySettingsView } from "./roleplay-types";
import type { RolePlayCompatibilityView, RolePlayGraphOptionView } from "./types";

export const decodeRolePlayCompatibility = (
  value: unknown,
  path = "rolePlayCompatibility",
): RolePlayCompatibilityView => {
  const item = record(value, path);
  const mode = string(item.mode, `${path}.mode`);
  if (mode === "expert_only") {
    return { mode, reasons: stringArray(item.reasons, `${path}.reasons`) };
  }
  if (mode !== "editable" && mode !== "partial") {
    throw new DecodeError(`${path}.mode`);
  }
  const profileVersion = number(item.profileVersion, `${path}.profileVersion`);
  if (profileVersion !== 1) throw new DecodeError(`${path}.profileVersion`);
  const editableFields = stringArray(item.editableFields, `${path}.editableFields`);
  if (mode === "editable") return { mode, profileVersion, editableFields };
  return {
    mode,
    profileVersion,
    editableFields,
    lockedReasons: stringArray(item.lockedReasons, `${path}.lockedReasons`),
  };
};

export const decodeRolePlayGraphOptions = (value: unknown): RolePlayGraphOptionView[] => {
  if (!Array.isArray(value)) throw new DecodeError("rolePlayGraphOptions");
  return value.map((raw, index) => {
    const path = `rolePlayGraphOptions[${index}]`;
    const item = record(raw, path);
    return {
      graphId: string(item.graphId, `${path}.graphId`),
      graphName: string(item.graphName, `${path}.graphName`),
      revisionId: string(item.revisionId, `${path}.revisionId`),
      revisionNo: number(item.revisionNo, `${path}.revisionNo`),
      replyOutputKeys: stringArray(item.replyOutputKeys, `${path}.replyOutputKeys`),
      primaryLlmNodeId: nullableString(item.primaryLlmNodeId, `${path}.primaryLlmNodeId`),
      compatibility: decodeRolePlayCompatibility(item.compatibility, `${path}.compatibility`),
    };
  });
};

export const decodeRolePlaySettings = (value: unknown): RolePlaySettingsView => {
  const path = "rolePlaySettings";
  const item = record(value, path);
  const profileVersion = number(item.profileVersion, `${path}.profileVersion`);
  if (profileVersion !== 1) throw new DecodeError(`${path}.profileVersion`);
  const model = record(item.model, `${path}.model`);
  const operationKey = jsonValue(model.operationKey, `${path}.model.operationKey`);
  if (operationKey === null || Array.isArray(operationKey) || typeof operationKey !== "object") {
    throw new DecodeError(`${path}.model.operationKey`);
  }
  const rawGeneration = item.generation;
  const generation = rawGeneration === null
    ? null
    : jsonObject(rawGeneration, `${path}.generation`);
  const rawStreaming = item.streaming;
  return {
    profileVersion,
    revisionId: string(item.revisionId, `${path}.revisionId`),
    primaryLlmNodeId: string(item.primaryLlmNodeId, `${path}.primaryLlmNodeId`),
    compatibility: decodeRolePlayCompatibility(item.compatibility, `${path}.compatibility`),
    model: {
      channelId: string(model.channelId, `${path}.model.channelId`),
      modelId: string(model.modelId, `${path}.model.modelId`),
      modelName: nullableString(model.modelName, `${path}.model.modelName`),
      operationKey,
    },
    generation,
    streaming: rawStreaming === null ? null : decodeStreaming(rawStreaming, `${path}.streaming`),
    contextPresetId: nullableString(item.contextPresetId, `${path}.contextPresetId`),
  };
};

const jsonObject = (value: unknown, path: string): JsonObject => {
  const decoded = jsonValue(value, path);
  if (decoded === null || Array.isArray(decoded) || typeof decoded !== "object") {
    throw new DecodeError(path);
  }
  return decoded;
};

const decodeStreaming = (
  value: unknown,
  path: string,
): NonNullable<RolePlaySettingsView["streaming"]> => {
  const item = record(value, path);
  const audience = string(item.audience, `${path}.audience`);
  if (!(["user", "trace", "both", "internal"] as const).includes(
    audience as "user" | "trace" | "both" | "internal",
  )) throw new DecodeError(`${path}.audience`);
  return {
    enabled: boolean(item.enabled, `${path}.enabled`),
    audience: audience as "user" | "trace" | "both" | "internal",
    persistChunks: boolean(item.persistChunks, `${path}.persistChunks`),
  };
};
