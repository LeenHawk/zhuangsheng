import {
  decodeContextBranch,
  decodeContextBranches,
  decodeContextCommits,
  decodeContextDiff,
  decodeMergeContext,
  decodeContextCommit,
  decodeVersionSnapshot,
  decodeWorkingContext,
} from "./decode-context";
import { DecodeError } from "./decode-error";
import type {
  ContextBranchView,
  ContextCommitView,
  ContextDiffView,
  ForkContextInput,
  MergeContextInput,
  MergeContextView,
  CommitContextPatchInput,
  CreateVersionSnapshotInput,
  VersionSnapshotView,
  WorkingContextView,
} from "./context-types";
import { requestJson } from "./http-json";
import { createIdempotencyKey } from "./idempotency";

export interface ContextCommandOptions {
  idempotencyKey?: string;
  signal?: AbortSignal;
}

export class HttpContextClient {
  constructor(private readonly baseUrl = "") {}

  async listBranches(contextId: string, signal?: AbortSignal): Promise<ContextBranchView[]> {
    return decodeContextBranches(await requestJson(
      this.baseUrl,
      `/v1/contexts/${encodeURIComponent(contextId)}/branches`,
      { signal },
    ));
  }

  async listCommits(contextId: string, signal?: AbortSignal): Promise<ContextCommitView[]> {
    return decodeContextCommits(await requestJson(
      this.baseUrl,
      `/v1/contexts/${encodeURIComponent(contextId)}/commits`,
      { signal },
    ));
  }

  async getBranch(contextId: string, branchId: string, signal?: AbortSignal): Promise<WorkingContextView> {
    const result = decodeWorkingContext(await requestJson(
      this.baseUrl,
      `/v1/contexts/${encodeURIComponent(contextId)}/branches/${encodeURIComponent(branchId)}`,
      { signal },
    ));
    if (result.contextId !== contextId || result.branchId !== branchId) throw new DecodeError("workingContext");
    return result;
  }

  async getCommit(commitId: string, signal?: AbortSignal): Promise<WorkingContextView> {
    const result = decodeWorkingContext(await requestJson(
      this.baseUrl,
      `/v1/context-commits/${encodeURIComponent(commitId)}`,
      { signal },
    ));
    if (result.headCommitId !== commitId) throw new DecodeError("workingContext.headCommitId");
    return result;
  }

  async commitPatch(
    contextId: string,
    branchId: string,
    input: CommitContextPatchInput,
    signal?: AbortSignal,
  ): Promise<ContextCommitView> {
    const result = decodeContextCommit(await requestJson(
      this.baseUrl,
      `/v1/contexts/${encodeURIComponent(contextId)}/branches/${encodeURIComponent(branchId)}/commits`,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          patch: {
            aggregateKind: "working_context", aggregateId: contextId, lineageKey: branchId,
            baseCommitId: input.baseCommitId, operationId: input.operationId,
            ops: input.ops.map((operation) => operation.op === "append"
              ? { ...operation, element_id: operation.elementId, elementId: undefined }
              : operation),
            schemaVersion: input.schemaVersion, policyVersion: input.policyVersion, author: input.author,
          },
          originRunId: input.originRunId ?? null,
          originNodeInstanceId: input.originNodeInstanceId ?? null,
        }),
        signal,
      },
    ));
    if (result.contextId !== contextId || result.branchId !== branchId || result.operationId !== input.operationId) {
      throw new DecodeError("contextCommit");
    }
    return result;
  }

  async createSnapshot(
    commitId: string,
    input: CreateVersionSnapshotInput,
    signal?: AbortSignal,
  ): Promise<VersionSnapshotView> {
    const result = decodeVersionSnapshot(await requestJson(
      this.baseUrl,
      `/v1/context-commits/${encodeURIComponent(commitId)}/snapshot`,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ retentionUntil: input.retentionUntil ?? null, pinned: input.pinned }),
        signal,
      },
    ));
    if (result.commitId !== commitId) throw new DecodeError("versionSnapshot.commitId");
    return result;
  }

  async fork(
    contextId: string,
    input: ForkContextInput,
    options: ContextCommandOptions = {},
  ): Promise<ContextBranchView> {
    const result = decodeContextBranch(await requestJson(
      this.baseUrl,
      `/v1/contexts/${encodeURIComponent(contextId)}/branches`,
      {
        method: "POST",
        headers: this.commandHeaders(options.idempotencyKey),
        body: JSON.stringify(input),
        signal: options.signal,
      },
    ));
    if (result.contextId !== contextId || result.forkCommitId !== input.fromCommitId
      || result.headCommitId !== input.fromCommitId || result.status !== "active") {
      throw new DecodeError("contextBranch");
    }
    return result;
  }

  async merge(
    contextId: string,
    input: MergeContextInput,
    options: ContextCommandOptions = {},
  ): Promise<MergeContextView> {
    const result = decodeMergeContext(await requestJson(
      this.baseUrl,
      `/v1/contexts/${encodeURIComponent(contextId)}/merges`,
      {
        method: "POST",
        headers: this.commandHeaders(options.idempotencyKey),
        body: JSON.stringify({ ...input, selections: input.selections ?? [] }),
        signal: options.signal,
      },
    ));
    if (result.contextId !== contextId
      || result.sourceBranchId !== input.sourceBranchId
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
    signal?: AbortSignal,
  ): Promise<ContextDiffView> {
    const query = new URLSearchParams({ from: fromCommitId, to: toCommitId });
    return decodeContextDiff(await requestJson(
      this.baseUrl,
      `/v1/contexts/${encodeURIComponent(contextId)}/diff?${query.toString()}`,
      { signal },
    ));
  }

  private commandHeaders(key?: string): Record<string, string> {
    return { "content-type": "application/json", "idempotency-key": key ?? createIdempotencyKey() };
  }
}
