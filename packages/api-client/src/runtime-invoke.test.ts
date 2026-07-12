import { describe, expect, it } from "vitest";

import { invokeRun, type RuntimeInvocationClient } from "./runtime-invoke";
import type { RunStatus, RunView } from "./run-types";
import type { DurableRunEvent, RunStreamMessage } from "./stream-types";

describe("invokeRun", () => {
  it("composes start, durable terminal wait, authoritative run and outputs", async () => {
    const calls: string[] = [];
    const client = fixture("completed", calls);
    const result = await invokeRun(
      client,
      "graphrev_1",
      {
        input: { message: "hello" },
        context: { mode: "temporary" },
        idempotencyKey: "invoke_1",
      },
      new AbortController().signal,
    );

    expect(result.run.status).toBe("completed");
    expect(result.outputs?.reply?.values[0]).toMatchObject({ value: "done" });
    expect(calls).toEqual(["start:graphrev_1", "events:2", "get:run_1", "outputs:run_1"]);
  });

  it("returns a failed terminal run without fabricating outputs", async () => {
    const calls: string[] = [];
    const result = await invokeRun(
      fixture("failed", calls),
      "graphrev_1",
      {
        input: null,
        context: { mode: "temporary" },
        idempotencyKey: "invoke_failed",
      },
      new AbortController().signal,
    );

    expect(result).toMatchObject({ run: { status: "failed" }, outputs: null });
    expect(calls).not.toContain("outputs:run_1");
  });
});

const fixture = (terminal: "completed" | "failed", calls: string[]): RuntimeInvocationClient => ({
  async startRun(graphRevisionId) {
    calls.push(`start:${graphRevisionId}`);
    return run("running");
  },
  async streamRunEvents(_runId, cursor, _signal, observer) {
    calls.push(`events:${cursor}`);
    observer.onOpen();
    observer.onMessage(durable(terminal));
  },
  async getRun(runId) {
    calls.push(`get:${runId}`);
    return run(terminal);
  },
  async getRunOutputs(runId) {
    calls.push(`outputs:${runId}`);
    return {
      reply: {
        collection: "single",
        values: [{
          kind: "inline_json",
          valueRef: "object_1",
          contentHash: `sha256:${"a".repeat(64)}`,
          sizeBytes: 6,
          value: "done",
        }],
      },
    };
  },
});

const run = (status: RunStatus): RunView => ({
  id: "run_1",
  graphRevisionId: "graphrev_1",
  status,
  controlEpoch: 0,
  contextId: "context_1",
  branchId: "branch_1",
  inputCommitId: "commit_1",
  inputRef: "object_1",
  outputCommitId: status === "completed" ? "commit_2" : null,
  lastDurableSeq: status === "running" ? 2 : 5,
  deadlineAt: 100,
  createdAt: 1,
  updatedAt: 2,
});

const durable = (status: "completed" | "failed"): RunStreamMessage => ({
  kind: "durable",
  event: {
    id: "event_5",
    runId: "run_1",
    durableSeq: 5,
    type: `run.${status}`,
    schemaVersion: 1,
    timestamp: 2,
    nodeInstanceId: null,
    attemptId: null,
    importance: "critical",
    payload: { schemaVersion: 1 },
  } satisfies DurableRunEvent,
});
