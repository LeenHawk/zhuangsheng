import { DecodeError } from "./decode-error";
import { nullableString, number, record, string, stringArray } from "./decode-helpers";
import type { RolePlayCompatibilityView, RolePlayGraphOptionView } from "./types";

const compatibility = (value: unknown, path: string): RolePlayCompatibilityView => {
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
      compatibility: compatibility(item.compatibility, `${path}.compatibility`),
    };
  });
};
