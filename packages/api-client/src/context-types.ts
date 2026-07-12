import type { JsonValue } from "./graph-types";
import type { ArtifactRef } from "./types";

export type ContextBranchStatus = "active" | "merged" | "abandoned";
export type ContextActorKind = "user" | "system" | "node" | "tool" | "application";

export interface ContextBranchView {
  contextId: string;
  branchId: string;
  headCommitId: string;
  forkCommitId: string;
  status: ContextBranchStatus;
}

export interface ContextCommitView {
  id: string;
  contextId: string;
  branchId: string;
  sequenceNo: number;
  operationId: string;
  parentCommitIds: string[];
  patchRef: string | null;
  schemaVersion: number;
  policyVersion: number;
  author: { kind: ContextActorKind; id: string | null };
  originRunId: string | null;
  originNodeInstanceId: string | null;
  createdAt: number;
}

export interface ContextDiffEntry {
  path: string;
  before: JsonValue;
  after: JsonValue;
}

export interface ContextDiffView {
  contextId: string;
  fromCommitId: string;
  toCommitId: string;
  changes: ContextDiffEntry[];
}

export interface ForkContextInput {
  sourceBranchId: string;
  fromCommitId: string;
  expectedSourceHead?: string;
}

export type MergeSourceDisposition = "mark_merged" | "keep_active";

export type ExplicitMergeResolution =
  | { type: "final_value"; value: JsonValue }
  | { type: "artifact_ref"; artifactRef: ArtifactRef };

export interface ExplicitMergeSelection {
  conflictId: string;
  path: string;
  resolution: ExplicitMergeResolution;
}

export interface MergeContextInput {
  sourceBranchId: string;
  targetBranchId: string;
  expectedSourceHead: string;
  expectedTargetHead: string;
  sourceDisposition: MergeSourceDisposition;
  selections?: ExplicitMergeSelection[];
}

export interface MergeConflictView {
  conflictId: string;
  path: string;
  base: JsonValue;
  source: JsonValue;
  target: JsonValue;
}

export interface MergeContextView {
  contextId: string;
  sourceBranchId: string;
  targetBranchId: string;
  baseCommitId: string;
  sourceHeadCommitId: string;
  targetHeadCommitId: string;
  status: "conflicted" | "merged";
  conflicts: MergeConflictView[];
  mergeCommitId: string | null;
}
