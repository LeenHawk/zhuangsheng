import type { JsonObject, JsonValue } from "@zhuangsheng/api-client";
import { canonicalHash, object, string } from "./json";
import { PRIORITY } from "./support";
import type {
  SillyTavernExportBundleView, SillyTavernExportDocumentView,
  SillyTavernImportWarningView, SillyTavernRuleScope, TextTransformRuleView,
} from "./types";

export async function exportSillyTavernBundle(
  name: string,
  spec: JsonObject,
  generation: JsonObject | null = null,
  extensions: JsonObject | null = null,
): Promise<SillyTavernExportBundleView> {
  const warnings: SillyTavernImportWarningView[] = [];
  const { prompts, order, assistantPrefill } = exportPrompts(spec, warnings);
  const document: JsonObject = { name, prompts, prompt_order: [{ character_id: 100001, order }] };
  exportGeneration(document, generation, extensions, warnings);
  if (assistantPrefill) document.assistant_prefill = assistantPrefill;
  const rules = runtimeRules(spec);
  const presetRules = rules.filter((rule) => sourceScope(rule) === "preset").map(exportRule);
  if (presetRules.length) document.extensions = { regex_scripts: presetRules };
  const documents: SillyTavernExportDocumentView[] = [await exportDocument("sillytavern-preset.json", "open_ai", "preset", document)];
  const global = rules.filter((rule) => sourceScope(rule) === "global").map(exportRule);
  if (global.length) documents.push(await exportDocument("sillytavern-global-regex.json", "regex_scripts", "global", global));
  const character = rules.filter((rule) => sourceScope(rule) === "character").map(exportRule);
  if (character.length) documents.push(await exportDocument("sillytavern-character-regex.json", "regex_scripts", "character", { data: { extensions: { regex_scripts: character } } }));
  return { compatibilityVersion: 1, documents, warnings };
}

function exportPrompts(spec: JsonObject, warnings: SillyTavernImportWarningView[]) {
  const items = (Array.isArray(spec.items) ? spec.items : []).map((raw, index) => ({ raw: object(raw), index }))
    .filter((entry): entry is { raw: JsonObject; index: number } => !!entry.raw)
    .sort((left, right) => numeric(left.raw.order) - numeric(right.raw.order) || left.index - right.index);
  const prompts: JsonValue[] = []; const order: JsonValue[] = []; const used = new Set<string>(); let assistantPrefill: string | null = null;
  for (const { raw: item } of items) {
    const position = string(object(item.position)?.type);
    const text = itemText(item);
    if (position === "assistant_prefill") { if (text !== null) assistantPrefill = text; continue; }
    const marker = markerIdentifier(item);
    if (!marker && text === null) { warnings.push({ code: "sillytavern_export_source", message: "unsupported context source was omitted", field: string(item.id) }); continue; }
    const base = marker ?? literalIdentifier(string(item.id) ?? "prompt");
    const identifier = unique(base, used);
    prompts.push({ identifier, name: string(item.name) ?? string(item.id) ?? identifier, role: roleName(string(item.requestedRole)), content: marker ? "" : text!, marker: !!marker });
    order.push({ identifier, enabled: item.enabled !== false });
  }
  return { prompts, order, assistantPrefill };
}

function markerIdentifier(item: JsonObject) {
  const source = object(item.source); if (string(source?.type) === "history") return "chatHistory";
  const profile = (string(item.id) ?? "").split(/[:/]/)[0];
  return ({ character: "charDescription", persona: "personaDescription", world: "scenario", lore: "worldInfoBefore", examples: "dialogueExamples", history: "chatHistory" } as Record<string, string>)[profile!];
}

function itemText(item: JsonObject) { const source = object(item.source); return ["literal", "template"].includes(string(source?.type) ?? "") ? string(source?.text) ?? string(source?.template) : null; }
function literalIdentifier(id: string) { const match = /^st:[^:]+:(.+)$/.exec(id); return match?.[1] ?? id; }
function roleName(role: string | null) { return role === "user" || role === "assistant" ? role : "system"; }
function unique(base: string, used: Set<string>) { let value = base; let suffix = 2; while (used.has(value)) value = `${base}-${suffix++}`; used.add(value); return value; }

function exportGeneration(target: JsonObject, generation: JsonObject | null, extensions: JsonObject | null, warnings: SillyTavernImportWarningView[]) {
  if (generation) {
    if (generation.temperature != null) target.temperature = generation.temperature;
    if (generation.topP != null) target.top_p = generation.topP;
    if (generation.maxOutputTokens != null) target.openai_max_tokens = generation.maxOutputTokens;
    if (generation.seed != null) target.seed = generation.seed;
    target.stop = Array.isArray(generation.stop) ? generation.stop : [];
  }
  const openai = object(extensions?.openai); const extra = object(openai?.extraBody);
  for (const key of ["frequency_penalty", "presence_penalty", "top_k", "top_a", "min_p", "repetition_penalty"]) if (extra?.[key] !== undefined) target[key] = extra[key]!;
  if (openai && (Object.keys(object(openai.options) ?? {}).length || Object.keys(object(openai.extraHeaders) ?? {}).length)) warnings.push({ code: "sillytavern_export_provider_fields", message: "provider options and headers were intentionally omitted", field: null });
}

function runtimeRules(spec: JsonObject): TextTransformRuleView[] { return (Array.isArray(spec.textTransforms) ? spec.textTransforms : []).flatMap((raw) => object(raw) ? [raw as unknown as TextTransformRuleView] : []); }
function sourceScope(rule: TextTransformRuleView): SillyTavernRuleScope { return rule.priority < PRIORITY.character ? "global" : rule.priority < PRIORITY.preset ? "character" : "preset"; }
function exportRule(rule: TextTransformRuleView): JsonObject {
  const placement = rule.targets.map((target) => ({ user_input: 1, assistant_output: 2, world_info: 5, reasoning: 6 })[target]);
  return { id: rule.id, scriptName: rule.name, findRegex: rule.findRegex, replaceString: rule.replaceString, trimStrings: rule.trimStrings, placement, disabled: rule.disabled, markdownOnly: rule.surfaces.includes("display"), promptOnly: rule.surfaces.includes("prompt"), runOnEdit: rule.runOnEdit, substituteRegex: ({ none: 0, raw: 1, escaped: 2 })[rule.patternMacroMode], minDepth: rule.minDepth, maxDepth: rule.maxDepth };
}

async function exportDocument(fileName: string, kind: "open_ai" | "regex_scripts", scope: SillyTavernRuleScope, document: JsonValue): Promise<SillyTavernExportDocumentView> { return { fileName, kind, scope, sourceHash: await canonicalHash(document), document }; }
function numeric(value: JsonValue | undefined) { return typeof value === "number" ? value : 0; }
