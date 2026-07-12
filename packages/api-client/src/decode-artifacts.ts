import { DecodeError } from "./decode-error";
import { nullableString, number, record, string } from "./decode-helpers";
import type {
  ArtifactClassification,
  ArtifactListView,
  ArtifactRetention,
  ArtifactStagingStatus,
  ArtifactStagingView,
  ArtifactView,
} from "./artifact-types";

const classifications = new Set<ArtifactClassification>(["public", "private", "sensitive"]);
const stagingStatuses = new Set<ArtifactStagingStatus>([
  "uploading", "staged", "validated", "quarantined", "deleting", "deleted", "committed",
]);

export const decodeArtifact = (value: unknown, path = "artifact"): ArtifactView => {
  const item = record(value, path);
  const metadata = record(item.metadata, `${path}.metadata`);
  const content = record(metadata.content, `${path}.metadata.content`);
  const classification = string(metadata.classification, `${path}.metadata.classification`) as ArtifactClassification;
  if (!classifications.has(classification)) throw new DecodeError(`${path}.metadata.classification`);
  const status = string(metadata.status, `${path}.metadata.status`);
  if (status !== "active" && status !== "deleted") throw new DecodeError(`${path}.metadata.status`);
  const retention = decodeRetention(metadata.retention, `${path}.metadata.retention`);
  const artifactId = string(metadata.artifactId, `${path}.metadata.artifactId`);
  const refId = string(content.artifactId, `${path}.metadata.content.artifactId`);
  if (artifactId !== refId) throw new DecodeError(`${path}.metadata.content.artifactId`);
  const contentHash = string(content.contentHash, `${path}.metadata.content.contentHash`);
  if (!/^sha256:[0-9a-f]{64}$/.test(contentHash)) {
    throw new DecodeError(`${path}.metadata.content.contentHash`);
  }
  const mediaType = string(content.mediaType, `${path}.metadata.content.mediaType`);
  if (!mediaType.includes("/") || mediaType.length > 128) {
    throw new DecodeError(`${path}.metadata.content.mediaType`);
  }
  return {
    metadata: {
      artifactId,
      content: {
        artifactId: refId,
        contentHash,
        byteSize: positive(content.byteSize, `${path}.metadata.content.byteSize`),
        mediaType,
      },
      name: nullableString(metadata.name, `${path}.metadata.name`),
      classification,
      status,
      originRunId: nullableString(metadata.originRunId, `${path}.metadata.originRunId`),
      originNodeInstanceId: nullableString(metadata.originNodeInstanceId, `${path}.metadata.originNodeInstanceId`),
      originToolCallId: nullableString(metadata.originToolCallId, `${path}.metadata.originToolCallId`),
      retention,
      createdAt: number(metadata.createdAt, `${path}.metadata.createdAt`),
    },
    metadataHeadCommitId: string(item.metadataHeadCommitId, `${path}.metadataHeadCommitId`),
  };
};

export const decodeArtifactList = (value: unknown): ArtifactListView => {
  const item = record(value, "artifactList");
  if (!Array.isArray(item.items)) throw new DecodeError("artifactList.items");
  return { items: item.items.map((artifact, index) => decodeArtifact(artifact, `artifactList.items[${index}]`)) };
};

export const decodeArtifactStaging = (value: unknown): ArtifactStagingView => {
  const item = record(value, "artifactStaging");
  const status = string(item.status, "artifactStaging.status") as ArtifactStagingStatus;
  if (!stagingStatuses.has(status)) throw new DecodeError("artifactStaging.status");
  return {
    stagingId: string(item.stagingId, "artifactStaging.stagingId"),
    status,
    lifecycleGeneration: nonNegative(item.lifecycleGeneration, "artifactStaging.lifecycleGeneration"),
    byteSize: item.byteSize === null ? null : nonNegative(item.byteSize, "artifactStaging.byteSize"),
    contentHash: nullableString(item.contentHash, "artifactStaging.contentHash"),
    validatedMediaType: nullableString(item.validatedMediaType, "artifactStaging.validatedMediaType"),
  };
};

const decodeRetention = (value: unknown, path: string): ArtifactRetention => {
  const item = record(value, path);
  const type = string(item.type, `${path}.type`);
  if (type === "ephemeral") return { type, expiresAt: number(item.expiresAt, `${path}.expiresAt`) };
  if (type === "audit_until") return { type, timestamp: number(item.timestamp, `${path}.timestamp`) };
  if (type === "run" || type === "context" || type === "pinned") return { type };
  throw new DecodeError(`${path}.type`);
};

const nonNegative = (value: unknown, path: string) => {
  const parsed = number(value, path);
  if (parsed < 0) throw new DecodeError(path);
  return parsed;
};

const positive = (value: unknown, path: string) => {
  const parsed = number(value, path);
  if (parsed <= 0) throw new DecodeError(path);
  return parsed;
};
