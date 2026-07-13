import type { JsonObject } from "@zhuangsheng/api-client";
import { object, string } from "./json";
import { importCompletionGeneration } from "./generation";
import { literalItem, specItems, upsert } from "./openai";
import { emptySpec, inactive, populateRoleMacros, sectionKind, substituteKnown, warn, type ImportParts } from "./support";
import type { SillyTavernPresetKind } from "./types";

export function importMaster(document: JsonObject, name: string, parts: ImportParts) {
  for (const key of ["context", "instruct", "sysprompt", "preset", "reasoning"]) {
    const section = object(document[key]); const kind = sectionKind(key);
    if (section && kind) importSection(kind, section, name, parts);
  }
  const start = object(document.srw); if (start) importStartReply(start, name, parts);
}

export function importSection(kind: SillyTavernPresetKind, document: JsonObject, name: string, parts: ImportParts) {
  if (kind === "system_prompt") importSystemPrompt(document, name, parts);
  if (kind === "context") importContextTemplate(document, name, parts);
  if (kind === "instruct") importInstruct(document, parts);
  if (kind === "reasoning") importReasoning(document, parts);
  if (kind === "text_completion") importCompletionGeneration(document, parts);
}

function importSystemPrompt(document: JsonObject, name: string, parts: ImportParts) {
  const raw = string(document.content);
  if (raw === null) { warn(parts, "invalid_sillytavern_system_prompt", "system prompt has no string content", "content"); return; }
  const spec = parts.contextSpec ??= emptySpec(name); populateRoleMacros(spec);
  const content = substituteKnown(raw, spec);
  upsert(spec, literalItem("st:system-prompt", string(document.name) ?? "System Prompt", content, "system", "start", -100));
  const post = string(document.post_history); if (post) upsert(spec, literalItem("st:post-history", "Post-history prompt", post, "system", "after_history", 100));
  if (content.includes("{{")) warn(parts, "sillytavern_prompt_macros_require_binding", "system prompt contains unresolved macros", "content");
}

function importContextTemplate(document: JsonObject, name: string, parts: ImportParts) {
  const story = string(document.story_string);
  if (story === null) { warn(parts, "invalid_sillytavern_context_template", "context template has no story_string", "story_string"); return; }
  if (!parts.contextSpec) { parts.contextSpec = emptySpec(name); inactive(parts, "story_string", "context template needs canonical bindings before marker order can be applied"); return; }
  const markers = [["system", "style"], ["wiBefore", "lore"], ["description", "character"], ["personality", "character"], ["scenario", "world"], ["wiAfter", "lore"], ["persona", "persona"]] as const;
  let matched = 0;
  for (const [marker, profile] of markers) {
    const position = markerPosition(story, marker); if (position === null) continue;
    const item = specItems(parts.contextSpec).map(object).find((value) => string(value?.id)?.split(/[:/]/)[0] === profile);
    if (item) { item.order = position; matched++; }
  }
  if (!matched) inactive(parts, "story_string", "custom Handlebars layout has no matching canonical items");
  else if (story.includes("{{#")) warn(parts, "sillytavern_context_layout_partial", "known markers were reordered; custom Handlebars control flow remains inactive", "story_string");
  for (const key of ["example_separator", "chat_start", "story_string_role", "story_string_depth"]) if (document[key] !== undefined) inactive(parts, key, "context formatting field remains inactive");
}

function importInstruct(document: JsonObject, parts: ImportParts) {
  for (const key of ["input_sequence", "output_sequence", "last_output_sequence", "system_sequence", "stop_sequence", "first_output_sequence", "output_suffix", "input_suffix", "system_suffix", "story_string_prefix", "story_string_suffix"]) if (document[key] !== undefined) inactive(parts, key, "instruct sequence formatting requires completion-mode framing");
  const stop = string(document.stop_sequence); if (stop) {
    const generation = parts.generation ??= { temperature: null, topP: null, maxOutputTokens: null, stop: [], seed: null };
    const values = Array.isArray(generation.stop) ? generation.stop : []; if (!values.includes(stop)) values.push(stop); generation.stop = values;
  }
}

function importReasoning(document: JsonObject, parts: ImportParts) { for (const key of ["prefix", "suffix", "separator", "auto_parse", "add_to_prompts"]) if (document[key] !== undefined) inactive(parts, key, "reasoning formatting is not sent as a prompt"); }
function importStartReply(document: JsonObject, name: string, parts: ImportParts) { const value = string(document.value); if (value) upsert(parts.contextSpec ??= emptySpec(name), literalItem("st:start-reply-with", "Start Reply With", value, "assistant", "assistant_prefill", 0)); }
function markerPosition(story: string, marker: string) { const direct = story.indexOf(`{{${marker}}}`); if (direct >= 0) return direct; const conditional = story.indexOf(`{{#if ${marker}}}`); return conditional >= 0 ? conditional : null; }
