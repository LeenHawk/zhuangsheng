import { useState, type FormEvent } from "react";
import { Cable, CheckCircle2 } from "lucide-react";

import type { ChannelView, GenerationProviderKind, SecretMetadataView } from "@zhuangsheng/api-client";
import { Badge, Button, Card, Input } from "@zhuangsheng/ui";

interface InputValue { name: string; baseUrl: string; providerKind: GenerationProviderKind; modelId: string; credentialSecretId: string | null }
const providers: Array<{ value: GenerationProviderKind; label: string; baseUrl: string }> = [
  { value: "open_ai_responses", label: "OpenAI Responses", baseUrl: "https://api.openai.com/v1" },
  { value: "open_ai_chat_completions", label: "OpenAI Chat Completions", baseUrl: "https://api.openai.com/v1" },
  { value: "claude_messages", label: "Claude Messages", baseUrl: "https://api.anthropic.com/v1" },
  { value: "gemini_generate_content", label: "Gemini Generate Content", baseUrl: "https://generativelanguage.googleapis.com/v1beta" },
];

export function ChannelSetupCard({ channels, secrets, pending, onSubmit }: { channels: ChannelView[]; secrets: SecretMetadataView[]; pending: boolean; onSubmit: (input: InputValue) => Promise<void> }) {
  const [form, setForm] = useState<InputValue>({ name: "Primary model", baseUrl: providers[0]!.baseUrl, providerKind: providers[0]!.value, modelId: "", credentialSecretId: secrets[0]?.secretRef.id ?? null });
  const set = <K extends keyof InputValue>(key: K, value: InputValue[K]) => setForm((current) => ({ ...current, [key]: value }));
  const valid = form.name.trim().length > 0 && form.baseUrl.trim().length > 0 && form.modelId.trim().length > 0;
  const submit = async (event: FormEvent) => { event.preventDefault(); if (!valid || pending) return; try { await onSubmit({ ...form, name: form.name.trim(), baseUrl: form.baseUrl.trim(), modelId: form.modelId.trim() }); } catch { /* command error is rendered by the route */ } };
  return (
    <Card className="p-5">
      <div className="flex items-center gap-2"><Cable className="size-5 text-accent" /><h2 className="font-semibold">2. 发布模型连接</h2><Badge className="ml-auto" tone={channels.some((item) => item.headRevisionId) ? "success" : "warning"}>{channels.filter((item) => item.headRevisionId).length} 个可用 Channel</Badge></div>
      <p className="mt-2 text-sm leading-6 text-secondary">连接配置会发布为不可变 Channel revision；模型被显式加入 allowlist。</p>
      <form className="mt-4 grid gap-3 md:grid-cols-2" onSubmit={submit}>
        <Field label="连接名称"><Input value={form.name} onChange={(event) => set("name", event.target.value)} /></Field>
        <Field label="协议"><select className="min-h-11 w-full rounded-xl border border-default bg-canvas px-3 text-sm" value={form.providerKind} onChange={(event) => { const provider = providers.find((item) => item.value === event.target.value)!; setForm((current) => ({ ...current, providerKind: provider.value, baseUrl: provider.baseUrl })); }}>{providers.map((provider) => <option key={provider.value} value={provider.value}>{provider.label}</option>)}</select></Field>
        <Field label="Base URL"><Input value={form.baseUrl} onChange={(event) => set("baseUrl", event.target.value)} inputMode="url" /></Field>
        <Field label="Model ID"><Input value={form.modelId} onChange={(event) => set("modelId", event.target.value)} placeholder="例如 gpt-4.1-mini" /></Field>
        <Field label="凭据"><select className="min-h-11 w-full rounded-xl border border-default bg-canvas px-3 text-sm" value={form.credentialSecretId ?? ""} onChange={(event) => set("credentialSecretId", event.target.value || null)}><option value="">无需认证（仅本地/明确允许）</option>{secrets.map((secret) => <option key={secret.secretRef.id} value={secret.secretRef.id}>{secret.name || secret.secretRef.id}</option>)}</select></Field>
        <div className="flex items-end"><Button type="submit" disabled={!valid || pending}>{pending ? "正在发布…" : "发布 Channel"}</Button></div>
      </form>
      {channels.length > 0 && <ul className="mt-4 space-y-2">{channels.map((channel) => <li key={channel.id} className="flex items-center gap-2 rounded-xl bg-elevated px-3 py-2 text-sm"><CheckCircle2 className={`size-4 ${channel.headRevisionId ? "text-success" : "text-warning"}`} /><span>{channel.name}</span><span className="ml-auto text-xs text-muted">{channel.headRevisionId ? "已发布" : "待完成发布"}</span></li>)}</ul>}
    </Card>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) { return <label className="block text-xs font-semibold text-secondary">{label}<div className="mt-1.5">{children}</div></label>; }
