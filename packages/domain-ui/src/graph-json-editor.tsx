import { Code2 } from "lucide-react";

import { Textarea } from "@zhuangsheng/ui";

export function GraphJsonEditor({ value, onChange, disabled }: { value: string; onChange: (value: string) => void; disabled?: boolean }) {
  return (
    <section className="flex h-full min-h-[480px] flex-col" aria-label="Canonical GraphDraft JSON editor">
      <div className="mb-2 flex items-center gap-2 text-xs font-semibold text-secondary">
        <Code2 className="size-4" />Canonical GraphDraft JSON
      </div>
      <Textarea
        aria-label="GraphDraft JSON"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        disabled={disabled}
        spellCheck={false}
        className="min-h-[450px] flex-1 resize-none rounded-xl font-mono text-xs leading-5"
      />
      <p className="mt-2 text-[11px] leading-4 text-muted">保存会提交完整文档；画布不会重建或覆盖未知配置。</p>
    </section>
  );
}
