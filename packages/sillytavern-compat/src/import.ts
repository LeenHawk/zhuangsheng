import type { JsonObject } from "@zhuangsheng/api-client";
import { canonicalHash, object } from "./json";
import { detectPresetKind } from "./detect";
import { importOpenAi } from "./openai";
import { importEmbeddedRegex, importTopLevelRegex } from "./regex";
import { importMaster, importSection } from "./sections";
import { emptySpec, importName, installRules, parts } from "./support";
import type { SillyTavernImportInput, SillyTavernImportPreviewView } from "./types";

export async function previewSillyTavernImport(input: SillyTavernImportInput): Promise<SillyTavernImportPreviewView> {
  const kind = detectPresetKind(input.document);
  if (kind === "unknown") throw new Error("document is not a recognized SillyTavern preset or regex export");
  const name = importName(input.document, input.sourceName);
  const state = parts(input.baseSpec);
  const document = object(input.document);
  if (kind === "open_ai" && document) importOpenAi(document, name, state);
  else if (kind === "regex_scripts") {
    if (Array.isArray(input.document)) importTopLevelRegex(input.document, state);
    else if (document) importEmbeddedRegex(document, state);
  } else if (kind === "master" && document) importMaster(document, name, state);
  else if (document) importSection(kind, document, name, state);
  if (kind !== "regex_scripts" && document) importEmbeddedRegex(document, state);
  if (!state.contextSpec && state.views.length) state.contextSpec = emptySpec(name);
  if (state.contextSpec) installRules(state, []);
  state.inactiveFields = [...new Set(state.inactiveFields)].sort();
  return {
    compatibilityVersion: 1, kind, name,
    sourceHash: await canonicalHash(input.document), contextSpec: state.contextSpec,
    generation: state.generation, providerExtensions: state.providerExtensions,
    textTransforms: state.views, inactiveFields: state.inactiveFields, warnings: state.warnings,
  };
}
