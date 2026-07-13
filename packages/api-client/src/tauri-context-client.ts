import {
  decodeContextBranch,
  decodeContextBranches,
  decodeContextCommit,
  decodeContextCommits,
  decodeContextDiff,
  decodeMergeContext,
  decodeVersionSnapshot,
  decodeWorkingContext,
} from "./decode-context";
import { DecodeError } from "./decode-error";
import { createIdempotencyKey } from "./idempotency";
import type {
  CommitContextPatchInput,
  ContextBranchView,
  ContextCommitView,
  ContextDiffView,
  CreateVersionSnapshotInput,
  ForkContextInput,
  MergeContextInput,
  MergeContextView,
  VersionSnapshotView,
  WorkingContextView,
} from "./context-types";
import type { TauriBridge } from "./transport";

export class TauriContextClient {
  constructor(private readonly bridge: TauriBridge) {}

  async listBranches(contextId: string): Promise<ContextBranchView[]> {
    return decodeContextBranches(await this.bridge.invoke("list_context_branches", { contextId }));
  }

  async listCommits(contextId: string): Promise<ContextCommitView[]> {
    return decodeContextCommits(await this.bridge.invoke("list_context_commits", { contextId }));
  }

  async getBranch(contextId: string, branchId: string): Promise<WorkingContextView> {
    const result = decodeWorkingContext(await this.bridge.invoke("get_working_context", {
      contextId, branchId,
    }));
    if (result.contextId !== contextId || result.branchId !== branchId) {
      throw new DecodeError("workingContext");
    }
    return result;
  }

  async getCommit(commitId: string): Promise<WorkingContextView> {
    const result = decodeWorkingContext(await this.bridge.invoke("get_context_at_commit", { commitId }));
    if (result.headCommitId !== commitId) throw new DecodeError("workingContext.headCommitId");
    return result;
  }

  async commitPatch(
    contextId: string,
    branchId: string,
    input: CommitContextPatchInput,
  ): Promise<ContextCommitView> {
    const result = decodeContextCommit(await this.bridge.invoke("commit_context_patch", { command: {
      patch: {
        aggregateKind: "working_context",
        aggregateId: contextId,
        lineageKey: branchId,
        baseCommitId: input.baseCommitId,
        operationId: input.operationId,
        ops: input.ops.map((operation) => operation.op === "append"
          ? { ...operation, element_id: operation.elementId, elementId: undefined }
          : operation),
        schemaVersion: input.schemaVersion,
        policyVersion: input.policyVersion,
        author: { kind: "user", id: "local-user" },
      },
      originRunId: input.originRunId ?? null,
      originNodeInstanceId: input.originNodeInstanceId ?? null,
    } }));
    if (result.contextId !== contextId || result.branchId !== branchId
      || result.operationId !== input.operationId) {
      throw new DecodeError("contextCommit");
    }
    return result;
  }

  async createSnapshot(
    commitId: string,
    input: CreateVersionSnapshotInput,
  ): Promise<VersionSnapshotView> {
    const result = decodeVersionSnapshot(await this.bridge.invoke("create_version_snapshot", { command: {
      commitId, retentionUntil: input.retentionUntil ?? null, pinned: input.pinned,
    } }));
    if (result.commitId !== commitId) throw new DecodeError("versionSnapshot.commitId");
    return result;
  }

  async fork(
    contextId: string,
    input: ForkContextInput,
    idempotencyKey = createIdempotencyKey(),
  ): Promise<ContextBranchView> {
    const result = decodeContextBranch(await this.bridge.invoke("fork_context", { command: {
      contextId, sourceBranchId: input.sourceBranchId, fromCommitId: input.fromCommitId,
      expectedSourceHead: input.expectedSourceHead ?? null, idempotencyKey,
    } }));
    if (result.contextId !== contextId || result.forkCommitId !== input.fromCommitId
      || result.headCommitId !== input.fromCommitId || result.status !== "active") {
      throw new DecodeError("contextBranch");
    }
    return result;
  }

  async merge(
    contextId: string,
    input: MergeContextInput,
    idempotencyKey = createIdempotencyKey(),
  ): Promise<MergeContextView> {
    const result = decodeMergeContext(await this.bridge.invoke("merge_context", { command: {
      contextId, ...input, selections: input.selections ?? [], idempotencyKey,
    } }));
    if (result.contextId !== contextId || result.sourceBranchId !== input.sourceBranchId
      || result.targetBranchId !== input.targetBranchId
      || result.sourceHeadCommitId !== input.expectedSourceHead
      || result.targetHeadCommitId !== input.expectedTargetHead) {
      throw new DecodeError("contextMerge");
    }
    return result;
  }

  async diff(
    contextId: string,
    fromCommitId: string,
    toCommitId: string,
  ): Promise<ContextDiffView> {
    return decodeContextDiff(await this.bridge.invoke("diff_context_commits", {
      contextId, fromCommitId, toCommitId,
    }));
  }
}
