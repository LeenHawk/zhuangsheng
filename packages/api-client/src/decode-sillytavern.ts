import type {
  SillyTavernImportPreviewView,
  SillyTavernImportResultView,
  SillyTavernPresetKind,
  TextTransformPlacement,
  TextTransformRuleView,
  TextTransformSurface,
} from "./config-types";
import { decodeContextPreset, decodeContextPresetVersion } from "./decode-config";
import { DecodeError } from "./decode-error";
import {
  boolean,
  jsonObject,
  nullableString,
  number,
  record,
  string,
  stringArray,
} from "./decode-helpers";

const kinds = new Set<SillyTavernPresetKind>([
  "open_ai", "master", "context", "instruct", "system_prompt",
  "text_completion", "reasoning", "regex_scripts", "unknown",
]);
const placements = new Set<TextTransformPlacement>([
  "user_input", "ai_output", "slash_command", "world_info", "reasoning",
]);
const surfaces = new Set<TextTransformSurface>(["canonical", "prompt", "display"]);
const scopes = new Set(["global", "character", "preset"] as const);
const macroModes = new Set(["none", "raw", "escaped"] as const);

export function decodeSillyTavernImportPreview(
  value: unknown,
): SillyTavernImportPreviewView {
  const item = record(value, "sillyTavernImportPreview");
  const compatibilityVersion = number(
    item.compatibilityVersion,
    "sillyTavernImportPreview.compatibilityVersion",
  );
  if (compatibilityVersion !== 1) {
    throw new DecodeError("sillyTavernImportPreview.compatibilityVersion");
  }
  const kind = member(item.kind, kinds, "sillyTavernImportPreview.kind");
  if (!Array.isArray(item.textTransforms) || !Array.isArray(item.warnings)) {
    throw new DecodeError("sillyTavernImportPreview.collections");
  }
  return {
    compatibilityVersion,
    kind,
    name: string(item.name, "sillyTavernImportPreview.name"),
    sourceHash: string(item.sourceHash, "sillyTavernImportPreview.sourceHash"),
    contextSpec: nullableObject(item.contextSpec, "sillyTavernImportPreview.contextSpec"),
    generation: nullableObject(item.generation, "sillyTavernImportPreview.generation"),
    providerExtensions: nullableObject(
      item.providerExtensions,
      "sillyTavernImportPreview.providerExtensions",
    ),
    textTransforms: item.textTransforms.map((rule, index) =>
      decodeRule(rule, `sillyTavernImportPreview.textTransforms[${index}]`)),
    inactiveFields: stringArray(
      item.inactiveFields,
      "sillyTavernImportPreview.inactiveFields",
    ),
    warnings: item.warnings.map((raw, index) => {
      const path = `sillyTavernImportPreview.warnings[${index}]`;
      const warning = record(raw, path);
      return {
        code: string(warning.code, `${path}.code`),
        message: string(warning.message, `${path}.message`),
        field: nullableString(warning.field, `${path}.field`),
      };
    }),
  };
}

export function decodeSillyTavernImportResult(
  value: unknown,
): SillyTavernImportResultView {
  const item = record(value, "sillyTavernImportResult");
  return {
    preview: decodeSillyTavernImportPreview(item.preview),
    preset: decodeContextPreset(item.preset),
    version: decodeContextPresetVersion(item.version),
  };
}

function decodeRule(value: unknown, path: string): TextTransformRuleView {
  const rule = record(value, path);
  const scope = member(rule.scope, scopes, `${path}.scope`);
  const macroMode = member(rule.macroMode, macroModes, `${path}.macroMode`);
  if (!Array.isArray(rule.placements) || !Array.isArray(rule.surfaces)) {
    throw new DecodeError(`${path}.surfaces`);
  }
  return {
    id: string(rule.id, `${path}.id`),
    name: string(rule.name, `${path}.name`),
    scope,
    order: number(rule.order, `${path}.order`),
    findRegex: string(rule.findRegex, `${path}.findRegex`),
    replaceString: string(rule.replaceString, `${path}.replaceString`),
    trimStrings: stringArray(rule.trimStrings, `${path}.trimStrings`),
    placements: rule.placements.map((value, index) =>
      member(value, placements, `${path}.placements[${index}]`)),
    surfaces: rule.surfaces.map((value, index) =>
      member(value, surfaces, `${path}.surfaces[${index}]`)),
    disabled: boolean(rule.disabled, `${path}.disabled`),
    runOnEdit: boolean(rule.runOnEdit, `${path}.runOnEdit`),
    macroMode,
    minDepth: nullableNumber(rule.minDepth, `${path}.minDepth`),
    maxDepth: nullableNumber(rule.maxDepth, `${path}.maxDepth`),
  };
}

function nullableObject(value: unknown, path: string) {
  return value === null ? null : jsonObject(value, path);
}

function nullableNumber(value: unknown, path: string) {
  return value === null ? null : number(value, path);
}

function member<T extends string>(value: unknown, values: Set<T>, path: string): T {
  const decoded = string(value, path) as T;
  if (!values.has(decoded)) throw new DecodeError(path);
  return decoded;
}
