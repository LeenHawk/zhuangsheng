export interface DurableRunEvent {
  id: string;
  runId: string;
  durableSeq: number;
  type: string;
  schemaVersion: 1;
  timestamp: number;
  nodeInstanceId: string | null;
  attemptId: string | null;
  importance: string;
  payload: unknown;
}

export type LlmStreamEvent =
  | { type: "text_delta"; callId: string; seq: number; itemId: string; text: string }
  | {
      type:
        | "started"
        | "reasoning_delta"
        | "tool_call_delta"
        | "tool_call_completed"
        | "hosted_tool_event"
        | "usage"
        | "completed"
        | "failed";
      callId: string;
      seq: number;
    };

export interface EphemeralRunEvent {
  schemaVersion: 1;
  runId: string;
  nodeInstanceId: string;
  attemptId: string;
  modelCallId: string;
  audience: "user" | "trace" | "both" | "internal";
  event: LlmStreamEvent;
}

export type RunStreamMessage =
  | { kind: "durable"; event: DurableRunEvent }
  | { kind: "ephemeral"; event: EphemeralRunEvent };

export interface LiveTextItem {
  key: string;
  nodeInstanceId: string;
  modelCallId: string;
  itemId: string;
  text: string;
  order: number;
}

export interface RunStreamProjection {
  runId: string;
  durableSeq: number;
  terminalStatus: "completed" | "failed" | "cancelled" | null;
  refreshVersion: number;
  liveItems: Record<string, LiveTextItem>;
  lastSeqByCall: Record<string, number>;
  nextLiveOrder: number;
  liveTruncated: boolean;
}

export type RunStreamConnectionState =
  | "idle"
  | "connecting"
  | "live"
  | "reconnecting"
  | "incompatible"
  | "closed";
