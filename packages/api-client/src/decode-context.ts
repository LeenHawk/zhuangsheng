import { DecodeError } from "./decode-error";
import { jsonValue, nullableString, number, record, string, stringArray } from "./decode-helpers";
import type {
  ContextActorKind,
  ContextBranchStatus,
  ContextBranchView,
  ContextCommitView,
  ContextDiffView,
  MergeContextView,
} from "./context-types";

const branchStatuses = new Set<ContextBranchStatus>(["active", "merged", "abandoned"]);
const actorKinds = new Set<ContextActorKind>(["user", "system", "node", "tool", "application"]);

export const decodeContextBranch = (value: unknown, path = "contextBranch"): ContextBranchView => {
  const item = record(value, path);
  const status = string(item.status, `${path}.status`) as ContextBranchStatus;
  if (!branchStatuses.has(status)) throw new DecodeError(`${path}.status`);
  return {
    contextId: string(item.contextId, `${path}.contextId`),
    branchId: string(item.branchId, `${path}.branchId`),
    headCommitId: string(item.headCommitId, `${path}.headCommitId`),
    forkCommitId: string(item.forkCommitId, `${path}.forkCommitId`),
    status,
  };
};

const commit = (value: unknown, path: string): ContextCommitView => {
  const item = record(value, path);
  const rawAuthor = record(item.author, `${path}.author`);
  const kind = string(rawAuthor.kind, `${path}.author.kind`) as ContextActorKind;
  if (!actorKinds.has(kind)) throw new DecodeError(`${path}.author.kind`);
  return {
    id: string(item.id, `${path}.id`),
    contextId: string(item.contextId, `${path}.contextId`),
    branchId: string(item.branchId, `${path}.branchId`),
    sequenceNo: number(item.sequenceNo, `${path}.sequenceNo`),
    operationId: string(item.operationId, `${path}.operationId`),
    parentCommitIds: stringArray(item.parentCommitIds, `${path}.parentCommitIds`),
    patchRef: nullableString(item.patchRef, `${path}.patchRef`),
    schemaVersion: number(item.schemaVersion, `${path}.schemaVersion`),
    policyVersion: number(item.policyVersion, `${path}.policyVersion`),
    author: { kind, id: nullableString(rawAuthor.id, `${path}.author.id`) },
    originRunId: nullableString(item.originRunId, `${path}.originRunId`),
    originNodeInstanceId: nullableString(
      item.originNodeInstanceId,
      `${path}.originNodeInstanceId`,
    ),
    createdAt: number(item.createdAt, `${path}.createdAt`),
  };
};

export const decodeContextBranches = (value: unknown): ContextBranchView[] => {
  if (!Array.isArray(value)) throw new DecodeError("contextBranches");
  return value.map((item, index) => decodeContextBranch(item, `contextBranches[${index}]`));
};

export const decodeContextCommits = (value: unknown): ContextCommitView[] => {
  if (!Array.isArray(value)) throw new DecodeError("contextCommits");
  return value.map((item, index) => commit(item, `contextCommits[${index}]`));
};

export const decodeContextDiff = (value: unknown): ContextDiffView => {
  const path = "contextDiff";
  const item = record(value, path);
  if (!Array.isArray(item.changes)) throw new DecodeError(`${path}.changes`);
  return {
    contextId: string(item.contextId, `${path}.contextId`),
    fromCommitId: string(item.fromCommitId, `${path}.fromCommitId`),
    toCommitId: string(item.toCommitId, `${path}.toCommitId`),
    changes: item.changes.map((raw, index) => {
      const changePath = `${path}.changes[${index}]`;
      const change = record(raw, changePath);
      return {
        path: string(change.path, `${changePath}.path`),
        before: jsonValue(change.before, `${changePath}.before`),
        after: jsonValue(change.after, `${changePath}.after`),
      };
    }),
  };
};

export const decodeMergeContext = (value: unknown): MergeContextView => {
  const path = "contextMerge";
  const item = record(value, path);
  const status = string(item.status, `${path}.status`);
  if (status !== "conflicted" && status !== "merged") throw new DecodeError(`${path}.status`);
  if (!Array.isArray(item.conflicts)) throw new DecodeError(`${path}.conflicts`);
  const conflicts = item.conflicts.map((raw, index) => {
    const conflictPath = `${path}.conflicts[${index}]`;
    const conflict = record(raw, conflictPath);
    return {
      conflictId: string(conflict.conflictId, `${conflictPath}.conflictId`),
      path: string(conflict.path, `${conflictPath}.path`),
      base: jsonValue(conflict.base, `${conflictPath}.base`),
      source: jsonValue(conflict.source, `${conflictPath}.source`),
      target: jsonValue(conflict.target, `${conflictPath}.target`),
    };
  });
  const mergeCommitId = nullableString(item.mergeCommitId, `${path}.mergeCommitId`);
  if ((status === "merged" && (mergeCommitId === null || conflicts.length > 0))
    || (status === "conflicted" && (mergeCommitId !== null || conflicts.length === 0))) {
    throw new DecodeError(`${path}.status`);
  }
  return {
    contextId: string(item.contextId, `${path}.contextId`),
    sourceBranchId: string(item.sourceBranchId, `${path}.sourceBranchId`),
    targetBranchId: string(item.targetBranchId, `${path}.targetBranchId`),
    baseCommitId: string(item.baseCommitId, `${path}.baseCommitId`),
    sourceHeadCommitId: string(item.sourceHeadCommitId, `${path}.sourceHeadCommitId`),
    targetHeadCommitId: string(item.targetHeadCommitId, `${path}.targetHeadCommitId`),
    status,
    conflicts,
    mergeCommitId,
  };
};
