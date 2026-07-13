import type { JsonObject, JsonValue } from "@zhuangsheng/api-client";
import { integer, number, object, string } from "./json";
import { warn, type ImportParts } from "./support";

const OPENAI_EXTRA = ["frequency_penalty", "presence_penalty", "top_k", "top_a", "min_p", "repetition_penalty"];
const COMPLETION_EXTRA = ["top_k", "min_p", "rep_pen", "rep_pen_range", "typical_p", "tfs"];

export function importOpenAiGeneration(document: JsonObject, parts: ImportParts) {
  setGeneration(parts, {
    temperature: number(document.temperature), topP: number(document.top_p),
    maxOutputTokens: positiveInteger(document.openai_max_tokens), stop: stopStrings(document),
    seed: nonNegativeInteger(document.seed),
  });
  setExtensions(document, OPENAI_EXTRA, parts, "non-portable generation fields were preserved as OpenAI extra-body options");
}

export function importCompletionGeneration(document: JsonObject, parts: ImportParts) {
  setGeneration(parts, {
    temperature: number(document.temp), topP: number(document.top_p),
    maxOutputTokens: positiveInteger(document.genamt) ?? positiveInteger(document.max_length),
    stop: stopStrings(document), seed: nonNegativeInteger(document.seed),
  });
  setExtensions(document, COMPLETION_EXTRA, parts, "text-completion sampler fields were preserved as provider extra-body options");
}

function setGeneration(parts: ImportParts, value: JsonObject) {
  if (value.temperature !== null || value.topP !== null || value.maxOutputTokens !== null || value.seed !== null || (value.stop as JsonValue[]).length) {
    parts.generation = value;
  }
}

function setExtensions(document: JsonObject, keys: string[], parts: ImportParts, message: string) {
  const extraBody = Object.fromEntries(keys.flatMap((key) => {
    const value = document[key];
    return value !== undefined && (typeof value === "boolean" || number(value) !== null) ? [[key, value]] : [];
  })) as JsonObject;
  if (!Object.keys(extraBody).length) return;
  parts.providerExtensions = { openai: { options: {}, extraBody, extraHeaders: {} }, claude: null, gemini: null };
  warn(parts, "sillytavern_provider_extensions", message);
}

function stopStrings(document: JsonObject): JsonValue[] {
  for (const key of ["stop", "stopping_strings"]) {
    const values = document[key];
    if (Array.isArray(values)) return values.filter((value): value is string => typeof value === "string");
  }
  const raw = string(document.custom_stopping_strings);
  if (!raw) return [];
  try { const value = JSON.parse(raw); return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : []; }
  catch { return []; }
}

function positiveInteger(value: JsonValue | undefined) { const result = integer(value); return result !== null && result > 0 ? result : null; }
function nonNegativeInteger(value: JsonValue | undefined) { const result = integer(value); return result !== null && result >= 0 ? result : null; }
