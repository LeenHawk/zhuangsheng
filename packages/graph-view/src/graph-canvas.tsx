import { useMemo } from "react";
import { Background, Controls, MiniMap, ReactFlow } from "@xyflow/react";

import type { GraphStructureProjection, RunGraphEdgeOverlay, RunGraphNodeOverlay } from "@zhuangsheng/api-client";

import { GraphNode } from "./graph-node";
import { graphElements } from "./layout";

const nodeTypes = { studio: GraphNode };

export function GraphCanvas({
  graph,
  nodeOverlay = {},
  edgeOverlay = {},
}: {
  graph: GraphStructureProjection;
  nodeOverlay?: Record<string, RunGraphNodeOverlay>;
  edgeOverlay?: Record<string, RunGraphEdgeOverlay>;
}) {
  const elements = useMemo(
    () => graphElements(graph, nodeOverlay, edgeOverlay),
    [edgeOverlay, graph, nodeOverlay],
  );
  if (elements.nodes.length === 0) {
    return <div className="grid h-full min-h-80 place-items-center text-sm text-muted">图中还没有节点。</div>;
  }
  return (
    <div className="h-full min-h-[480px] overflow-hidden rounded-xl bg-canvas">
      <ReactFlow
        nodes={elements.nodes}
        edges={elements.edges}
        nodeTypes={nodeTypes}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable
        fitView
        fitViewOptions={{ padding: 0.2 }}
        minZoom={0.25}
      >
        <Background gap={24} size={1} />
        <MiniMap pannable zoomable className="!bg-surface" />
        <Controls showInteractive={false} />
      </ReactFlow>
    </div>
  );
}
