import type { JsonValue } from "./graph-types";

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
