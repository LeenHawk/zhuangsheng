import { Handle, Position, type NodeProps } from "@xyflow/react";
import { CornerDownRight } from "lucide-react";

import type { StudioNode } from "./layout";

export function GraphNode({ data, selected }: NodeProps<StudioNode>) {
  return (
    <div className={`min-w-52 rounded-xl border bg-surface shadow-soft ${selected ? "border-accent ring-2 ring-accent/20" : "border-default"}`}>
      <div className="flex items-center justify-between border-b border-default px-3 py-2">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-primary">{data.label}</div>
          <div className="mt-0.5 font-mono text-[10px] uppercase tracking-wider text-muted">{data.kind}</div>
        </div>
        {data.isEntry && <span className="rounded-md bg-accent-soft px-1.5 py-0.5 text-[10px] font-bold text-accent">入口</span>}
      </div>
      <div className="grid grid-cols-2 gap-3 px-3 py-2.5 text-[11px]">
        <PortList direction="input" ports={data.inputs} />
        <PortList direction="output" ports={data.outputs} />
      </div>
    </div>
  );
}

function PortList({ direction, ports }: { direction: "input" | "output"; ports: string[] }) {
  return (
    <div className={direction === "output" ? "text-right" : undefined}>
      <div className="mb-1 text-[9px] font-bold uppercase tracking-wider text-muted">
        {direction === "input" ? "Inputs" : "Outputs"}
      </div>
      {ports.length === 0 && <div className="text-muted">—</div>}
      {ports.map((port, index) => (
        <div key={port} className="relative flex min-h-5 items-center gap-1 text-secondary">
          {direction === "output" && <CornerDownRight className="ml-auto size-3" />}
          <span className={direction === "output" ? undefined : "mr-auto"}>{port}</span>
          <Handle
            id={`${direction === "input" ? "in" : "out"}:${port}`}
            type={direction === "input" ? "target" : "source"}
            position={direction === "input" ? Position.Left : Position.Right}
            style={{ top: 66 + index * 20, [direction === "input" ? "left" : "right"]: -5 }}
            isConnectable={false}
          />
        </div>
      ))}
    </div>
  );
}
