import type { RunGraphEdgeOverlay, RunGraphNodeOverlay, RunStreamProjection } from "./stream-types";

export const selectRunGraphNodeOverlay = (
  state: RunStreamProjection,
): Record<string, RunGraphNodeOverlay> => {
  const overlay: Record<string, RunGraphNodeOverlay> = {};
  for (const event of state.recentEvents) {
    if (!event.graphNodeId) continue;
    const current = overlay[event.graphNodeId] ?? {
      status: "scheduled",
      activationCount: 0,
      attemptCount: 0,
      lastDurableSeq: 0,
    };
    overlay[event.graphNodeId] = {
      status: nodeStatus(event.type) ?? current.status,
      activationCount: current.activationCount + (event.type === "node.scheduled" ? 1 : 0),
      attemptCount: current.attemptCount + (event.type === "node.started" ? 1 : 0),
      lastDurableSeq: event.durableSeq,
    };
  }
  return overlay;
};

export const selectRunGraphEdgeOverlay = (
  state: RunStreamProjection,
): Record<string, RunGraphEdgeOverlay> => {
  const overlay: Record<string, RunGraphEdgeOverlay> = {};
  const queueEdges: Record<string, string> = {};
  for (const event of state.recentEvents) {
    if (event.graphEdgeId && event.queueValueId) queueEdges[event.queueValueId] = event.graphEdgeId;
    const edgeId = event.graphEdgeId ?? (event.queueValueId ? queueEdges[event.queueValueId] : null);
    if (!edgeId || !event.type.startsWith("edge.value.")) continue;
    const current = overlay[edgeId] ?? {
      enqueuedCount: 0,
      consumedCount: 0,
      strandedCount: 0,
      lastDurableSeq: 0,
    };
    overlay[edgeId] = {
      enqueuedCount: current.enqueuedCount + (event.type === "edge.value.enqueued" ? 1 : 0),
      consumedCount: current.consumedCount + (event.type === "edge.value.consumed" ? 1 : 0),
      strandedCount: current.strandedCount + (event.type === "edge.value.stranded" ? 1 : 0),
      lastDurableSeq: event.durableSeq,
    };
  }
  return overlay;
};

const nodeStatus = (type: string): RunGraphNodeOverlay["status"] | null => {
  if (type === "node.scheduled") return "scheduled";
  if (type === "node.started") return "running";
  if (type.startsWith("node.wait.") || type === "coordination.window_opened") return "waiting";
  if (type.startsWith("node.retry.") || type === "node.attempt.timed_out" || type === "node.lease.expired") {
    return "retrying";
  }
  if (type === "node.completed") return "completed";
  if (type === "node.failed") return "failed";
  return null;
};
