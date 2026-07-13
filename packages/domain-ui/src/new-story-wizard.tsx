import { useRef, useState } from "react";
import { ArrowLeft, ArrowRight, Check, LockOpen } from "lucide-react";

import { createIdempotencyKey, type ConversationRunSpec, type RolePlayGraphOptionView, type RolePlaySettingsView, type SecretStoreStatusView } from "@zhuangsheng/api-client";
import { Badge, Button, Card, Input, Textarea } from "@zhuangsheng/ui";

interface Props {
  templates: RolePlayGraphOptionView[];
  settings: Record<string, RolePlaySettingsView | null>;
  secretStatus: SecretStoreStatusView | null;
  pending: boolean;
  onSubmit: (title: string | undefined, run: ConversationRunSpec, openingMessage: string) => Promise<void>;
  onUnlock: (password: string, idempotencyKey: string) => Promise<void>;
  onClose: () => void;
}

export function NewStoryWizard(props: Props) {
  const available = props.templates.filter((template) => template.replyOutputKeys.length > 0 && template.compatibility.mode !== "expert_only");
  const [step, setStep] = useState(1);
  const [revisionId, setRevisionId] = useState(available[0]?.revisionId ?? "");
  const [title, setTitle] = useState("");
  const [opening, setOpening] = useState("");
  const [password, setPassword] = useState("");
  const [unlocking, setUnlocking] = useState(false);
  const [unlockError, setUnlockError] = useState<string | null>(null);
  const unlockKey = useRef<string | null>(null);
  const selected = available.find((template) => template.revisionId === revisionId) ?? available[0];
  const detail = selected ? props.settings[selected.revisionId] ?? null : null;
  const unlock = async () => {
    if (password.length < 12 || unlocking) return;
    unlockKey.current ??= createIdempotencyKey(); setUnlocking(true); setUnlockError(null);
    try { await props.onUnlock(password, unlockKey.current); setPassword(""); unlockKey.current = null; }
    catch { setUnlockError("解锁失败；请检查主密码后重试。"); }
    finally { setUnlocking(false); }
  };
  const submit = async () => {
    if (!selected || !opening.trim() || props.pending) return;
    await props.onSubmit(title.trim() || undefined, {
      graphRevisionId: selected.revisionId,
      replyOutputKey: selected.replyOutputKeys[0]!,
      inputShape: "conversation_message_v1",
    }, opening.trim());
  };
  return (
    <Card className="mt-6 overflow-hidden">
      <div className="border-b border-default px-5 py-4">
        <div className="flex flex-wrap items-center gap-2">
          <Badge tone="info">新故事 · 第 {step} / 4 步</Badge>
          {["角色与模板", "Persona 与世界", "模型与能力", "开场检查"].map((label, index) => (
            <span key={label} className={`text-xs ${step === index + 1 ? "font-semibold text-primary" : "text-muted"}`}>
              {index + 1}. {label}
            </span>
          ))}
        </div>
      </div>
      <div className="p-5">
        {step === 1 && <TemplateStep templates={available} selected={selected} onSelect={setRevisionId} />}
        {step === 2 && <ContextStep selected={selected} detail={detail} />}
        {step === 3 && (
          <CapabilityStep selected={selected} detail={detail} status={props.secretStatus}
            password={password} unlocking={unlocking} error={unlockError}
            onPassword={(value) => { setPassword(value); unlockKey.current = null; }}
            onUnlock={() => void unlock()} />
        )}
        {step === 4 && (
          <OpeningStep selected={selected} detail={detail} title={title} opening={opening}
            onTitle={setTitle} onOpening={setOpening} />
        )}
      </div>
      <div className="flex flex-wrap justify-between gap-2 border-t border-default px-5 py-4">
        <div className="flex gap-2"><Button variant="ghost" onClick={props.onClose}>取消</Button>{step > 1 && <Button variant="secondary" onClick={() => setStep((value) => value - 1)}><ArrowLeft className="size-4" />上一步</Button>}</div>
        {step < 4 ? <Button disabled={!selected} onClick={() => setStep((value) => value + 1)}>下一步<ArrowRight className="size-4" /></Button> : <Button disabled={!selected || !opening.trim() || props.pending} onClick={() => void submit()}>{props.pending ? "正在创建并提交首个 Turn…" : "创建故事并开始"}</Button>}
      </div>
    </Card>
  );
}

function TemplateStep({ templates, selected, onSelect }: { templates: RolePlayGraphOptionView[]; selected?: RolePlayGraphOptionView; onSelect: (id: string) => void }) {
  return <section><h2 className="font-display text-xl font-bold">选择角色与 Agent 模板</h2><p className="mt-1 text-sm text-secondary">只显示已 Apply 且能输出角色回复合同的版本。</p><div className="mt-4 grid gap-3 sm:grid-cols-2">{templates.map((template) => <label key={template.revisionId} className={`cursor-pointer rounded-2xl border p-4 ${selected?.revisionId === template.revisionId ? "border-accent bg-accent-soft" : "border-default"}`}><input className="sr-only" type="radio" name="story-template" value={template.revisionId} checked={selected?.revisionId === template.revisionId} onChange={() => onSelect(template.revisionId)} /><span className="font-semibold">{template.graphName}</span><span className="mt-1 block font-mono text-xs text-muted">revision {template.revisionNo} · {template.revisionId}</span><span className="mt-2 block text-xs text-secondary">{compatibilitySummary(template)}</span></label>)}</div></section>;
}

