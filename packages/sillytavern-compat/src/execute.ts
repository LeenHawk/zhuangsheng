import { compileRegex } from "./regex";
import { previewSillyTavernImport } from "./import";
import type { SillyTavernRegexTestResultView, TestSillyTavernRegexInput, TextTransformRuleView } from "./types";

const MAX_INPUT_BYTES = 4 * 1024 * 1024;
const MAX_OUTPUT_BYTES = 8 * 1024 * 1024;

export async function testSillyTavernRegex(input: TestSillyTavernRegexInput): Promise<SillyTavernRegexTestResultView> {
  const preview = await previewSillyTavernImport(input);
  const macros = { ...macrosFromSpec(preview.contextSpec), ...input.macros };
  return applyTextTransforms(input.input, preview.textTransforms, {
    target: input.target, surface: input.surface, depth: input.depth ?? null,
    isEdit: input.isEdit ?? false, macros,
  });
}

export function applyTextTransforms(
  input: string,
  rules: TextTransformRuleView[],
  context: { target: string; surface: string; depth: number | null; isEdit: boolean; macros: Record<string, string> },
) {
  if (new TextEncoder().encode(input).byteLength > MAX_INPUT_BYTES) throw new Error("text transform input exceeds four MiB");
  let text = input; const appliedRuleIds: string[] = [];
  for (const rule of rules) {
    if (!applies(rule, context)) continue;
    const next = applyRule(text, rule, context.macros);
    if (next !== text) appliedRuleIds.push(rule.id);
    text = next;
    if (new TextEncoder().encode(text).byteLength > MAX_OUTPUT_BYTES) throw new Error(`output exceeds eight MiB after ${rule.id}`);
  }
  return { text, appliedRuleIds };
}

function applies(rule: TextTransformRuleView, context: { target: string; surface: string; depth: number | null; isEdit: boolean }) {
  if (rule.disabled || context.isEdit && !rule.runOnEdit || !rule.targets.includes(context.target as never) || !rule.surfaces.includes(context.surface as never)) return false;
  if (context.depth === null) return true;
  return (rule.minDepth === null || context.depth >= rule.minDepth) && (rule.maxDepth === null || context.depth <= rule.maxDepth);
}

function applyRule(input: string, rule: TextTransformRuleView, macros: Record<string, string>) {
  const source = rule.patternMacroMode === "none" ? rule.findRegex : substitute(rule.findRegex, macros, rule.patternMacroMode === "escaped");
  const regex = compileRegex(source); const global = regex.global && !regex.sticky;
  let output = ""; let cursor = 0; let matched = false;
  while (true) {
    const match = regex.exec(input); if (!match) break;
    output += input.slice(cursor, match.index) + replacement(rule, match, input, macros);
    cursor = match.index + match[0].length; matched = true;
    if (!global) break;
    if (match[0] === "") regex.lastIndex++;
  }
  return matched ? output + input.slice(cursor) : input;
}

function replacement(rule: TextTransformRuleView, match: RegExpExecArray, input: string, macros: Record<string, string>) {
  const value = rule.replaceString.replace(/\$([$&`']|\d{1,2}|<[^>]+>)/g, (token, reference: string) => {
    if (reference === "$") return "$";
    if (reference === "&") return trimmed(match[0], rule, macros);
    if (reference === "`") return input.slice(0, match.index);
    if (reference === "'") return input.slice(match.index + match[0].length);
    if (reference.startsWith("<")) return trimmed(match.groups?.[reference.slice(1, -1)] ?? "", rule, macros);
    return trimmed(match[Number(reference)] ?? token, rule, macros);
  });
  return substitute(value, macros, false);
}

function trimmed(value: string, rule: TextTransformRuleView, macros: Record<string, string>) {
  return rule.trimStrings.reduce((text, trim) => text.replaceAll(substitute(trim, macros, false), ""), value);
}

function substitute(text: string, macros: Record<string, string>, escaped: boolean) {
  return Object.entries(macros).reduce((result, [name, value]) => result.replaceAll(`{{${name}}}`, escaped ? escapeRegex(value) : value), text);
}

function escapeRegex(value: string) { return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"); }
function macrosFromSpec(spec: import("@zhuangsheng/api-client").JsonObject | null) { const value = spec?.textTransformMacros; return value && !Array.isArray(value) && typeof value === "object" ? value as Record<string, string> : {}; }
