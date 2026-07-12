import { RunStreamProtocolError } from "./stream-error";
import type {
  DurableRunEvent,
  EphemeralRunEvent,
  RunStreamMessage,
  RunStreamProjection,
} from "./stream-types";

const MAX_LIVE_TEXT_CHARS = 256 * 1024;

export const createRunStreamProjection = (
  runId: string,
  durableSeq = 0,
): RunStreamProjection => ({
  runId,
  durableSeq,
  terminalStatus: null,
  refreshVersion: 0,
  liveItems: {},
  lastSeqByCall: {},
  nextLiveOrder: 0,
    liveTruncated: false,
    recentEvents: [],
});

export const reduceRunStream = (
  state: RunStreamProjection,
  message: RunStreamMessage,
): RunStreamProjection => {
  if (message.event.runId !== state.runId) {
    throw new RunStreamProtocolError("run stream crossed its run boundary");
  }
  return message.kind === "durable"
    ? reduceDurable(state, message.event)
    : reduceEphemeral(state, message.event);
};

export const disconnectRunStream = (state: RunStreamProjection): RunStreamProjection => ({
  ...state,
  liveItems: {},
  lastSeqByCall: {},
  nextLiveOrder: 0,
  liveTruncated: false,
});

export const selectLiveText = (state: RunStreamProjection): string =>
  Object.values(state.liveItems)
    .sort((left, right) => left.order - right.order)
    .map((item) => item.text)
    .join("");

const reduceDurable = (
  state: RunStreamProjection,
  event: DurableRunEvent,
): RunStreamProjection => {
  if (event.durableSeq <= state.durableSeq) return state;
  if (event.importance === "critical" && !KNOWN_DURABLE_EVENTS.has(event.type)) {
    throw new RunStreamProtocolError(`unknown critical run event: ${event.type}`);
  }
  let next: RunStreamProjection = {
    ...state,
    durableSeq: event.durableSeq,
    recentEvents: [
      ...state.recentEvents.slice(-499),
      {
        durableSeq: event.durableSeq,
        type: event.type,
        timestamp: event.timestamp,
        nodeInstanceId: event.nodeInstanceId,
        attemptId: event.attemptId,
        graphNodeId: graphNodeId(event.payload),
        importance: event.importance,
      },
    ],
  };
  if ((event.type === "node.completed" || event.type === "node.failed") && event.nodeInstanceId) {
    next = clearNode(next, event.nodeInstanceId);
  }
  const terminalStatus = terminal(event.type);
  if (terminalStatus) {
    next = {
      ...disconnectRunStream(next),
      terminalStatus,
      refreshVersion: next.refreshVersion + 1,
    };
  } else if (REFRESH_EVENTS.has(event.type) || event.type.startsWith("node.wait.")) {
    next = { ...next, refreshVersion: next.refreshVersion + 1 };
  }
  return next;
};

const graphNodeId = (payload: unknown): string | null => {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) return null;
  const value = (payload as Record<string, unknown>).nodeId;
  return typeof value === "string" ? value : null;
};

const reduceEphemeral = (
  state: RunStreamProjection,
  envelope: EphemeralRunEvent,
): RunStreamProjection => {
  if (envelope.audience !== "user" && envelope.audience !== "both") return state;
  const event = envelope.event;
  const previousSeq = state.lastSeqByCall[event.callId];
  if (previousSeq !== undefined && event.seq <= previousSeq) return state;
  let next: RunStreamProjection = {
    ...state,
    lastSeqByCall: { ...state.lastSeqByCall, [event.callId]: event.seq },
  };
  if (event.type !== "text_delta" || event.text.length === 0) return next;
  const key = `${envelope.modelCallId}/${event.itemId}`;
  const current = state.liveItems[key];
  const used = Object.values(state.liveItems).reduce((total, item) => total + item.text.length, 0);
  const remaining = Math.max(0, MAX_LIVE_TEXT_CHARS - used);
  const append = event.text.slice(0, remaining);
  const item = current ?? {
    key,
    nodeInstanceId: envelope.nodeInstanceId,
    modelCallId: envelope.modelCallId,
    itemId: event.itemId,
    text: "",
    order: state.nextLiveOrder,
  };
  next = {
    ...next,
    liveItems: {
      ...state.liveItems,
      [key]: { ...item, text: item.text + append },
    },
    nextLiveOrder: current ? state.nextLiveOrder : state.nextLiveOrder + 1,
    liveTruncated: state.liveTruncated || append.length < event.text.length,
  };
  return next;
};

const clearNode = (state: RunStreamProjection, nodeInstanceId: string): RunStreamProjection => {
  const liveItems = Object.fromEntries(
    Object.entries(state.liveItems).filter(([, item]) => item.nodeInstanceId !== nodeInstanceId),
  );
  return Object.keys(liveItems).length === Object.keys(state.liveItems).length
    ? state
    : { ...state, liveItems };
};

const terminal = (type: string): RunStreamProjection["terminalStatus"] => {
  if (type === "run.completed") return "completed";
  if (type === "run.failed") return "failed";
  if (type === "run.cancelled") return "cancelled";
  return null;
};

const REFRESH_EVENTS = new Set([
  "run.interrupt.requested",
  "run.waiting",
  "run.interrupted",
  "run.resumed",
  "llm.tool.approval_requested",
  "llm.tool.approval_resolved",
  "effect.outcome_unknown",
  "effect.resolved",
]);

const KNOWN_DURABLE_EVENTS = new Set([
  "run.created", "run.started", "run.waiting", "run.completed", "run.failed",
  "run.cancel.requested", "run.cancelled", "run.interrupt.requested", "run.interrupted",
  "run.resumed", "run.output.committed", "node.scheduled", "node.started",
  "node.completed", "node.failed", "node.attempt.leased", "node.attempt.timed_out",
  "node.lease.expired", "node.retry.scheduled", "node.retry.ready",
  "node.wait.secret_store_required", "node.wait.secret_store_resolved",
  "edge.value.enqueued", "edge.value.consumed", "edge.value.stranded",
  "router.decision", "router.decision_error", "router.read_conflict",
  "coordination.merge_selected", "coordination.expand_completed",
  "coordination.join_item_indexed", "coordination.join_item_stranded",
  "coordination.join_tuple_selected", "coordination.window_opened",
  "coordination.window_item_added", "coordination.window_closed",
  "llm.count.prepared", "llm.count.started", "llm.count.completed", "llm.count.failed",
  "llm.count.retry_prepared", "llm.count.retry_ready", "llm.output.repair_prepared",
  "llm.stream.chunk", "llm.tool.prepared", "llm.tool.started", "llm.tool.completed",
  "llm.tool.failed", "llm.tool.outcome_unknown", "llm.tool.retry_prepared",
  "llm.tool.retry_ready", "llm.tool.memory_search_completed",
  "llm.tool.approval_requested", "llm.tool.approval_resolved",
  "effect.outcome_unknown", "effect.resolved",
]);
