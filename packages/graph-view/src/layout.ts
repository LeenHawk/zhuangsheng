import type { Edge, Node } from "@xyflow/react";

import type { GraphStructureProjection } from "@zhuangsheng/api-client";

export interface StudioNodeData extends Record<string, unknown> {
  label: string;
  kind: string;
  isEntry: boolean;
  inputs: string[];
  outputs: string[];
}

export type StudioNode = Node<StudioNodeData, "studio">;

export function graphElements(graph: GraphStructureProjection): { nodes: StudioNode[]; edges: Edge[] } {
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
    },
  }));
  const edges = graph.edges.map((edge): Edge => ({
    id: edge.id,
    source: edge.source,
    sourceHandle: `out:${edge.sourcePort}`,
    target: edge.target,
    targetHandle: `in:${edge.targetPort}`,
    label: `${edge.sourcePort} → ${edge.targetPort}`,
  }));
  return { nodes, edges };
}
