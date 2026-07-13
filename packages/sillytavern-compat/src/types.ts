import type {
  ContextPresetVersionView,
  ContextPresetView,
  GraphRevisionView,
  JsonObject,
  JsonValue,
} from "@zhuangsheng/api-client";

export type SillyTavernPresetKind =
  | "open_ai" | "master" | "context" | "instruct" | "system_prompt"
  | "text_completion" | "reasoning" | "regex_scripts" | "unknown";
export type SillyTavernRuleScope = "global" | "character" | "preset";
export type TextTransformTarget = "user_input" | "assistant_output" | "world_info" | "reasoning";
export type TextTransformSurface = "canonical" | "prompt" | "display";
export type PatternMacroMode = "none" | "raw" | "escaped";

export interface TextTransformRuleView {
  id: string;
  name: string;
  priority: number;
  order: number;
  findRegex: string;
  replaceString: string;
  trimStrings: string[];
  targets: TextTransformTarget[];
  surfaces: TextTransformSurface[];
  disabled: boolean;
  runOnEdit: boolean;
  patternMacroMode: PatternMacroMode;
  minDepth: number | null;
  maxDepth: number | null;
}

export interface SillyTavernTextTransformView extends TextTransformRuleView {
  sourceScope: SillyTavernRuleScope;
  inactivePlacements: number[];
}

export interface SillyTavernImportWarningView {
  code: string;
  message: string;
  field: string | null;
}

export interface SillyTavernImportPreviewView {
  compatibilityVersion: 1;
  kind: SillyTavernPresetKind;
  name: string;
  sourceHash: string;
  contextSpec: JsonObject | null;
  generation: JsonObject | null;
  providerExtensions: JsonObject | null;
  textTransforms: SillyTavernTextTransformView[];
  inactiveFields: string[];
  warnings: SillyTavernImportWarningView[];
}

export interface SillyTavernImportInput {
  document: JsonValue;
  sourceName?: string | null;
  targetPresetId?: string | null;
  baseSpec?: JsonObject | null;
}

export interface ApplySillyTavernImportInput extends SillyTavernImportInput {
  expectedHeadVersionId?: string | null;
  channelId?: string | null;
}

export interface SillyTavernImportResultView {
  preview: SillyTavernImportPreviewView;
  preset: ContextPresetView;
  version: ContextPresetVersionView;
  graphRevision: GraphRevisionView | null;
}

export interface TestSillyTavernRegexInput extends SillyTavernImportInput {
  input: string;
  target: TextTransformTarget;
  surface: TextTransformSurface;
  depth?: number | null;
  isEdit?: boolean;
  macros?: Record<string, string>;
}

export interface SillyTavernRegexTestResultView {
  text: string;
  appliedRuleIds: string[];
}

export interface SillyTavernExportDocumentView {
  fileName: string;
  kind: SillyTavernPresetKind;
  scope: SillyTavernRuleScope;
  sourceHash: string;
  document: JsonValue;
}

export interface SillyTavernExportBundleView {
  compatibilityVersion: 1;
  documents: SillyTavernExportDocumentView[];
  warnings: SillyTavernImportWarningView[];
}
