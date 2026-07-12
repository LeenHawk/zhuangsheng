import type { ContextPresetPreviewView } from "@zhuangsheng/api-client";
import { Badge } from "@zhuangsheng/ui";

export function ContextPreviewPanel({ preview }: { preview: ContextPresetPreviewView }) {
  return <section className="mt-4 rounded-xl border border-info/20 bg-info/5 p-4" aria-label="Context preview">
    <div className="flex flex-wrap items-center gap-2">
      <h3 className="font-semibold">Context preview</h3>
      <Badge tone="info">metadata-only · sample bindings</Badge>
      <Badge>{preview.countSource}</Badge>
      <span className="ml-auto text-xs text-muted">{preview.budgetReport.assembledTokens} / {preview.budgetReport.availableInputTokens} tokens</span>
    </div>
    <div className="mt-3 grid gap-2">{preview.items.map((item) => (
      <div key={item.itemId} className="flex flex-col gap-2 rounded-lg border border-default bg-surface p-3 sm:flex-row sm:items-center">
        <div className="min-w-0 flex-1"><p className="truncate text-sm font-medium">{item.name ?? item.itemId}</p><p className="text-xs text-muted">{item.sourceType} → {item.requestedRole}</p></div>
        <div className="flex items-center gap-2"><Badge tone={item.included ? "success" : "warning"}>{item.action}</Badge><span className="text-xs tabular-nums text-muted">{item.tokenCount} tokens</span></div>
        {item.reason && <p className="text-xs text-muted sm:max-w-xs">{item.reason}</p>}
      </div>
    ))}</div>
    <p className="mt-3 truncate font-mono text-[11px] text-muted" title={preview.snapshot.assemblyDigest}>assembly {preview.snapshot.assemblyDigest}</p>
  </section>;
}
