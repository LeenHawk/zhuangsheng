import { boolean, jsonObject, nullableString, number, record, string } from "./decode-helpers";
import { DecodeError } from "./decode-error";
import type { ChannelModelDiscoveryView, ChannelRevisionView, ChannelView, ContextBudgetAction, ContextCountSource, ContextPresetPreviewView, ContextPresetVersionView, ContextPresetView } from "./config-types";
import type { JsonObject } from "./graph-types";

const list = <T>(value: unknown, path: string, decode: (value: unknown, path: string) => T): T[] => {
  if (!Array.isArray(value)) throw new DecodeError(path);
  return value.map((item, index) => decode(item, `${path}[${index}]`));
};

const channel = (value: unknown, path: string): ChannelView => {
  const item = record(value, path);
  return { id: string(item.id, `${path}.id`), name: string(item.name, `${path}.name`), headRevisionId: nullableString(item.headRevisionId, `${path}.headRevisionId`), createdAt: number(item.createdAt, `${path}.createdAt`), updatedAt: number(item.updatedAt, `${path}.updatedAt`) };
};

const preset = (value: unknown, path: string): ContextPresetView => {
  const item = record(value, path);
  return { id: string(item.id, `${path}.id`), name: string(item.name, `${path}.name`), headVersionId: nullableString(item.headVersionId, `${path}.headVersionId`), createdAt: number(item.createdAt, `${path}.createdAt`), updatedAt: number(item.updatedAt, `${path}.updatedAt`) };
};

export const decodeChannels = (value: unknown): ChannelView[] => list(value, "channels", channel);
export const decodeChannel = (value: unknown): ChannelView => channel(value, "channel");
export const decodeContextPresets = (value: unknown): ContextPresetView[] => list(value, "contextPresets", preset);
export const decodeContextPreset = (value: unknown): ContextPresetView => preset(value, "contextPreset");

export const decodeChannelRevision = (value: unknown): ChannelRevisionView => {
  const item = record(value, "channelRevision");
  const operationTaxonomyVersion = number(
    item.operationTaxonomyVersion,
    "channelRevision.operationTaxonomyVersion",
  );
  const adapterDecoderVersion = number(
    item.adapterDecoderVersion,
    "channelRevision.adapterDecoderVersion",
  );
  if (operationTaxonomyVersion !== 1 || adapterDecoderVersion !== 1) {
    throw new DecodeError("channelRevision.version");
  }
  const operationKeys = jsonObjects(item.operationKeys, "channelRevision.operationKeys");
  const capabilities = jsonObjects(item.capabilities, "channelRevision.capabilities");
  if (!Array.isArray(item.modelCatalogs)) throw new DecodeError("channelRevision.modelCatalogs");
  return {
    id: string(item.id, "channelRevision.id"),
    channelId: string(item.channelId, "channelRevision.channelId"),
    revisionNo: number(item.revisionNo, "channelRevision.revisionNo"),
    operationTaxonomyVersion,
    adapterDecoderVersion,
    baseUrl: string(item.baseUrl, "channelRevision.baseUrl"),
    transportPolicy: jsonObject(item.transportPolicy, "channelRevision.transportPolicy"),
    credential: jsonObject(item.credential, "channelRevision.credential"),
    operationKeys,
    modelCatalogs: item.modelCatalogs.map((raw, index) => {
      const path = `channelRevision.modelCatalogs[${index}]`;
      const catalog = record(raw, path);
      const policy = string(catalog.policy, `${path}.policy`);
      if (policy !== "open" && policy !== "allowlist") throw new DecodeError(`${path}.policy`);
      return {
        operationKey: jsonObject(catalog.operationKey, `${path}.operationKey`),
        policy,
        models: jsonObjects(catalog.models, `${path}.models`),
      };
    }),
    capabilities,
    contentHash: string(item.contentHash, "channelRevision.contentHash"),
    createdAt: number(item.createdAt, "channelRevision.createdAt"),
  };
};

export const decodeChannelModelDiscovery = (value: unknown): ChannelModelDiscoveryView => {
  const item = record(value, "channelModelDiscovery");
  const operationKey = jsonObject(item.operationKey, "channelModelDiscovery.operationKey");
  return {
    channelId: string(item.channelId, "channelModelDiscovery.channelId"),
    channelRevisionId: string(item.channelRevisionId, "channelModelDiscovery.channelRevisionId"),
    operationKey,
    models: list(item.models, "channelModelDiscovery.models", (raw, path) => {
      const model = record(raw, path);
      return {
        id: string(model.id, `${path}.id`),
        name: nullableString(model.name, `${path}.name`),
        contextWindow: model.contextWindow === null ? null : number(model.contextWindow, `${path}.contextWindow`),
        maxOutputTokens: model.maxOutputTokens === null ? null : number(model.maxOutputTokens, `${path}.maxOutputTokens`),
      };
    }),
  };
};

const jsonObjects = (value: unknown, path: string): JsonObject[] => {
  if (!Array.isArray(value)) throw new DecodeError(path);
  return value.map((item, index) => jsonObject(item, `${path}[${index}]`));
};

