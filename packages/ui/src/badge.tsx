import type { HTMLAttributes } from "react";

import { cn } from "./cn";

type Tone = "neutral" | "info" | "running" | "success" | "warning" | "danger";

const tones: Record<Tone, string> = {
  neutral: "border-default bg-elevated text-secondary",
  info: "border-info/25 bg-info/10 text-info",
  running: "border-running/25 bg-running/10 text-running",
  success: "border-success/25 bg-success/10 text-success",
  warning: "border-warning/25 bg-warning/10 text-warning",
  danger: "border-danger/25 bg-danger/10 text-danger",
};

export function Badge({ className, children, tone = "neutral", ...spanProps }: HTMLAttributes<HTMLSpanElement> & { tone?: Tone }) {
  return (
    <span className={cn("inline-flex items-center rounded-full border px-2.5 py-1 text-xs font-semibold", tones[tone], className)} {...spanProps}>
      {children}
    </span>
  );
}
