import { describe, expect, it } from "vitest";

import { SseParser } from "./sse-parser";

describe("SseParser", () => {
  it("parses split CRLF frames, multiline data, and ignores heartbeats", () => {
    const parser = new SseParser();
    expect(parser.push(": heartbeat\r\nid: 4\r\nevent: run.started\r\ndata: {\"a\":" )).toEqual([]);
    expect(parser.push("1}\r\ndata: tail\r\n\r\n")).toEqual([{
      id: "4",
      event: "run.started",
      data: "{\"a\":1}\ntail",
    }]);
    expect(parser.finish()).toEqual([]);
  });

  it("does not inherit a durable id into a following ephemeral frame", () => {
    const parser = new SseParser();
    expect(parser.push("id: 5\nevent: run.started\ndata: {}\n\nevent: llm.stream.text_delta\ndata: {}\n\n"))
      .toEqual([
        { id: "5", event: "run.started", data: "{}" },
        { event: "llm.stream.text_delta", data: "{}" },
      ]);
  });
});
