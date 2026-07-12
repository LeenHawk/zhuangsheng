import { afterEach, describe, expect, it, vi } from "vitest";

import { DecodeError } from "./decode-error";
import {
  decodeContextBranches,
  decodeContextCommits,
  decodeContextDiff,
  decodeMergeContext,
} from "./decode-context";
import { HttpContextClient } from "./http-context-client";

const branch = {
  contextId: "context/1",
  branchId: "branch_1",
  headCommitId: "commit_2",
  forkCommitId: "commit_1",
  status: "active",
};

const commit = {
  id: "commit_2",
  contextId: "context/1",
  branchId: "branch_1",
  sequenceNo: 2,
  operationId: "operation_2",
  parentCommitIds: ["commit_1"],
  patchRef: "object_1",
  schemaVersion: 1,
  policyVersion: 1,
  author: { kind: "node", id: "node_1" },
  originRunId: "run_1",
  originNodeInstanceId: "nodeinstance_1",
  createdAt: 2,
};

describe("context query decoders", () => {
  it("validates branch lifecycle and commit actor kinds", () => {
    expect(decodeContextBranches([branch])[0]?.status).toBe("active");
    expect(decodeContextCommits([commit])[0]?.author.kind).toBe("node");
    expect(() => decodeContextBranches([{ ...branch, status: "future" }])).toThrow(DecodeError);
    expect(() => decodeContextCommits([{ ...commit, author: { kind: "future", id: null } }]))
      .toThrow(DecodeError);
  });

  it("retains arbitrary JSON values in stable pointer changes", () => {
    const decoded = decodeContextDiff({
      contextId: "context/1",
      fromCommitId: "commit/1",
      toCommitId: "commit 2",
      changes: [{ path: "/lore/items", before: [1], after: [1, { kept: true }] }],
    });
    expect(decoded.changes[0]).toEqual({
      path: "/lore/items",
      before: [1],
      after: [1, { kept: true }],
    });
  });

  it("enforces merge status, conflict, and commit invariants", () => {
    expect(() => decodeMergeContext({
      contextId: "context_1",
      sourceBranchId: "source",
      targetBranchId: "target",
      baseCommitId: "base",
      sourceHeadCommitId: "source_head",
      targetHeadCommitId: "target_head",
      status: "merged",
      conflicts: [],
      mergeCommitId: null,
    })).toThrow(DecodeError);
  });
});

describe("HttpContextClient", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("encodes context and commit identifiers on canonical query routes", async () => {
    const calls: Array<RequestInfo | URL> = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      calls.push(input);
      const payload = calls.length === 1 ? [branch] : calls.length === 2 ? [commit] : {
        contextId: "context/1",
        fromCommitId: "commit/1",
        toCommitId: "commit 2",
        changes: [],
      };
      return new Response(JSON.stringify(payload), { status: 200 });
    });
    const client = new HttpContextClient("https://runtime.example");

    await client.listBranches("context/1");
    await client.listCommits("context/1");
    await client.diff("context/1", "commit/1", "commit 2");

    expect(calls).toEqual([
      "https://runtime.example/v1/contexts/context%2F1/branches",
      "https://runtime.example/v1/contexts/context%2F1/commits",
      "https://runtime.example/v1/contexts/context%2F1/diff?from=commit%2F1&to=commit+2",
    ]);
  });

  it("forks and merges with explicit CAS and stable idempotency keys", async () => {
    const calls: Array<{ input: RequestInfo | URL; init?: RequestInit }> = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ input, init });
      const payload = calls.length === 1 ? {
        ...branch,
        headCommitId: "commit_1",
      } : {
        contextId: "context/1",
        sourceBranchId: "branch/source",
        targetBranchId: "branch target",
        baseCommitId: "commit_base",
        sourceHeadCommitId: "commit_source",
        targetHeadCommitId: "commit_target",
        status: "merged",
        conflicts: [],
        mergeCommitId: "commit_merge",
      };
      return Response.json(payload);
    });
    const client = new HttpContextClient("https://runtime.example");

    await client.fork("context/1", {
      sourceBranchId: "branch_root",
      fromCommitId: "commit_1",
      expectedSourceHead: "commit_1",
    }, { idempotencyKey: "fork-key" });
    await client.merge("context/1", {
      sourceBranchId: "branch/source",
      targetBranchId: "branch target",
      expectedSourceHead: "commit_source",
      expectedTargetHead: "commit_target",
      sourceDisposition: "mark_merged",
    }, { idempotencyKey: "merge-key" });

    expect(calls.map((call) => call.input)).toEqual([
      "https://runtime.example/v1/contexts/context%2F1/branches",
      "https://runtime.example/v1/contexts/context%2F1/merges",
    ]);
    expect(calls.map((call) => (call.init?.headers as Record<string, string>)["idempotency-key"]))
      .toEqual(["fork-key", "merge-key"]);
    expect(JSON.parse(calls[1]?.init?.body as string)).toMatchObject({
      expectedSourceHead: "commit_source",
      expectedTargetHead: "commit_target",
      selections: [],
    });
  });

  it("loads projections and submits exact patch and snapshot contracts", async () => {
    const calls: Array<{ input: RequestInfo | URL; init?: RequestInit }> = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ input, init });
      if (calls.length <= 2) return Response.json({
        contextId: "context/1", branchId: "branch/1", headCommitId: "commit/2", value: { lore: true },
      });
      if (calls.length === 3) return Response.json({ ...commit, branchId: "branch/1", id: "commit_3", operationId: "operation_3" });
      return Response.json({
        commitId: "commit/2", snapshotRef: "object_2", checksum: "sha256:snapshot",
        schemaVersion: 1, retentionUntil: null, pinned: true, createdAt: 3,
      });
    });
    const client = new HttpContextClient("https://runtime.example");
    await client.getBranch("context/1", "branch/1");
    await client.getCommit("commit/2");
    await client.commitPatch("context/1", "branch/1", {
      baseCommitId: "commit/2", operationId: "operation_3",
      ops: [{ op: "append", path: "/lore", elementId: "entry_1", value: { text: "moon" } }],
      schemaVersion: 1, policyVersion: 1, author: { kind: "user", id: "local-user" },
    });
    await client.createSnapshot("commit/2", { pinned: true });
    expect(calls.map((call) => call.input)).toEqual([
      "https://runtime.example/v1/contexts/context%2F1/branches/branch%2F1",
      "https://runtime.example/v1/context-commits/commit%2F2",
      "https://runtime.example/v1/contexts/context%2F1/branches/branch%2F1/commits",
      "https://runtime.example/v1/context-commits/commit%2F2/snapshot",
    ]);
    expect(JSON.parse(calls[2]?.init?.body as string).patch.ops[0]).toEqual({
      op: "append", path: "/lore", element_id: "entry_1", value: { text: "moon" },
    });
  });
});
