import type { RunGraphNodeOverlay, RunStreamProjection } from "./stream-types";

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
