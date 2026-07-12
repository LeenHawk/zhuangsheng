import { AlertTriangle, CheckCircle2 } from "lucide-react";

import type { ValidationIssue } from "@zhuangsheng/api-client";

export function GraphDiagnostics({ issues, emptyLabel = "当前没有诊断。" }: { issues: ValidationIssue[]; emptyLabel?: string }) {
  if (issues.length === 0) {
    return <div className="flex items-center gap-2 rounded-xl border border-success/20 bg-success/5 p-3 text-sm text-success"><CheckCircle2 className="size-4" />{emptyLabel}</div>;
  }
  return (
    <ul className="space-y-2" aria-label="Graph diagnostics">
      {issues.map((issue, index) => (
        <li key={`${issue.code}:${issue.path}:${index}`} className="rounded-xl border border-warning/25 bg-warning/5 p-3">
          <div className="flex items-start gap-2">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" aria-hidden="true" />
            <div className="min-w-0">
              <div className="text-sm font-semibold text-primary">{issue.message}</div>
              <div className="mt-1 break-all font-mono text-[11px] text-muted">{issue.code} · {issue.path || "/"}</div>
            </div>
          </div>
        </li>
      ))}
    </ul>
  );
}