function ContextStep({ selected, detail }: { selected?: RolePlayGraphOptionView; detail: RolePlaySettingsView | null }) {
  return <section><h2 className="font-display text-xl font-bold">Persona 与世界来源</h2><p className="mt-1 text-sm text-secondary">本故事会固定使用模板绑定的版本化 Context；不会创建浏览器私有 override。</p><div className="mt-4 rounded-2xl border border-default bg-elevated p-4"><p className="font-semibold">{selected?.graphName}</p><dl className="mt-3 grid gap-3 text-sm sm:grid-cols-2"><Item label="ContextPreset" value={detail?.contextPresetId ?? "正在读取版本映射"} /><Item label="可编辑范围" value={editableFields(selected)} /></dl><p className="mt-4 text-xs text-muted">角色 Persona、用户 Persona、世界与历史装配顺序来自该 ContextPreset。需要新建内容时先发布新模板版本，再返回选择。</p></div></section>;
}

function CapabilityStep(props: { selected?: RolePlayGraphOptionView; detail: RolePlaySettingsView | null; status: SecretStoreStatusView | null; password: string; unlocking: boolean; error: string | null; onPassword: (value: string) => void; onUnlock: () => void }) {
  const streaming = props.detail?.streaming;
  return <section><h2 className="font-display text-xl font-bold">模型与能力</h2><p className="mt-1 text-sm text-secondary">检查本次故事会固定的模型、生成档位和权限边界。</p><dl className="mt-4 grid gap-3 rounded-2xl border border-default bg-elevated p-4 text-sm sm:grid-cols-2"><Item label="模型" value={props.detail?.model.modelName ?? props.detail?.model.modelId ?? "由模板固定"} /><Item label="LLM Node" value={props.selected?.primaryLlmNodeId ?? "未映射"} /><Item label="Streaming" value={streaming?.enabled ? `${streaming.audience} audience` : "关闭"} /><Item label="能力边界" value={capabilitySummary(props.selected)} /></dl>{props.status?.initialized && props.status.locked && <div className="mt-4 rounded-2xl border border-warning/30 bg-warning/5 p-4"><p className="text-sm font-semibold text-warning">Secret Store 已锁定</p><p className="mt-1 text-xs text-secondary">若该模型需要凭据，请在创建前于此解锁；密码不会成为普通 wait response。</p><div className="mt-3 flex flex-col gap-2 sm:flex-row"><Input aria-label="向导主密码" type="password" value={props.password} onChange={(event) => props.onPassword(event.target.value)} minLength={12} maxLength={1024} autoComplete="current-password" /><Button disabled={props.password.length < 12 || props.unlocking} onClick={props.onUnlock}><LockOpen className="size-4" />{props.unlocking ? "解锁中…" : "在此解锁"}</Button></div>{props.error && <p className="mt-2 text-xs text-danger">{props.error}</p>}</div>}{props.status?.initialized && !props.status.locked && <p className="mt-4 text-sm text-success"><Check className="mr-1 inline size-4" />Secret Store 当前进程已解锁。</p>}</section>;
}

function OpeningStep(props: { selected?: RolePlayGraphOptionView; detail: RolePlaySettingsView | null; title: string; opening: string; onTitle: (value: string) => void; onOpening: (value: string) => void }) {
  return <section><h2 className="font-display text-xl font-bold">开场检查</h2><p className="mt-1 text-sm text-secondary">最终提交才会创建 Conversation，并用返回的 root head 原子提交首个 Turn。</p><div className="mt-4 grid gap-3 sm:grid-cols-2"><label className="text-sm font-semibold">故事名称<span className="mt-1 block text-xs font-normal text-muted">可留空，之后仍可显示为未命名故事。</span><Input className="mt-2" value={props.title} onChange={(event) => props.onTitle(event.target.value)} maxLength={200} /></label><div className="rounded-2xl bg-elevated p-4 text-sm"><p className="font-semibold">{props.selected?.graphName}</p><p className="mt-1 text-muted">{props.detail?.model.modelName ?? props.detail?.model.modelId ?? "固定模型"}</p><p className="mt-1 font-mono text-xs text-muted">{props.selected?.revisionId}</p></div></div><label className="mt-4 block text-sm font-semibold">首条消息<Textarea className="mt-2" value={props.opening} onChange={(event) => props.onOpening(event.target.value)} placeholder="从一个明确的场景或行动开始…" maxLength={64 * 1024} autoFocus /></label></section>;
}

function Item({ label, value }: { label: string; value: string }) {
  return <div><dt className="text-xs text-muted">{label}</dt><dd className="mt-1 break-words font-medium">{value}</dd></div>;
}

function compatibilitySummary(template: RolePlayGraphOptionView): string {
  if (template.compatibility.mode === "editable") return "用户模式可完整映射";
  if (template.compatibility.mode === "partial") return `部分兼容：${template.compatibility.lockedReasons.join("、")}`;
  return `专家专用：${template.compatibility.reasons.join("、")}`;
}
function editableFields(template?: RolePlayGraphOptionView): string {
  return template && template.compatibility.mode !== "expert_only" ? template.compatibility.editableFields.join("、") || "模板定义" : "只读";
}
function capabilitySummary(template?: RolePlayGraphOptionView): string {
  return template?.compatibility.mode === "partial" ? template.compatibility.lockedReasons.join("、") : "用户模式可映射";
}
