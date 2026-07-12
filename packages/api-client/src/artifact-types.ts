import type { ArtifactRef } from "./types";

export type ArtifactClassification = "public" | "private" | "sensitive";

export type ArtifactRetention =
  | { type: "ephemeral"; expiresAt: number }
  | { type: "run" }
  | { type: "context" }
  | { type: "pinned" }
  | { type: "audit_until"; timestamp: number };

export interface ArtifactMetadata {
  artifactId: string;
  content: ArtifactRef;
  name: string | null;
  classification: ArtifactClassification;
  status: "active" | "deleted";
  originRunId: string | null;
  originNodeInstanceId: string | null;
  originToolCallId: string | null;
  retention: ArtifactRetention;
  createdAt: number;
}

export interface ArtifactView {
  metadata: ArtifactMetadata;
  metadataHeadCommitId: string;
}

export interface ArtifactListView {
  items: ArtifactView[];
}

export type ArtifactStagingStatus =
  | "uploading"
  | "staged"
  | "validated"
  | "quarantined"
  | "deleting"
  | "deleted"
  | "committed";

export interface ArtifactStagingView {
  stagingId: string;
  status: ArtifactStagingStatus;
  lifecycleGeneration: number;
  byteSize: number | null;
  contentHash: string | null;
  validatedMediaType: string | null;
}

export interface UploadArtifactInput {
  object: Blob;
  name?: string | null;
  classification: ArtifactClassification;
  retention: ArtifactRetention;
  contextId?: string | null;
  declaredMediaType?: string | null;
}