export const decodeContextPresetVersion = (value: unknown): ContextPresetVersionView => {
  const item = record(value, "contextPresetVersion");
  const spec = record(item.spec, "contextPresetVersion.spec") as JsonObject;
  const semanticPolicyVersion = number(
    item.semanticPolicyVersion,
    "contextPresetVersion.semanticPolicyVersion",
  );
  if (semanticPolicyVersion !== 1) {
    throw new DecodeError("contextPresetVersion.semanticPolicyVersion");
  }
  return {
    id: string(item.id, "contextPresetVersion.id"),
    presetId: string(item.presetId, "contextPresetVersion.presetId"),
    versionNo: number(item.versionNo, "contextPresetVersion.versionNo"),
    semanticPolicyVersion,
    spec,
    contentHash: string(item.contentHash, "contextPresetVersion.contentHash"),
    createdAt: number(item.createdAt, "contextPresetVersion.createdAt"),
  };
};

const actions = new Set<ContextBudgetAction>(["kept", "dropped", "truncated", "deduped", "unsupported"]);
const countSources = new Set<ContextCountSource>(["provider", "local", "estimate"]);

export const decodeContextPresetPreview = (value: unknown): ContextPresetPreviewView => {
  const item = record(value, "contextPresetPreview");
  const contentMode = string(item.contentMode, "contextPresetPreview.contentMode");
  if (contentMode !== "metadata_only") throw new DecodeError("contextPresetPreview.contentMode");
  const countSource = decodeCountSource(item.countSource, "contextPresetPreview.countSource");
  if (!Array.isArray(item.items)) throw new DecodeError("contextPresetPreview.items");
  const budget = record(item.budgetReport, "contextPresetPreview.budgetReport");
  if (!Array.isArray(budget.items)) throw new DecodeError("contextPresetPreview.budgetReport.items");
  const snapshot = record(item.snapshot, "contextPresetPreview.snapshot");
  const budgetCountSource = decodeCountSource(budget.countSource, "contextPresetPreview.budgetReport.countSource");
  if (budgetCountSource !== countSource) throw new DecodeError("contextPresetPreview.countSource");
  return {
    presetId: string(item.presetId, "contextPresetPreview.presetId"),
    versionId: string(item.versionId, "contextPresetPreview.versionId"),
    contentMode,
    countSource,
    items: item.items.map((raw, index) => previewItem(raw, `contextPresetPreview.items[${index}]`)),
    budgetReport: {
      availableInputTokens: nonNegative(budget.availableInputTokens, "contextPresetPreview.budgetReport.availableInputTokens"),
      fixedRequestTokens: nonNegative(budget.fixedRequestTokens, "contextPresetPreview.budgetReport.fixedRequestTokens"),
      assembledTokens: nonNegative(budget.assembledTokens, "contextPresetPreview.budgetReport.assembledTokens"),
      countSource: budgetCountSource,
      items: budget.items.map((raw, index) => budgetItem(raw, `contextPresetPreview.budgetReport.items[${index}]`)),
    },
    snapshot: {
      config: record(snapshot.config, "contextPresetPreview.snapshot.config") as JsonObject,
      readSetRef: string(snapshot.readSetRef, "contextPresetPreview.snapshot.readSetRef"),
      readSetDigest: string(snapshot.readSetDigest, "contextPresetPreview.snapshot.readSetDigest"),
      resolvedBindingsDigest: string(snapshot.resolvedBindingsDigest, "contextPresetPreview.snapshot.resolvedBindingsDigest"),
      assemblyDigest: string(snapshot.assemblyDigest, "contextPresetPreview.snapshot.assemblyDigest"),
    },
  };
};

const previewItem = (value: unknown, path: string) => {
  const item = record(value, path);
  return {
    itemId: string(item.itemId, `${path}.itemId`),
    name: nullableString(item.name, `${path}.name`),
    sourceType: string(item.sourceType, `${path}.sourceType`),
    requestedRole: string(item.requestedRole, `${path}.requestedRole`),
    enabled: boolean(item.enabled, `${path}.enabled`),
    included: boolean(item.included, `${path}.included`),
    tokenCount: nonNegative(item.tokenCount, `${path}.tokenCount`),
    action: decodeAction(item.action, `${path}.action`),
    reason: nullableString(item.reason, `${path}.reason`),
  };
};

const budgetItem = (value: unknown, path: string) => {
  const item = record(value, path);
  return {
    itemId: string(item.itemId, `${path}.itemId`),
    included: boolean(item.included, `${path}.included`),
    tokenCount: nonNegative(item.tokenCount, `${path}.tokenCount`),
    action: decodeAction(item.action, `${path}.action`),
    reason: nullableString(item.reason, `${path}.reason`),
  };
};

const decodeAction = (value: unknown, path: string) => {
  const action = string(value, path) as ContextBudgetAction;
  if (!actions.has(action)) throw new DecodeError(path);
  return action;
};

const decodeCountSource = (value: unknown, path: string) => {
  const source = string(value, path) as ContextCountSource;
  if (!countSources.has(source)) throw new DecodeError(path);
  return source;
};

const nonNegative = (value: unknown, path: string) => {
  const parsed = number(value, path);
  if (parsed < 0) throw new DecodeError(path);
  return parsed;
};
