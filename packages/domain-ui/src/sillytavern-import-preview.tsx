import { AlertTriangle, CheckCircle2 } from "lucide-react";

import { stringifyJsonExact, type SillyTavernImportPreviewView } from "@zhuangsheng/api-client";
import { Badge, Card } from "@zhuangsheng/ui";

export function SillyTavernImportPreview({
  preview,
  expert,
}: {
  preview: SillyTavernImportPreviewView;
  expert: boolean;
}) {
  const itemCount = Array.isArray(preview.contextSpec?.items)
    ? preview.contextSpec.items.length
    : 0;
  const activeRules = preview.textTransforms.filter((rule) => !rule.disabled).length;
  return <div className="mt-4 space-y-3">
    <div className="grid gap-2 sm:grid-cols-3">
      <Summary label="识别格式" value={kindLabel(preview.kind)} />
      <Summary label="Prompt sections" value={`${itemCount}`} />
      <Summary label="正则规则" value={`${activeRules}/${preview.textTransforms.length} 启用`} />
    </div>
    {preview.warnings.length === 0
      ? <div className="flex items-center gap-2 rounded-xl bg-success/10 px-3 py-2 text-xs text-success"><CheckCircle2 className="size-4" />没有发现兼容性警告</div>
      : <div className="rounded-xl border border-warning/30 bg-warning/5 p-3"><div className="flex items-center gap-2 text-xs font-semibold text-warning"><AlertTriangle className="size-4" />{preview.warnings.length} 条需要确认</div><ul className="mt-2 space-y-1 text-xs text-secondary">{preview.warnings.map((warning, index) => <li key={`${warning.code}-${index}`}>{warning.field ? `${warning.field}：` : ""}{warning.message}</li>)}</ul></div>}
    {preview.generation && <Card className="p-3"><p className="text-xs font-semibold">生成参数</p><pre className="mt-2 overflow-auto text-[11px] text-secondary">{stringifyJsonExact(preview.generation, 2)}</pre></Card>}
    {expert && <ExpertDetails preview={preview} />}
  </div>;
}

function ExpertDetails({ preview }: { preview: SillyTavernImportPreviewView }) {
  return <div className="space-y-3 rounded-xl border border-default p-3">
    <div className="flex flex-wrap gap-2"><Badge tone="info">compat v{preview.compatibilityVersion}</Badge><Badge>{preview.sourceHash}</Badge>{preview.inactiveFields.map((field) => <Badge key={field} tone="warning">inactive · {field}</Badge>)}</div>
    {preview.textTransforms.length > 0 && <div><p className="text-xs font-semibold">Regex execution plan</p><div className="mt-2 space-y-2">{preview.textTransforms.map((rule) => <div key={rule.id} className="rounded-lg bg-elevated p-2 text-xs"><div className="flex flex-wrap items-center gap-2"><span className="font-semibold">{rule.name}</span><Badge>{rule.scope}</Badge>{rule.surfaces.map((surface) => <Badge key={surface} tone="info">{surface}</Badge>)}{rule.disabled && <Badge tone="warning">disabled</Badge>}</div><code className="mt-1 block break-all text-[11px] text-secondary">{rule.findRegex} → {rule.replaceString}</code><p className="mt-1 text-[11px] text-muted">placement: {rule.placements.join(", ") || "none"} · depth {rule.minDepth ?? "*"}..{rule.maxDepth ?? "*"}</p></div>)}</div></div>}
  </div>;
}

function Summary({ label, value }: { label: string; value: string }) {
  return <div className="rounded-xl bg-elevated p-3"><p className="text-[11px] text-muted">{label}</p><p className="mt-1 text-sm font-semibold">{value}</p></div>;
}

function kindLabel(kind: SillyTavernImportPreviewView["kind"]) {
  const labels: Record<SillyTavernImportPreviewView["kind"], string> = {
    open_ai: "OpenAI preset", master: "Master preset", context: "Context template",
    instruct: "Instruct template", system_prompt: "System prompt",
    text_completion: "Text completion", reasoning: "Reasoning template",
    regex_scripts: "Regex scripts", unknown: "Unknown",
  };
  return labels[kind];
}
