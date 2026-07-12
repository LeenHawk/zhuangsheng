import { afterEach, describe, expect, it, vi } from "vitest";

import { DecodeError } from "./decode-error";
import { decodeContextBranches, decodeContextCommits, decodeContextDiff } from "./decode-context";
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
});
