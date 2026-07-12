import type { JsonObject } from "./graph-types";

export type MemoryProposalStatus = "proposed" | "awaiting_confirmation" | "awaiting_review" | "approved" | "rejected" | "applied" | "conflicted";
export type MemoryChangeType = "create" | "replace_content" | "mark_obsolete" | "delete_tombstone";
export type MemoryRecordStatus = "reserved" | "active" | "obsolete" | "deleted" | "discarded";

export interface MemoryContent {
  schemaVersion: 1;
  text: string;
  tags: string[];
  attributes: JsonObject;
}

export interface MemoryProposalView {
  id: string;
  scopeId: string;
  memoryId: string;
  expectedHeadCommitId: string | null;
  changeType: MemoryChangeType;
  contentRef: string | null;
  proposedContent: MemoryContent | null;
  reason: string;
  evidenceRefs: string[];
  requestedBy: { kind: string; id: string | null };
  schemaVersion: number;
  policyVersion: number;
  originRunId: string | null;
  originNodeInstanceId: string | null;
  appliedCommitId: string | null;
  status: MemoryProposalStatus;
  createdAt: number;
  updatedAt: number;
}

export interface MemoryProposalCursor { updatedAt: number; id: string }
export interface MemoryProposalListView { proposals: MemoryProposalView[]; nextCursor: MemoryProposalCursor | null }

export interface MemoryRecordView {
  id: string;
  scopeId: string;
  status: MemoryRecordStatus;
  headCommitId: string | null;
  contentRef: string | null;
  content: MemoryContent | null;
  createdAt: number;
  updatedAt: number;
}

export interface MemorySearchView {
  records: MemoryRecordView[];
  truncated: boolean;
  scopeSnapshotToken: string;
}

export type MemoryChangeInput =
  | { type: "create"; content: MemoryContent }
  | { type: "replace_content"; content: MemoryContent }
  | { type: "mark_obsolete" }
  | { type: "delete_tombstone" };

export interface ProposeMemoryInput {
  scopeId: string;
  memoryId: string | null;
  expectedHeadCommitId: string | null;
  change: MemoryChangeInput;
  reason: string;
  evidenceRefs: string[];
  idempotencyKey?: string;
}
