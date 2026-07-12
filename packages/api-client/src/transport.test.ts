import { describe, expect, it } from "vitest";

import { TauriTransport, type TauriBridge } from "./transport";
import type { RunStreamMessage } from "./stream-types";

describe("TauriTransport", () => {
  it("treats duplicated and out-of-order callbacks only as wake hints", async () => {
    const rows = [event(1), event(2), event(3)];
    let visible = 1;
    let handler: () => void = () => undefined;
    let listening = false;
    const bridge: TauriBridge = {
      invoke: async <T>(_operation: string, payload: unknown) => {
        const cursor = (payload as { afterDurableSeq: number }).afterDurableSeq;
        return rows.slice(0, visible).filter((row) => row.durableSeq > cursor) as T;
      },
      listen: async (_event, next) => { handler = next; listening = true; return () => { listening = false; }; },
    };
    const received: RunStreamMessage[] = [];
    const controller = new AbortController();
    let opened!: () => void;
    const ready = new Promise<void>((resolve) => { opened = resolve; });
    const subscription = new TauriTransport(bridge).subscribeRun("run_1", 0, controller.signal, {
      onOpen: opened,
      onMessage: (message) => { received.push(message); if (received.length === 3) controller.abort(); },
    });
    await ready;
    while (!listening) await Promise.resolve();
    visible = 3;
    handler(); handler();
    await subscription;
    expect(received.map((message) => message.kind === "durable" ? message.event.durableSeq : 0)).toEqual([1, 2, 3]);
  });
});

const event = (durableSeq: number) => ({
  id: `event_${durableSeq}`, runId: "run_1", durableSeq, type: "node.completed",
  schemaVersion: 1, timestamp: durableSeq, nodeInstanceId: "node_1", attemptId: "attempt_1",
  importance: "critical", payload: { schemaVersion: 1, nodeId: "reply" },
});
