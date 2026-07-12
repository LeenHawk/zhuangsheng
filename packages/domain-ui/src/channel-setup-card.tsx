import { useEffect, useState, type FormEvent } from "react";
import { Cable, CheckCircle2, RefreshCw } from "lucide-react";

import type { ChannelModelDiscoveryView, ChannelView, DiscoveredChannelModel, GenerationProviderKind, SecretMetadataView } from "@zhuangsheng/api-client";
import { Badge, Button, Card, Input } from "@zhuangsheng/ui";

interface InputValue { name: string; baseUrl: string; providerKind: GenerationProviderKind; modelId: string; credentialSecretId: string | null; structuredOutput: boolean }
const providers: Array<{ value: GenerationProviderKind; label: string; baseUrl: string }> = [
  { value: "open_ai_responses", label: "OpenAI Responses", baseUrl: "https://api.openai.com/v1" },
  { value: "open_ai_chat_completions", label: "OpenAI Chat Completions", baseUrl: "https://api.openai.com/v1" },
  { value: "claude_messages", label: "Claude Messages", baseUrl: "https://api.anthropic.com/v1" },
  { value: "gemini_generate_content", label: "Gemini Generate Content", baseUrl: "https://generativelanguage.googleapis.com/v1beta" },
];

interface Props {
  channels: ChannelView[];
  secrets: SecretMetadataView[];
  discovery: ChannelModelDiscoveryView | null;
  publishPending: boolean;
  discoveryPending: boolean;
  onSubmit: (input: InputValue) => Promise<void>;
  onDiscover: (channel: ChannelView) => void;
  onPublishDiscovered: (model: DiscoveredChannelModel, structuredOutput: boolean) => Promise<void>;
}

export function ChannelSetupCard(props: Props) {
  const [form, setForm] = useState<InputValue>({ name: "Primary model", baseUrl: providers[0]!.baseUrl, providerKind: providers[0]!.value, modelId: "", credentialSecretId: props.secrets[0]?.secretRef.id ?? null, structuredOutput: false });
  const set = <K extends keyof InputValue>(key: K, value: InputValue[K]) => setForm((current) => ({ ...current, [key]: value }));
  const valid = form.name.trim().length > 0 && form.baseUrl.trim().length > 0 && form.modelId.trim().length > 0 && form.structuredOutput;
  const submit = async (event: FormEvent) => { event.preventDefault(); if (!valid || props.publishPending) return; try { await props.onSubmit({ ...form, name: form.name.trim(), baseUrl: form.baseUrl.trim(), modelId: form.modelId.trim() }); } catch { /* command error is rendered by the route */ } };
  return (
    <Card className="p-5">
      <div className="flex items-center gap-2"><Cable className="size-5 text-accent" /><h2 className="font-semibold">2. 发布模型连接</h2><Badge className="ml-auto" tone={props.channels.some((item) => item.headRevisionId) ? "success" : "warning"}>{props.channels.filter((item) => item.headRevisionId).length} 个可用 Channel</Badge></div>
      <p className="mt-2 text-sm leading-6 text-secondary">连接配置会发布为不可变 Channel revision；模型被显式加入 allowlist。</p>
      <form className="mt-4 grid gap-3 md:grid-cols-2" onSubmit={submit}>
        <Field label="连接名称"><Input value={form.name} onChange={(event) => set("name", event.target.value)} /></Field>
        <Field label="协议"><select className="min-h-11 w-full rounded-xl border border-default bg-canvas px-3 text-sm" value={form.providerKind} onChange={(event) => { const provider = providers.find((item) => item.value === event.target.value)!; setForm((current) => ({ ...current, providerKind: provider.value, baseUrl: provider.baseUrl })); }}>{providers.map((provider) => <option key={provider.value} value={provider.value}>{provider.label}</option>)}</select></Field>
        <Field label="Base URL"><Input value={form.baseUrl} onChange={(event) => set("baseUrl", event.target.value)} inputMode="url" /></Field>
        <Field label="Model ID"><Input value={form.modelId} onChange={(event) => set("modelId", event.target.value)} placeholder="例如 gpt-4.1-mini" /></Field>
        <Field label="凭据"><select className="min-h-11 w-full rounded-xl border border-default bg-canvas px-3 text-sm" value={form.credentialSecretId ?? ""} onChange={(event) => set("credentialSecretId", event.target.value || null)}><option value="">无需认证（仅本地/明确允许）</option>{props.secrets.map((secret) => <option key={secret.secretRef.id} value={secret.secretRef.id}>{secret.name || secret.secretRef.id}</option>)}</select></Field>
        <label className="flex items-center gap-2 text-sm text-secondary"><input type="checkbox" checked={form.structuredOutput} onChange={(event) => set("structuredOutput", event.target.checked)} />我确认该模型支持结构化 JSON 输出（角色回复合同需要）</label>
        <div className="flex items-end"><Button type="submit" disabled={!valid || props.publishPending}>{props.publishPending ? "正在发布…" : "发布 Channel"}</Button></div>
      </form>
      {props.channels.length > 0 && <ul className="mt-4 space-y-2">{props.channels.map((channel) => <li key={channel.id} className="rounded-xl bg-elevated px-3 py-2 text-sm"><div className="flex items-center gap-2"><CheckCircle2 className={`size-4 ${channel.headRevisionId ? "text-success" : "text-warning"}`} /><span>{channel.name}</span><span className="ml-auto text-xs text-muted">{channel.headRevisionId ? "已发布" : "待完成发布"}</span>{channel.headRevisionId && <Button size="compact" variant="secondary" disabled={props.discoveryPending} onClick={() => props.onDiscover(channel)}><RefreshCw className={`size-3.5 ${props.discoveryPending ? "animate-spin" : ""}`} />发现模型</Button>}</div>{props.discovery?.channelId === channel.id && <ModelDiscovery discovery={props.discovery} pending={props.publishPending} onPublish={props.onPublishDiscovered} />}</li>)}</ul>}
    </Card>
  );
}

