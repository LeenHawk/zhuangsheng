import { afterEach, describe, expect, it, vi } from "vitest";

import { DecodeError } from "./decode-error";
import { decodeRun } from "./decode-runs";
import { HttpRuntimeClient } from "./http-runtime-client";

describe("RunView client", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("decodes stable status and cursor fields fail-closed", () => {
    expect(decodeRun(run()).status).toBe("running");
    expect(() => decodeRun({ ...run(), status: "future_status" })).toThrow(DecodeError);
    expect(() => decodeRun({ ...run(), lastDurableSeq: -1 })).toThrow(DecodeError);
  });

  it("lists recent runs and sends control epoch CAS with an explicit key", async () => {
    const calls: Array<{ input: RequestInfo | URL; init?: RequestInit }> = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ input, init });
      const payload = calls.length === 1 ? { items: [run()] } : { ...run(), status: "interrupting", controlEpoch: 2 };
      return new Response(JSON.stringify(payload), { status: 200 });
    });
    const client = new HttpRuntimeClient("https://roleplay.example");

    expect((await client.listRecentRuns(999)).items[0]?.id).toBe("run_1");
    const interrupted = await client.interrupt("run_1", {
      expectedEpoch: 1,
      idempotencyKey: "interrupt_1",
      reason: "inspect state",
    });

    expect(interrupted.status).toBe("interrupting");
    expect(calls[0]?.input).toBe("https://roleplay.example/v1/runs?limit=100");
    expect(calls[1]?.input).toBe("https://roleplay.example/v1/runs/run_1/interrupt");
    expect(calls[1]?.init?.headers).toMatchObject({ "idempotency-key": "interrupt_1" });
    expect(JSON.parse(calls[1]?.init?.body as string)).toEqual({
      expectedEpoch: 1,
      reason: "inspect state",
    });
  });
});

const run = () => ({
  id: "run_1",
  graphRevisionId: "graphrev_1",
  status: "running",
  controlEpoch: 1,
  contextId: "context_1",
  branchId: "branch_1",
  inputCommitId: "commit_1",
  inputRef: "object_1",
  outputCommitId: null,
  lastDurableSeq: 3,
  deadlineAt: 100,
  createdAt: 1,
  updatedAt: 2,
});
