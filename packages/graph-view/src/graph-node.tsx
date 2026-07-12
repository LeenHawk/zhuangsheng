import { Handle, Position, type NodeProps } from "@xyflow/react";
import { Check, CircleDashed, CircleX, Clock3, CornerDownRight, Loader2, RotateCw } from "lucide-react";

import type { StudioNode } from "./layout";

export function GraphNode({ data, selected }: NodeProps<StudioNode>) {
  const overlay = data.overlay;
  return (
    <div className={`min-w-52 rounded-xl border bg-surface shadow-soft ${selected ? "border-accent ring-2 ring-accent/20" : overlay ? statusBorder[overlay.status] : "border-default"}`}>
      <div className="flex items-center justify-between border-b border-default px-3 py-2">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-primary">{data.label}</div>
          <div className="mt-0.5 font-mono text-[10px] uppercase tracking-wider text-muted">{data.kind}</div>
        </div>
        {data.isEntry && <span className="rounded-md bg-accent-soft px-1.5 py-0.5 text-[10px] font-bold text-accent">入口</span>}
      </div>
      {overlay && (
        <div className="flex items-center gap-2 border-b border-default px-3 py-2 text-[10px]">
          <StatusIcon status={overlay.status} />
          <span className="font-semibold">{statusLabel[overlay.status]}</span>
          <span className="ml-auto text-muted">{overlay.activationCount} activation · {overlay.attemptCount} attempt</span>
        </div>
      )}
      <div className="grid grid-cols-2 gap-3 px-3 py-2.5 text-[11px]">
        <PortList direction="input" ports={data.inputs} />
        <PortList direction="output" ports={data.outputs} />
      </div>
    </div>
  );
}

function StatusIcon({ status }: { status: NonNullable<StudioNode["data"]["overlay"]>["status"] }) {
  const Icon = status === "completed" ? Check
    : status === "failed" ? CircleX
      : status === "running" ? Loader2
        : status === "waiting" ? Clock3
          : status === "retrying" ? RotateCw
            : CircleDashed;
  return <Icon className={`size-3.5 ${status === "running" || status === "retrying" ? "animate-spin" : ""}`} aria-hidden="true" />;
}

const statusLabel = {
  scheduled: "已调度",
  running: "运行中",
  waiting: "等待中",
  retrying: "准备重试",
  completed: "已完成",
  failed: "失败",
} as const;

const statusBorder = {
  scheduled: "border-info/40",
  running: "border-running/50",
  waiting: "border-warning/50",
  retrying: "border-warning/50",
  completed: "border-success/50",
  failed: "border-danger/50",
} as const;

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
