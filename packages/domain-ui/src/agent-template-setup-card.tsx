import { useEffect, useState, type FormEvent } from "react";
import { Bot, CheckCircle2 } from "lucide-react";

import type { ChannelView, ContextPresetView, RolePlayGraphOptionView, RolePlaySettingsView } from "@zhuangsheng/api-client";
import { Badge, Button, Card, Input } from "@zhuangsheng/ui";
import { RolePlaySettingsPanel } from "./roleplay-settings-panel";

interface InputValue { name: string; channelId: string; presetId: string }

interface Props { channels: ChannelView[]; presets: ContextPresetView[]; templates: RolePlayGraphOptionView[]; settings: RolePlaySettingsView | null; pending: boolean; settingsPending: boolean; onSubmit: (input: InputValue) => Promise<unknown>; onInspect: (template: RolePlayGraphOptionView) => void }

export function AgentTemplateSetupCard({ channels, presets, templates, settings, pending, settingsPending, onSubmit, onInspect }: Props) {
  const usableChannels = channels.filter((item) => item.headRevisionId);
  const usablePresets = presets.filter((item) => item.headVersionId);
  const [form, setForm] = useState<InputValue>({ name: "Role Play Agent", channelId: usableChannels[0]?.id ?? "", presetId: usablePresets[0]?.id ?? "" });
  useEffect(() => { if (!form.channelId && usableChannels[0]) setForm((value) => ({ ...value, channelId: usableChannels[0]!.id })); }, [form.channelId, usableChannels]);
  useEffect(() => { if (!form.presetId && usablePresets[0]) setForm((value) => ({ ...value, presetId: usablePresets[0]!.id })); }, [form.presetId, usablePresets]);
  const set = (key: keyof InputValue, value: string) => setForm((current) => ({ ...current, [key]: value }));
  const valid = form.name.trim().length > 0 && form.channelId.length > 0 && form.presetId.length > 0;
  const submit = async (event: FormEvent) => { event.preventDefault(); if (!valid || pending) return; try { await onSubmit({ ...form, name: form.name.trim() }); } catch { /* route renders typed error */ } };
  return (
    <Card className="border-accent/25 p-5">
      <div className="flex items-center gap-2"><Bot className="size-5 text-accent" /><h2 className="font-semibold">4. 创建可运行的 Agent 模板</h2><Badge className="ml-auto" tone={templates.length ? "success" : "warning"}>{templates.length} 个可用于故事</Badge></div>
      <p className="mt-2 text-sm leading-6 text-secondary">服务端会生成精确 Conversation 合同、保存 GraphDraft 并 Apply；浏览器不复制 schema 或猜测 LLM 节点。</p>
      <form className="mt-4 grid gap-3 md:grid-cols-2" onSubmit={submit}>
        <Field label="模板名称"><Input value={form.name} onChange={(event) => set("name", event.target.value)} /></Field>
        <Field label="模型 Channel"><select className="min-h-11 w-full rounded-xl border border-default bg-canvas px-3 text-sm" value={form.channelId} onChange={(event) => set("channelId", event.target.value)}><option value="" disabled>选择已发布 Channel</option>{usableChannels.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></Field>
        <Field label="角色 ContextPreset"><select className="min-h-11 w-full rounded-xl border border-default bg-canvas px-3 text-sm" value={form.presetId} onChange={(event) => set("presetId", event.target.value)}><option value="" disabled>选择已发布角色</option>{usablePresets.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></Field>
        <div className="flex items-end"><Button type="submit" disabled={!valid || pending}>{pending ? "正在创建并 Apply…" : "创建 Agent 模板"}</Button></div>
      </form>
      {templates.length > 0 && <ul className="mt-4 space-y-2">{templates.map((template) => <li key={template.revisionId} className="rounded-xl bg-success/5 px-3 py-2 text-sm"><div className="flex items-center gap-2"><CheckCircle2 className="size-4 text-success" /><span>{template.graphName}</span><Badge className="ml-auto" tone={template.compatibility.mode === "editable" ? "success" : "warning"}>{template.compatibility.mode}</Badge>{template.primaryLlmNodeId && <Button size="compact" variant="secondary" disabled={settingsPending} onClick={() => onInspect(template)}>{settingsPending ? "正在读取…" : "查看设置"}</Button>}</div>{settings?.revisionId === template.revisionId && <RolePlaySettingsPanel settings={settings} />}</li>)}</ul>}
    </Card>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) { return <label className="block text-xs font-semibold text-secondary">{label}<div className="mt-1.5">{children}</div></label>; }
