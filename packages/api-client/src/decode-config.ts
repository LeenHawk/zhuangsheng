import { nullableString, number, record, string } from "./decode-helpers";
import { DecodeError } from "./decode-error";
import type { ChannelRevisionView, ChannelView, ContextPresetVersionView, ContextPresetView } from "./config-types";
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
  return {
    id: string(item.id, "channelRevision.id"),
    channelId: string(item.channelId, "channelRevision.channelId"),
    revisionNo: number(item.revisionNo, "channelRevision.revisionNo"),
    operationTaxonomyVersion,
    adapterDecoderVersion,
    baseUrl: string(item.baseUrl, "channelRevision.baseUrl"),
    contentHash: string(item.contentHash, "channelRevision.contentHash"),
    createdAt: number(item.createdAt, "channelRevision.createdAt"),
  };
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
