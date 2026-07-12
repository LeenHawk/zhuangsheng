import { useState, type FormEvent } from "react";
import { ContactRound } from "lucide-react";

import type { ContextPresetPreviewView, ContextPresetView } from "@zhuangsheng/api-client";
import { Badge, Button, Card, Input, Textarea } from "@zhuangsheng/ui";
import { ContextPreviewPanel } from "./context-preview-panel";

interface InputValue { name: string; characterName: string; identity: string; personality: string; speakingStyle: string; boundaries: string }

export function RolePresetSetupCard({ presets, preview, pending, previewPending, onSubmit, onPreview }: { presets: ContextPresetView[]; preview: ContextPresetPreviewView | null; pending: boolean; previewPending: boolean; onSubmit: (input: InputValue) => Promise<void>; onPreview: (preset: ContextPresetView) => void }) {
  const [form, setForm] = useState<InputValue>({ name: "角色模板", characterName: "", identity: "", personality: "", speakingStyle: "", boundaries: "" });
  const set = (key: keyof InputValue, value: string) => setForm((current) => ({ ...current, [key]: value }));
  const valid = form.name.trim().length > 0 && form.characterName.trim().length > 0 && form.identity.trim().length > 0;
  const submit = async (event: FormEvent) => { event.preventDefault(); if (!valid || pending) return; try { await onSubmit(form); } catch { /* route keeps the typed error */ } };
  return (
    <Card className="p-5">
      <div className="flex items-center gap-2"><ContactRound className="size-5 text-accent" /><h2 className="font-semibold">3. 发布角色 ContextPreset</h2><Badge className="ml-auto" tone={presets.some((item) => item.headVersionId) ? "success" : "warning"}>{presets.filter((item) => item.headVersionId).length} 个已发布模板</Badge></div>
      <p className="mt-2 text-sm leading-6 text-secondary">友好字段会编译成同一份 canonical ContextPreset，不会另存一份 simple settings。</p>
      <form className="mt-4 grid gap-3 md:grid-cols-2" onSubmit={submit}>
        <Field label="模板名称"><Input value={form.name} onChange={(event) => set("name", event.target.value)} /></Field>
        <Field label="角色名称"><Input value={form.characterName} onChange={(event) => set("characterName", event.target.value)} /></Field>
        <Field label="身份与背景"><Textarea value={form.identity} onChange={(event) => set("identity", event.target.value)} /></Field>
        <Field label="性格与目标"><Textarea value={form.personality} onChange={(event) => set("personality", event.target.value)} /></Field>
        <Field label="说话风格"><Textarea value={form.speakingStyle} onChange={(event) => set("speakingStyle", event.target.value)} /></Field>
        <Field label="内容边界"><Textarea value={form.boundaries} onChange={(event) => set("boundaries", event.target.value)} /></Field>
        <div className="md:col-span-2"><Button type="submit" disabled={!valid || pending}>{pending ? "正在发布…" : "发布角色模板"}</Button></div>
      </form>
      {presets.length > 0 && <div className="mt-4 flex flex-wrap gap-2">{presets.map((preset) => <div key={preset.id} className="flex items-center gap-1"><Badge tone={preset.headVersionId ? "success" : "warning"}>{preset.name} · {preset.headVersionId ? "已发布" : "待完成"}</Badge>{preset.headVersionId && <Button size="compact" variant="secondary" disabled={previewPending} onClick={() => onPreview(preset)}>Preview {preset.name}</Button>}</div>)}</div>}
      {preview && <ContextPreviewPanel preview={preview} />}
    </Card>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) { return <label className="block text-xs font-semibold text-secondary">{label}<div className="mt-1.5">{children}</div></label>; }
