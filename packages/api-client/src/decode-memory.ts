import { boolean, nullableString, number, record, string, stringArray } from "./decode-helpers";
import { DecodeError } from "./decode-error";
import type { JsonObject } from "./graph-types";
import type { MemoryContent, MemoryProposalListView, MemoryProposalStatus, MemoryProposalView, MemoryRecordStatus, MemoryRecordView, MemorySearchView } from "./memory-types";

const oneOf = <T extends string>(value: unknown, path: string, allowed: readonly T[]): T => {
  const decoded = string(value, path) as T;
  if (!allowed.includes(decoded)) throw new DecodeError(path);
  return decoded;
};
const proposalStatuses = ["proposed", "awaiting_confirmation", "awaiting_review", "approved", "rejected", "applied", "conflicted"] as const;
const recordStatuses = ["reserved", "active", "obsolete", "deleted", "discarded"] as const;

const content = (value: unknown, path: string): MemoryContent => {
  const item = record(value, path);
  const schemaVersion = number(item.schemaVersion, `${path}.schemaVersion`);
  if (schemaVersion !== 1) throw new DecodeError(`${path}.schemaVersion`);
  return { schemaVersion, text: string(item.text, `${path}.text`), tags: stringArray(item.tags, `${path}.tags`), attributes: record(item.attributes, `${path}.attributes`) as JsonObject };
};

const nullableContent = (value: unknown, path: string) => value === null ? null : content(value, path);

export const decodeMemoryProposal = (value: unknown, path = "memoryProposal"): MemoryProposalView => {
  const item = record(value, path); const actor = record(item.requestedBy, `${path}.requestedBy`);
  return {
    id: string(item.id, `${path}.id`), scopeId: string(item.scopeId, `${path}.scopeId`), memoryId: string(item.memoryId, `${path}.memoryId`),
    expectedHeadCommitId: nullableString(item.expectedHeadCommitId, `${path}.expectedHeadCommitId`),
    changeType: oneOf(item.changeType, `${path}.changeType`, ["create", "replace_content", "mark_obsolete", "delete_tombstone"] as const),
    contentRef: nullableString(item.contentRef, `${path}.contentRef`), proposedContent: nullableContent(item.proposedContent, `${path}.proposedContent`),
    reason: string(item.reason, `${path}.reason`), evidenceRefs: stringArray(item.evidenceRefs, `${path}.evidenceRefs`),
    requestedBy: { kind: string(actor.kind, `${path}.requestedBy.kind`), id: nullableString(actor.id, `${path}.requestedBy.id`) },
    schemaVersion: number(item.schemaVersion, `${path}.schemaVersion`), policyVersion: number(item.policyVersion, `${path}.policyVersion`),
    originRunId: nullableString(item.originRunId, `${path}.originRunId`), originNodeInstanceId: nullableString(item.originNodeInstanceId, `${path}.originNodeInstanceId`),
    appliedCommitId: nullableString(item.appliedCommitId, `${path}.appliedCommitId`), status: oneOf<MemoryProposalStatus>(item.status, `${path}.status`, proposalStatuses),
    createdAt: number(item.createdAt, `${path}.createdAt`), updatedAt: number(item.updatedAt, `${path}.updatedAt`),
  };
};

export const decodeMemoryProposalList = (value: unknown): MemoryProposalListView => {
  const item = record(value, "memoryProposals");
  if (!Array.isArray(item.proposals)) throw new DecodeError("memoryProposals.proposals");
  const cursor = item.nextCursor === null ? null : record(item.nextCursor, "memoryProposals.nextCursor");
  return { proposals: item.proposals.map((proposal, index) => decodeMemoryProposal(proposal, `memoryProposals.proposals[${index}]`)), nextCursor: cursor ? { updatedAt: number(cursor.updatedAt, "memoryProposals.nextCursor.updatedAt"), id: string(cursor.id, "memoryProposals.nextCursor.id") } : null };
};

const memoryRecord = (value: unknown, path: string): MemoryRecordView => {
  const item = record(value, path);
  return { id: string(item.id, `${path}.id`), scopeId: string(item.scopeId, `${path}.scopeId`), status: oneOf<MemoryRecordStatus>(item.status, `${path}.status`, recordStatuses), headCommitId: nullableString(item.headCommitId, `${path}.headCommitId`), contentRef: nullableString(item.contentRef, `${path}.contentRef`), content: nullableContent(item.content, `${path}.content`), createdAt: number(item.createdAt, `${path}.createdAt`), updatedAt: number(item.updatedAt, `${path}.updatedAt`) };
};

export const decodeMemorySearch = (value: unknown): MemorySearchView => {
  const item = record(value, "memorySearch"); if (!Array.isArray(item.records)) throw new DecodeError("memorySearch.records");
  return { records: item.records.map((value, index) => memoryRecord(value, `memorySearch.records[${index}]`)), truncated: boolean(item.truncated, "memorySearch.truncated"), scopeSnapshotToken: string(item.scopeSnapshotToken, "memorySearch.scopeSnapshotToken") };
};
