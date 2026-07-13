import { stringifyJsonExact, type JsonObject, type JsonValue } from "@zhuangsheng/api-client";
import { boolean, integer, object, string } from "./json";
import { PRIORITY, installRules, warn, type ImportParts } from "./support";
import type {
  PatternMacroMode, SillyTavernRuleScope, SillyTavernTextTransformView,
  TextTransformSurface, TextTransformTarget,
} from "./types";

export function importTopLevelRegex(document: JsonValue, parts: ImportParts) {
  installRules(parts, parseArray(document, "global", "regex_scripts", parts));
}

export function importEmbeddedRegex(document: JsonObject, parts: ImportParts) {
  const data = object(document.data); const dataExtensions = object(data?.extensions);
  const extensions = object(document.extensions);
  const rules = [
    [dataExtensions?.regex_scripts, "character", "data.extensions.regex_scripts"],
    [extensions?.regex_scripts, "preset", "extensions.regex_scripts"],
  ] as const;
  installRules(parts, rules.flatMap(([value, scope, path]) => value === undefined ? [] : parseArray(value, scope, path, parts)));
}

function parseArray(value: JsonValue, scope: SillyTavernRuleScope, path: string, parts: ImportParts) {
  if (!Array.isArray(value)) throw new Error(`${path} must be an array`);
  if (value.length > 256) throw new Error("preset has more than 256 text transform rules");
  return value.map((entry, index) => parseRule(entry, scope, index, path, parts));
}

function parseRule(value: JsonValue, scope: SillyTavernRuleScope, index: number, path: string, parts: ImportParts): SillyTavernTextTransformView {
  const rule = object(value);
  if (!rule) throw new Error(`${path}[${index}] must be an object`);
  const findRegex = requiredString(rule.findRegex, `${path}[${index}].findRegex`);
  const replaceString = requiredString(rule.replaceString, `${path}[${index}].replaceString`)
    .replace(/\{\{match\}\}/gi, "$0");
  const { targets, inactivePlacements } = targetsAndInactive(rule, index, path, parts);
  const surfaces = surfacesFor(rule);
  const macroValue = integer(rule.substituteRegex) ?? 0;
  const patternMacroMode: PatternMacroMode = macroValue === 1 ? "raw" : macroValue === 2 ? "escaped" : "none";
  if (![0, 1, 2].includes(macroValue)) warn(parts, "unknown_sillytavern_regex_substitution", `unknown substituteRegex value ${macroValue}; substitution disabled`, `${path}[${index}].substituteRegex`);
  const id = string(rule.id)?.trim() || `st-${scope}-${index}-${hashToken(stringifyJsonExact(value))}`;
  const result: SillyTavernTextTransformView = {
    id, name: string(rule.scriptName) ?? "Unnamed regex", sourceScope: scope,
    priority: PRIORITY[scope], order: index, findRegex, replaceString,
    trimStrings: Array.isArray(rule.trimStrings) ? rule.trimStrings.filter((item): item is string => typeof item === "string") : [],
    targets, inactivePlacements, surfaces, disabled: boolean(rule.disabled),
    runOnEdit: boolean(rule.runOnEdit), patternMacroMode,
    minDepth: integer(rule.minDepth), maxDepth: nonNegative(rule.maxDepth),
  };
  validateRule(result);
  return result;
}

function targetsAndInactive(rule: JsonObject, index: number, path: string, parts: ImportParts) {
  let placements = Array.isArray(rule.placement) ? rule.placement.flatMap((value) => integer(value) ?? []).map(Number) : [];
  const legacyDisplay = placements.includes(0);
  if (legacyDisplay) {
    placements = placements.length === 1 ? [1, 2, 3, 5, 6] : placements.filter((value) => value !== 0);
    warn(parts, "sillytavern_regex_legacy_display", "legacy MD placement was migrated to prompt and display surfaces", `${path}[${index}].placement`);
  }
  if (placements.includes(4)) {
    placements = placements.length === 1 ? [3] : placements.filter((value) => value !== 4);
    warn(parts, "sillytavern_regex_legacy_sendas", "legacy sendAs placement remains inactive without STscript", `${path}[${index}].placement`);
  }
  const targets: TextTransformTarget[] = [];
  const inactivePlacements: number[] = [];
  for (const placement of placements) {
    const target = ({ 1: "user_input", 2: "assistant_output", 5: "world_info", 6: "reasoning" } as const)[placement as 1];
    if (target) targets.push(target);
    else {
      inactivePlacements.push(placement);
      warn(parts, placement === 3 ? "sillytavern_regex_slash_inactive" : "unknown_sillytavern_regex_placement", placement === 3 ? "STscript slash-command placement is preserved only in frontend compatibility metadata" : `unknown regex placement ${placement} was ignored`, `${path}[${index}].placement`);
    }
  }
  return { targets: [...new Set(targets)], inactivePlacements: [...new Set(inactivePlacements)] };
}

function surfacesFor(rule: JsonObject): TextTransformSurface[] {
  const markdown = boolean(rule.markdownOnly) || (Array.isArray(rule.placement) && rule.placement.some((value) => integer(value) === 0));
  const prompt = boolean(rule.promptOnly) || markdown && Array.isArray(rule.placement) && rule.placement.some((value) => integer(value) === 0);
  if (markdown && prompt) return ["prompt", "display"];
  if (markdown) return ["display"];
  if (prompt) return ["prompt"];
  return ["canonical"];
}

function validateRule(rule: SillyTavernTextTransformView) {
  if (!rule.id || rule.id.length > 128 || rule.findRegex.length > 65_536 || rule.replaceString.length > 65_536) throw new Error(`invalid regex rule ${rule.id}`);
  compileRegex(rule.findRegex.replace(/\{\{[A-Za-z_][A-Za-z0-9_]*\}\}/g, "macro"));
}

export function compileRegex(source: string) {
  const parsed = splitRegex(source);
  const flags = [...new Set(parsed.flags)].join("");
  if ([...flags].some((flag) => !"dgimsuy".includes(flag))) throw new Error(`unsupported regex flags: ${flags}`);
  return new RegExp(parsed.pattern, flags);
}

function splitRegex(source: string) {
  if (!source.startsWith("/")) return { pattern: source, flags: "" };
  for (let index = source.length - 1; index > 0; index--) {
    if (source[index] !== "/") continue;
    let slashes = 0; for (let cursor = index - 1; source[cursor] === "\\"; cursor--) slashes++;
    if (slashes % 2 === 0) return { pattern: source.slice(1, index), flags: source.slice(index + 1) };
  }
  throw new Error("slash-delimited regex has no closing slash");
}

function requiredString(value: JsonValue | undefined, field: string) { const result = string(value); if (result === null) throw new Error(`${field} must be a string`); return result; }
function nonNegative(value: JsonValue | undefined) { const result = integer(value); return result !== null && result >= 0 ? result : null; }
function hashToken(value: string) { let hash = 2166136261; for (const char of value) hash = Math.imul(hash ^ char.charCodeAt(0), 16777619); return (hash >>> 0).toString(16).padStart(8, "0"); }
