import type { JsonValue } from "@zhuangsheng/api-client";
import { object } from "./json";
import type { SillyTavernPresetKind } from "./types";

export function detectPresetKind(document: JsonValue): SillyTavernPresetKind {
  if (isRegexArray(document)) return "regex_scripts";
  const value = object(document);
  if (!value) return "unknown";
  if (Array.isArray(value.prompts) && Array.isArray(value.prompt_order)) return "open_ai";
  if (embeddedRegex(value)) return "regex_scripts";
  if (["context", "instruct", "sysprompt", "reasoning", "preset"].some((key) => key in value)) return "master";
  if ("story_string" in value) return "context";
  if ("input_sequence" in value && "output_sequence" in value) return "instruct";
  if ("content" in value && "name" in value) return "system_prompt";
  if (["prefix", "suffix", "separator"].every((key) => key in value)) return "reasoning";
  if (["temp", "top_k", "top_p", "rep_pen"].every((key) => key in value)) return "text_completion";
  return "unknown";
}

export function isRegexArray(value: JsonValue | undefined): boolean {
  return Array.isArray(value) && value.every((entry) => {
    const rule = object(entry);
    return !!rule && typeof rule.findRegex === "string" && typeof rule.replaceString === "string";
  });
}

function embeddedRegex(value: Record<string, JsonValue>) {
  const data = object(value.data);
  const dataExtensions = data && object(data.extensions);
  const extensions = object(value.extensions);
  return isRegexArray(dataExtensions?.regex_scripts) || isRegexArray(extensions?.regex_scripts);
}
