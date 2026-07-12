import { DecodeError } from "./decode-error";
import { nullableString, number, record, string } from "./decode-helpers";
import type { SseFrame } from "./sse-parser";
import type {
  DurableRunEvent,
  EphemeralRunEvent,
  LlmStreamEvent,
  RunStreamMessage,
} from "./stream-types";

const streamEventTypes: Record<string, LlmStreamEvent["type"]> = {
  "llm.stream.started": "started",
  "llm.stream.text_delta": "text_delta",
  "llm.stream.reasoning_delta": "reasoning_delta",
  "llm.stream.tool_call_delta": "tool_call_delta",
  "llm.stream.tool_call_completed": "tool_call_completed",
  "llm.stream.hosted_tool": "hosted_tool_event",
  "llm.stream.usage": "usage",
  "llm.stream.completed": "completed",
  "llm.stream.failed": "failed",
};

export const decodeRunStreamFrame = (
  frame: SseFrame,
  expectedRunId: string,
): RunStreamMessage => {
  const payload = parseJson(frame.data, "runStream.data");
  return frame.id !== undefined && frame.id !== ""
    ? { kind: "durable", event: durable(frame, payload, expectedRunId) }
    : { kind: "ephemeral", event: ephemeral(frame, payload, expectedRunId) };
};

const durable = (frame: SseFrame, value: unknown, expectedRunId: string): DurableRunEvent => {
  const path = "runStream.durable";
  const item = record(value, path);
  const durableSeq = positiveInteger(item.durableSeq, `${path}.durableSeq`);
  if (frame.id !== String(durableSeq)) throw new DecodeError(`${path}.durableSeq`);
  const type = string(item.type, `${path}.type`);
  if (frame.event !== type) throw new DecodeError(`${path}.type`);
  const runId = string(item.runId, `${path}.runId`);
  if (runId !== expectedRunId) throw new DecodeError(`${path}.runId`);
  if (item.schemaVersion !== 1) throw new DecodeError(`${path}.schemaVersion`);
  return {
    id: string(item.id, `${path}.id`),
    runId,
    durableSeq,
    type,
    schemaVersion: 1,
    timestamp: number(item.timestamp, `${path}.timestamp`),
    nodeInstanceId: nullableString(item.nodeInstanceId, `${path}.nodeInstanceId`),
    attemptId: nullableString(item.attemptId, `${path}.attemptId`),
    importance: string(item.importance, `${path}.importance`),
    payload: item.payload,
  };
};

const ephemeral = (frame: SseFrame, value: unknown, expectedRunId: string): EphemeralRunEvent => {
  const path = "runStream.ephemeral";
  const item = record(value, path);
  if (item.schemaVersion !== 1) throw new DecodeError(`${path}.schemaVersion`);
  const runId = string(item.runId, `${path}.runId`);
  if (runId !== expectedRunId) throw new DecodeError(`${path}.runId`);
  const audience = string(item.audience, `${path}.audience`);
  if (!isAudience(audience)) throw new DecodeError(`${path}.audience`);
  const event = llmEvent(frame.event, item.event, `${path}.event`);
  const modelCallId = string(item.modelCallId, `${path}.modelCallId`);
  if (modelCallId !== event.callId) throw new DecodeError(`${path}.modelCallId`);
  return {
    schemaVersion: 1,
    runId,
    nodeInstanceId: string(item.nodeInstanceId, `${path}.nodeInstanceId`),
    attemptId: string(item.attemptId, `${path}.attemptId`),
    modelCallId,
    audience,
    event,
  };
};

const llmEvent = (frameType: string, value: unknown, path: string): LlmStreamEvent => {
  const item = record(value, path);
  const type = string(item.type, `${path}.type`);
  if (streamEventTypes[frameType] !== type) throw new DecodeError(`${path}.type`);
  const callId = string(item.callId, `${path}.callId`);
  const seq = nonNegativeInteger(item.seq, `${path}.seq`);
  if (type !== "text_delta") return { type: type as Exclude<LlmStreamEvent["type"], "text_delta">, callId, seq };
  const text = string(item.text, `${path}.text`);
  if (text.length > 1024 * 1024) throw new DecodeError(`${path}.text`);
  return {
    type,
    callId,
    seq,
    itemId: string(item.itemId, `${path}.itemId`),
    text,
  };
};

const parseJson = (value: string, path: string): unknown => {
  try {
    return JSON.parse(value);
  } catch {
    throw new DecodeError(path);
  }
};

const positiveInteger = (value: unknown, path: string) => {
  const parsed = number(value, path);
  if (parsed <= 0) throw new DecodeError(path);
  return parsed;
};

const nonNegativeInteger = (value: unknown, path: string) => {
  const parsed = number(value, path);
  if (parsed < 0) throw new DecodeError(path);
  return parsed;
};

const isAudience = (value: string): value is EphemeralRunEvent["audience"] =>
  value === "user" || value === "trace" || value === "both" || value === "internal";
