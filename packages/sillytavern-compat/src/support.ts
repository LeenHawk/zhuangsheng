import type { JsonObject, JsonValue } from "@zhuangsheng/api-client";
import { cloneJson, object, string } from "./json";
import type {
  SillyTavernImportWarningView,
  SillyTavernPresetKind,
  SillyTavernTextTransformView,
  TextTransformRuleView,
} from "./types";

export interface ImportParts {
  contextSpec: JsonObject | null;
  generation: JsonObject | null;
  providerExtensions: JsonObject | null;
  views: SillyTavernTextTransformView[];
  inactiveFields: string[];
  warnings: SillyTavernImportWarningView[];
}

export const PRIORITY = { global: 0, character: 1_000, preset: 2_000 } as const;
const SECTION_KINDS: Record<string, SillyTavernPresetKind> = {
  context: "context",
  instruct: "instruct",
  sysprompt: "system_prompt",
  preset: "text_completion",
  reasoning: "reasoning",
};

export function parts(baseSpec?: JsonObject | null): ImportParts {
  const contextSpec = baseSpec ? cloneJson(baseSpec) : null;
  const rules = Array.isArray(contextSpec?.textTransforms)
    ? contextSpec.textTransforms.flatMap((value) => runtimeRule(value) ? [viewFromRuntime(runtimeRule(value)!)] : [])
    : [];
  return { contextSpec, generation: null, providerExtensions: null, views: rules, inactiveFields: [], warnings: [] };
}

export function emptySpec(name: string): JsonObject {
  return {
    id: null, name, mode: "chat", items: [], budget: null, postProcess: [],
    textTransforms: [], textTransformMacros: {}, preview: null,
  };
}

export function importName(document: JsonValue, sourceName?: string | null) {
  return (string(object(document)?.name) ?? sourceName ?? "SillyTavern import").trim().slice(0, 200);
}

export function warn(parts: ImportParts, code: string, message: string, field: string | null = null) {
  parts.warnings.push({ code, message, field });
}

export function inactive(parts: ImportParts, field: string, message: string) {
  parts.inactiveFields.push(field);
  warn(parts, "sillytavern_field_inactive", message, field);
}

export function installRules(parts: ImportParts, imported: SillyTavernTextTransformView[]) {
  for (const rule of imported) {
    const index = parts.views.findIndex((value) => value.id === rule.id);
    if (index >= 0) parts.views[index] = rule; else parts.views.push(rule);
  }
  parts.views.sort((left, right) => left.priority - right.priority || left.order - right.order);
  const spec = parts.contextSpec ??= emptySpec("SillyTavern regex");
  spec.textTransforms = parts.views.map(stripView) as unknown as JsonValue;
}

export function populateRoleMacros(spec: JsonObject) {
  const macros = object(spec.textTransformMacros) ?? {};
  const items = Array.isArray(spec.items) ? spec.items : [];
  for (const raw of items) {
    const item = object(raw); const id = string(item?.id); const name = string(item?.name);
    if (!id || !name?.trim()) continue;
    const profile = id.split(/[:/]/, 1)[0];
    if (profile === "character") { macros.char ??= name; macros.name2 ??= name; }
    if (profile === "persona") { macros.user ??= name; macros.name1 ??= name; }
  }
  spec.textTransformMacros = macros;
}

export function substituteKnown(text: string, spec: JsonObject) {
  const macros = object(spec.textTransformMacros) ?? {};
  return Object.entries(macros).reduce((result, [name, value]) =>
    typeof value === "string" ? result.replaceAll(`{{${name}}}`, value) : result, text);
}

export function sectionKind(key: string): SillyTavernPresetKind | null {
  return SECTION_KINDS[key] ?? null;
}

function stripView(rule: SillyTavernTextTransformView): TextTransformRuleView {
  const { sourceScope: _scope, inactivePlacements: _inactive, ...runtime } = rule;
  return runtime;
}

function runtimeRule(value: JsonValue): TextTransformRuleView | null {
  const rule = object(value);
  if (!rule || typeof rule.id !== "string" || typeof rule.findRegex !== "string") return null;
  return rule as unknown as TextTransformRuleView;
}

function viewFromRuntime(rule: TextTransformRuleView): SillyTavernTextTransformView {
  const sourceScope = rule.priority < PRIORITY.character ? "global" : rule.priority < PRIORITY.preset ? "character" : "preset";
  return { ...rule, sourceScope, inactivePlacements: [] };
}
