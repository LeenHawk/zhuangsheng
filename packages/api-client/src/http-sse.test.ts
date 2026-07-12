import { afterEach, describe, expect, it, vi } from "vitest";

import { streamRunEvents } from "./http-sse";
import type { RunStreamMessage } from "./stream-types";

describe("streamRunEvents", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("decodes durable and ephemeral frames from a chunked fetch body", async () => {
    const encoder = new TextEncoder();
    const durable = JSON.stringify({
      id: "event_1",
      runId: "run_1",
      durableSeq: 1,
      type: "run.started",
      schemaVersion: 1,
      timestamp: 1,
      nodeInstanceId: null,
      attemptId: null,
      importance: "critical",
      payload: { schemaVersion: 1 },
    });
    const ephemeral = JSON.stringify({
      schemaVersion: 1,
      runId: "run_1",
      nodeInstanceId: "node_1",
      attemptId: "attempt_1",
      modelCallId: "call_1",
      audience: "user",
      event: { type: "text_delta", callId: "call_1", seq: 0, itemId: "item_1", text: "hello" },
    });
    const chunks = [
      `: heartbeat\nid: 1\nevent: run.started\ndata: ${durable.slice(0, 30)}`,
      `${durable.slice(30)}\n\nevent: llm.stream.text_delta\ndata: ${ephemeral}\n\n`,
    ];
    vi.stubGlobal("fetch", async () => new Response(new ReadableStream({
      start(controller) {
        chunks.forEach((chunk) => controller.enqueue(encoder.encode(chunk)));
        controller.close();
      },
    }), { headers: { "content-type": "text/event-stream; charset=utf-8" } }));
    const messages: RunStreamMessage[] = [];
    let opened = false;

    await streamRunEvents("", "run_1", 0, new AbortController().signal, {
      onOpen: () => { opened = true; },
      onMessage: (message) => messages.push(message),
    });

    expect(opened).toBe(true);
    expect(messages.map((message) => message.kind)).toEqual(["durable", "ephemeral"]);
    expect(messages[0]?.event.runId).toBe("run_1");
  });
});
