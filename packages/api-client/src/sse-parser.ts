import { RunStreamProtocolError } from "./stream-error";

const MAX_LINE_CHARS = 1024 * 1024;
const MAX_EVENT_CHARS = 2 * 1024 * 1024;

export interface SseFrame {
  event: string;
  data: string;
  id?: string;
}

export class SseParser {
  private buffer = "";
  private event = "";
  private id: string | undefined;
  private data: string[] = [];
  private dataLength = 0;
  private hasData = false;

  push(chunk: string): SseFrame[] {
    this.buffer += chunk;
    if (!this.buffer.includes("\n") && this.buffer.length > MAX_LINE_CHARS) {
      throw new RunStreamProtocolError("SSE line exceeds the client limit");
    }
    const frames: SseFrame[] = [];
    let newline = this.buffer.indexOf("\n");
    while (newline >= 0) {
      let line = this.buffer.slice(0, newline);
      this.buffer = this.buffer.slice(newline + 1);
      if (line.endsWith("\r")) line = line.slice(0, -1);
      const frame = this.line(line);
      if (frame) frames.push(frame);
      newline = this.buffer.indexOf("\n");
    }
    return frames;
  }

  finish(): SseFrame[] {
    const frames: SseFrame[] = [];
    if (this.buffer.length > 0) {
      const line = this.buffer.endsWith("\r") ? this.buffer.slice(0, -1) : this.buffer;
      this.buffer = "";
      const frame = this.line(line);
      if (frame) frames.push(frame);
    }
    const pending = this.dispatch();
    if (pending) frames.push(pending);
    return frames;
  }

  private line(line: string): SseFrame | undefined {
    if (line === "") return this.dispatch();
    if (line.startsWith(":")) return undefined;
    const colon = line.indexOf(":");
    const field = colon < 0 ? line : line.slice(0, colon);
    let value = colon < 0 ? "" : line.slice(colon + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (field === "event") this.event = value;
    if (field === "data") {
      this.dataLength += value.length;
      if (this.dataLength > MAX_EVENT_CHARS) {
        throw new RunStreamProtocolError("SSE event exceeds the client limit");
      }
      this.data.push(value);
      this.hasData = true;
    }
    if (field === "id" && !value.includes("\0")) this.id = value;
    return undefined;
  }

  private dispatch(): SseFrame | undefined {
    if (!this.hasData) {
      this.reset();
      return undefined;
    }
    const frame: SseFrame = {
      event: this.event || "message",
      data: this.data.join("\n"),
      ...(this.id === undefined ? {} : { id: this.id }),
    };
    this.reset();
    return frame;
  }

  private reset() {
    this.event = "";
    this.id = undefined;
    this.data = [];
    this.dataLength = 0;
    this.hasData = false;
  }
}