function ModelDiscovery({ discovery, pending, onPublish }: { discovery: ChannelModelDiscoveryView; pending: boolean; onPublish: Props["onPublishDiscovered"] }) {
  const [modelId, setModelId] = useState(discovery.models[0]?.id ?? "");
  const [structuredOutput, setStructuredOutput] = useState(false);
  useEffect(() => { setModelId(discovery.models[0]?.id ?? ""); setStructuredOutput(false); }, [discovery]);
  const model = discovery.models.find((item) => item.id === modelId);
  return <div className="mt-3 border-t border-default pt-3"><p className="text-xs text-secondary">发现结果是临时视图。选择模型后会显式发布新 revision，不会自动改写当前配置。</p><div className="mt-3 grid gap-3 md:grid-cols-2"><Field label={`可用模型（${discovery.models.length}）`}><select className="min-h-11 w-full rounded-xl border border-default bg-canvas px-3 text-sm" value={modelId} onChange={(event) => setModelId(event.target.value)}>{discovery.models.map((item) => <option key={item.id} value={item.id}>{item.name ? `${item.name} · ${item.id}` : item.id}</option>)}</select></Field><label className="flex items-center gap-2 text-xs text-secondary"><input type="checkbox" checked={structuredOutput} onChange={(event) => setStructuredOutput(event.target.checked)} />我确认所选模型支持结构化 JSON 输出</label><div><Button size="compact" disabled={!model || !structuredOutput || pending} onClick={() => model && void onPublish(model, structuredOutput).catch(() => undefined)}>{pending ? "正在发布所选模型…" : "发布所选模型"}</Button></div></div></div>;
}

function Field({ label, children }: { label: string; children: React.ReactNode }) { return <label className="block text-xs font-semibold text-secondary">{label}<div className="mt-1.5">{children}</div></label>; }
