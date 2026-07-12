import type { Edge, Node } from "@xyflow/react";

import type { GraphStructureProjection, RunGraphEdgeOverlay, RunGraphNodeOverlay } from "@zhuangsheng/api-client";

export interface StudioNodeData extends Record<string, unknown> {
  label: string;
  kind: string;
  isEntry: boolean;
  inputs: string[];
  outputs: string[];
  overlay: RunGraphNodeOverlay | null;
}

export type StudioNode = Node<StudioNodeData, "studio">;

export function graphElements(
  graph: GraphStructureProjection,
  nodeOverlay: Record<string, RunGraphNodeOverlay> = {},
  edgeOverlay: Record<string, RunGraphEdgeOverlay> = {},
): { nodes: StudioNode[]; edges: Edge[] } {
  const nodes = graph.nodes.map((node, index): StudioNode => ({
    id: node.id,
    type: "studio",
    position: { x: (index % 3) * 280, y: Math.floor(index / 3) * 190 },
    data: {
      label: node.name || node.id,
      kind: node.kind,
      isEntry: node.isEntry,
      inputs: node.inputs.map((port) => port.name),
      outputs: node.outputs.map((port) => port.name),
      overlay: nodeOverlay[node.id] ?? null,
    },
  }));
  const edges = graph.edges.map((edge): Edge => {
    const overlay = edgeOverlay[edge.id];
    const pending = overlay ? overlay.enqueuedCount - overlay.consumedCount - overlay.strandedCount : 0;
    return {
      id: edge.id,
      source: edge.source,
      sourceHandle: `out:${edge.sourcePort}`,
      target: edge.target,
      targetHandle: `in:${edge.targetPort}`,
      label: overlay
        ? `${edge.sourcePort} → ${edge.targetPort} · ${overlay.enqueuedCount}/${overlay.consumedCount}/${overlay.strandedCount}`
        : `${edge.sourcePort} → ${edge.targetPort}`,
      animated: pending > 0,
      style: overlay?.strandedCount ? { stroke: "hsl(var(--danger))" } : undefined,
    };
  });
  return { nodes, edges };
}
