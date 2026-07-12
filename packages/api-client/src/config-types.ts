import type { JsonObject } from "./graph-types";

export interface ChannelView {
  id: string;
  name: string;
  headRevisionId: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface ContextPresetView {
  id: string;
  name: string;
  headVersionId: string | null;
  createdAt: number;
  updatedAt: number;
}

export type GenerationProviderKind =
  | "open_ai_responses"
  | "open_ai_chat_completions"
  | "claude_messages"
  | "gemini_generate_content";

export interface PublishChannelInput {
  expectedHeadRevisionId: string | null;
  baseUrl: string;
  providerKind: GenerationProviderKind;
  modelId: string;
  credentialSecretId: string | null;
  allowLoopbackHttp: boolean;
  allowUnauthenticated: boolean;
  structuredOutput: boolean;
}

export interface ChannelRevisionView {
  id: string;
  channelId: string;
  revisionNo: number;
  operationTaxonomyVersion: 1;
  adapterDecoderVersion: 1;
  baseUrl: string;
  contentHash: string;
  createdAt: number;
}

export interface ChannelModelDiscoveryView {
  channelId: string;
  channelRevisionId: string;
  operationKey: JsonObject;
  models: Array<{
    id: string;
    name: string | null;
    contextWindow: number | null;
    maxOutputTokens: number | null;
  }>;
}

export interface PublishPresetInput {
  expectedHeadVersionId: string | null;
  spec: JsonObject;
}

export interface ContextPresetVersionView {
  id: string;
  presetId: string;
  versionNo: number;
  semanticPolicyVersion: 1;
  spec: JsonObject;
  contentHash: string;
  createdAt: number;
}

export type ContextBudgetAction = "kept" | "dropped" | "truncated" | "deduped" | "unsupported";
export type ContextCountSource = "provider" | "local" | "estimate";

export interface ContextPreviewItemView {
  itemId: string;
  name: string | null;
  sourceType: string;
  requestedRole: string;
  enabled: boolean;
  included: boolean;
  tokenCount: number;
  action: ContextBudgetAction;
  reason: string | null;
}

export interface ContextBudgetReportView {
  availableInputTokens: number;
  fixedRequestTokens: number;
  assembledTokens: number;
  countSource: ContextCountSource;
  items: Array<{
    itemId: string;
    included: boolean;
    tokenCount: number;
    action: ContextBudgetAction;
    reason: string | null;
  }>;
}

export interface ContextPresetPreviewView {
  presetId: string;
  versionId: string;
  contentMode: "metadata_only";
  countSource: ContextCountSource;
  items: ContextPreviewItemView[];
  budgetReport: ContextBudgetReportView;
  snapshot: {
    config: JsonObject;
    readSetRef: string;
    readSetDigest: string;
    resolvedBindingsDigest: string;
    assemblyDigest: string;
  };
}
