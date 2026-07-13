import type { JsonObject, JsonValue } from "@zhuangsheng/api-client";
import { boolean, integer, object, string } from "./json";
import { importOpenAiGeneration } from "./generation";
import { emptySpec, inactive, populateRoleMacros, substituteKnown, warn, type ImportParts } from "./support";

export function importOpenAi(document: JsonObject, name: string, parts: ImportParts) {
  importOpenAiGeneration(document, parts);
  markInactive(document, parts);
  if (!Array.isArray(document.prompts) || !Array.isArray(document.prompt_order)) throw new Error("prompts and prompt_order must be arrays");
  const prompts = new Map<string, JsonObject>();
  for (const raw of document.prompts) { const prompt = object(raw); const id = string(prompt?.identifier); if (prompt && id) prompts.set(id, prompt); }
  const selected = document.prompt_order.map(object).find((value) => integer(value?.character_id) === 100001)
    ?? object(document.prompt_order[0]);
  if (!selected || !Array.isArray(selected.order)) throw new Error("selected prompt order has no order array");
  const order = selected.order; const history = order.findIndex((entry) => string(object(entry)?.identifier) === "chatHistory");
  const spec = parts.contextSpec ??= emptySpec(name); spec.name = name; populateRoleMacros(spec);
  const used = new Set<number>();
  order.forEach((raw, index) => {
    const entry = object(raw); const identifier = string(entry?.identifier);
    if (!identifier) { warn(parts, "invalid_sillytavern_prompt_order", "prompt order entry has no identifier", `prompt_order.order[${index}]`); return; }
    const prompt = prompts.get(identifier);
    if (!prompt) { warn(parts, "missing_sillytavern_prompt", `prompt order references missing prompt ${identifier}`, `prompt_order.order[${index}]`); return; }
    const enabled = boolean(entry?.enabled, true);
    const position = history >= 0 ? index > history ? "after_history" : "before_history" : "start";
    if (boolean(prompt.marker)) importMarker(spec, identifier, enabled, index, position, used, parts);
    else importLiteral(spec, prompt, identifier, enabled, index, position, parts);
  });
  const prefill = string(document.assistant_prefill);
  if (prefill) upsert(spec, literalItem("st:assistant-prefill", "Assistant Prefill", prefill, "assistant", "assistant_prefill", 0));
}

function importLiteral(spec: JsonObject, prompt: JsonObject, identifier: string, enabled: boolean, index: number, position: string, parts: ImportParts) {
  const content = substituteKnown(string(prompt.content) ?? "", spec);
  const roleValue = string(prompt.role) ?? "system";
  const role = roleValue === "user" || roleValue === "assistant" ? roleValue : "system";
  if (!["system", "user", "assistant"].includes(roleValue)) warn(parts, "unknown_sillytavern_prompt_role", `prompt ${identifier} role ${roleValue} was mapped to system`, `prompts.${identifier}.role`);
  const slug = [...identifier].map((char) => /[A-Za-z0-9_-]/.test(char) ? char : "-").join("").slice(0, 96);
  const item = literalItem(`st:${index}:${slug}`, string(prompt.name) ?? null, content, role, position, index);
  item.enabled = enabled; upsert(spec, item);
  if (content.includes("{{")) warn(parts, "sillytavern_prompt_macros_require_binding", `prompt ${identifier} contains unresolved macros`, `prompts.${identifier}.content`);
}

function importMarker(spec: JsonObject, identifier: string, enabled: boolean, index: number, position: string, used: Set<number>, parts: ImportParts) {
  const aliases = ({ charDescription: ["character"], charPersonality: ["character"], personaDescription: ["persona"], scenario: ["world"], worldInfoBefore: ["lore"], worldInfoAfter: ["lore"], dialogueExamples: ["examples"], chatHistory: ["history"] } as Record<string, string[]>)[identifier] ?? [];
  const items = specItems(spec);
  const found = items.findIndex((raw, itemIndex) => {
    const item = object(raw); const id = string(item?.id); return !used.has(itemIndex) && !!id && (id === identifier || aliases.includes(id.split(/[:/]/)[0]!));
  });
  if (found >= 0) {
    const item = object(items[found])!; item.enabled = enabled; item.order = index;
    if (object(item.source)?.type !== "history") item.position = { type: position };
    used.add(found); return;
  }
  if (identifier === "chatHistory") { items.push(historyItem(enabled, index)); return; }
  warn(parts, "unresolved_sillytavern_marker", `marker ${identifier} has no canonical source in the target preset`, `prompts.${identifier}`);
}

export function literalItem(id: string, name: string | null, text: string, role: string, position: string, order: number): JsonObject {
  return { id, name, enabled: true, requestedRole: role, source: { type: "literal", text }, position: { type: position }, order, priority: 100, insertionDepth: 0, budget: { maxTokens: null, required: true }, overflow: null };
}

function historyItem(enabled: boolean, order: number): JsonObject {
  return { id: "history", name: "Chat History", enabled, requestedRole: "context", source: { type: "history", bindingId: "history", strategy: { type: "all" } }, position: { type: "history" }, order, priority: 90, insertionDepth: 0, budget: { maxTokens: null, required: false }, overflow: { type: "keep_recent", count: null } };
}

export function specItems(spec: JsonObject): JsonValue[] { if (!Array.isArray(spec.items)) spec.items = []; return spec.items as JsonValue[]; }
export function upsert(spec: JsonObject, item: JsonObject) { const items = specItems(spec); const index = items.findIndex((value) => string(object(value)?.id) === item.id); if (index >= 0) items[index] = item; else items.push(item); }

function markInactive(document: JsonObject, parts: ImportParts) {
  for (const key of ["reverse_proxy", "proxy_password", "custom_url", "custom_include_headers", "custom_include_body", "custom_exclude_body"]) if (document[key] !== undefined && document[key] !== null) inactive(parts, key, "connection and credential fields are never imported from presets");
  for (const key of ["chat_completion_source", "openai_model", "claude_model", "openrouter_model"]) if (document[key] !== undefined) inactive(parts, key, "model selection remains controlled by the versioned channel and graph");
}
