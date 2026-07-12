import { describe, expect, it } from "vitest";

import { DecodeError } from "./decode-error";
import { followRunEvents, type RunEventStreamClient } from "./run-stream-follow";
import {
  createRunStreamProjection,
  disconnectRunStream,
  reduceRunStream,
  selectLiveText,
} from "./run-stream-reducer";
import { decodeRunStreamFrame } from "./stream-decode";
import { RunStreamProtocolError } from "./stream-error";
import type { DurableRunEvent, RunStreamMessage, RunStreamProjection } from "./stream-types";

describe("run stream decoding and reduction", () => {
  it("keeps ephemeral text outside the durable cursor and clears it on disconnect", () => {
    let projection = createRunStreamProjection("run_1");
    projection = reduceRunStream(projection, ephemeral(0, "Moon"));
    projection = reduceRunStream(projection, ephemeral(0, "duplicate"));
    projection = reduceRunStream(projection, ephemeral(2, "light"));
    expect(selectLiveText(projection)).toBe("Moonlight");
    expect(projection.durableSeq).toBe(0);
    projection = disconnectRunStream(projection);
    expect(selectLiveText(projection)).toBe("");
  });

  it("accepts durable sequence gaps, ignores duplicates, and fails on unknown critical events", () => {
    let projection = createRunStreamProjection("run_1");
    projection = reduceRunStream(projection, durable(4, "run.started"));
    const duplicate = reduceRunStream(projection, durable(4, "run.started"));
    expect(duplicate).toBe(projection);
    projection = reduceRunStream(projection, durable(9, "run.completed"));
    expect(projection.durableSeq).toBe(9);
    expect(projection.terminalStatus).toBe("completed");
    expect(() => reduceRunStream(createRunStreamProjection("run_1"), durable(1, "future.critical")))
      .toThrow(RunStreamProtocolError);
  });

  it("validates SSE id, event name, run identity, and live schema version", () => {
    const decoded = decodeRunStreamFrame({
      id: "3",
      event: "run.started",
      data: JSON.stringify(durable(3, "run.started").event),
    }, "run_1");
    expect(decoded.kind).toBe("durable");
    expect(() => decodeRunStreamFrame({
      id: "2",
      event: "run.started",
      data: JSON.stringify(durable(3, "run.started").event),
    }, "run_1")).toThrow(DecodeError);
    expect(() => decodeRunStreamFrame({
      event: "llm.stream.text_delta",
      data: JSON.stringify({ ...ephemeral(1, "x").event, schemaVersion: 2 }),
    }, "run_1")).toThrow(DecodeError);
  });
});

describe("followRunEvents", () => {
  it("reconnects from the last reduced cursor and terminates on a durable terminal event", async () => {
    const cursors: number[] = [];
    const projections: RunStreamProjection[] = [];
    const connections: string[] = [];
    let connection = 0;
    const client: RunEventStreamClient = {
      async streamRunEvents(_runId, cursor, _signal, observer) {
        cursors.push(cursor);
        observer.onOpen();
        connection += 1;
        if (connection === 1) {
          observer.onMessage(durable(4, "run.started"));
          observer.onMessage(ephemeral(1, "temporary"));
          throw new Error("network lost");
        }
        observer.onMessage(durable(7, "run.completed"));
      },
    };
    await followRunEvents(client, "run_1", new AbortController().signal, {
      backoffBaseMs: 0,
      random: () => 0.5,
      onProjection: (projection) => projections.push(projection),
      onConnection: (state) => connections.push(state),
    });
    expect(cursors).toEqual([0, 4]);
    expect(projections.some((projection) => selectLiveText(projection) === "temporary")).toBe(true);
    expect(selectLiveText(projections.at(-2)!)).toBe("");
    expect(projections.at(-1)?.terminalStatus).toBe("completed");
    expect(connections).toContain("reconnecting");
    expect(connections.at(-1)).toBe("closed");
  });
});

const durable = (durableSeq: number, type: string): RunStreamMessage => ({
  kind: "durable",
  event: {
    id: `event_${durableSeq}`,
    runId: "run_1",
    durableSeq,
    type,
    schemaVersion: 1,
    timestamp: 1,
    nodeInstanceId: null,
    attemptId: null,
    importance: "critical",
    payload: { schemaVersion: 1 },
  } satisfies DurableRunEvent,
});

const ephemeral = (seq: number, text: string): RunStreamMessage => ({
  kind: "ephemeral",
  event: {
    schemaVersion: 1,
    runId: "run_1",
    nodeInstanceId: "node_1",
    attemptId: "attempt_1",
    modelCallId: "call_1",
    audience: "user",
    event: { type: "text_delta", callId: "call_1", seq, itemId: "item_1", text },
  },
});
