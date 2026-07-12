import { afterEach, describe, expect, it, vi } from "vitest";

import { DecodeError } from "./decode-error";
import { decodeRun, decodeRunOutputs } from "./decode-runs";
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

  it("starts runs and decodes inline and referenced outputs", async () => {
    const calls: Array<{ input: RequestInfo | URL; init?: RequestInit }> = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ input, init });
      const path = String(input);
      if (path.endsWith("/runs")) return Response.json(run());
      if (path.endsWith("/outputs")) return Response.json(outputs());
      return Response.json({ large: ["value"] });
    });
    const client = new HttpRuntimeClient("https://roleplay.example");
    const started = await client.startRun("graphrev_1", {
      input: { message: "hello" },
      context: { mode: "temporary" },
      idempotencyKey: "start_1",
    });
    const decoded = await client.getRunOutputs(started.id);
    const large = await client.loadJsonValue("object/large");

    expect(decoded.reply?.values[0]).toMatchObject({ kind: "inline_json", value: { text: "ok" } });
    expect(decoded.archive?.values[0]).toMatchObject({ kind: "json_value_ref", downloadPath: "/v1/values/object_3" });
    expect(large).toEqual({ large: ["value"] });
    expect(calls[0]?.input).toBe("https://roleplay.example/v1/graphs/graphrev_1/runs");
    expect(calls[0]?.init?.headers).toMatchObject({ "idempotency-key": "start_1" });
    expect(JSON.parse(calls[0]?.init?.body as string)).toEqual({
      input: { message: "hello" },
      context: { mode: "temporary" },
      deadlineAt: null,
    });
    expect(calls[2]?.input).toBe("https://roleplay.example/v1/values/object%2Flarge");
  });

  it("rejects unknown output envelopes and invalid collections", () => {
    expect(() => decodeRunOutputs({ reply: { collection: "future", values: [] } }))
      .toThrow(DecodeError);
    expect(() => decodeRunOutputs({
      reply: { collection: "single", values: [{ ...outputs().reply.values[0], kind: "future" }] },
    })).toThrow(DecodeError);
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

const outputs = () => ({
  reply: {
    collection: "single",
    values: [{
      kind: "inline_json",
      valueRef: "object_2",
      contentHash: `sha256:${"a".repeat(64)}`,
      sizeBytes: 13,
      value: { text: "ok" },
    }],
  },
  archive: {
    collection: "append",
    values: [{
      kind: "json_value_ref",
      valueRef: "object_3",
      contentHash: `sha256:${"b".repeat(64)}`,
      sizeBytes: 2_000_000,
      downloadPath: "/v1/values/object_3",
    }],
  },
});
