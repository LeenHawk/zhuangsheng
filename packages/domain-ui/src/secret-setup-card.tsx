import { useRef, useState, type FormEvent } from "react";
import { KeyRound, ShieldCheck } from "lucide-react";

import { createIdempotencyKey, type SecretMetadataView, type SecretStoreStatusView } from "@zhuangsheng/api-client";
import { Badge, Button, Card, Input } from "@zhuangsheng/ui";

interface SecretSetupInput { secretId: string; name: string; value: string; masterPassword: string; passwordCommandKey: string; putCommandKey: string }

export function SecretSetupCard({ status, secrets, pending, onSubmit }: { status: SecretStoreStatusView | null; secrets: SecretMetadataView[]; pending: boolean; onSubmit: (input: SecretSetupInput) => Promise<void> }) {
  const [secretId, setSecretId] = useState("provider-api-key");
  const [name, setName] = useState("模型 API Key");
  const [value, setValue] = useState("");
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const passwordKey = useRef<string | null>(null);
  const putKey = useRef<string | null>(null);
  const initializing = status?.initialized === false;
  const valid = Boolean(status) && /^[A-Za-z0-9_.-]{1,128}$/.test(secretId) && value.length > 0 && password.length >= 12 && password.length <= 1024 && (!initializing || password === confirmation);
  const submit = async (event: FormEvent) => {
    event.preventDefault(); if (!valid || pending) return;
    passwordKey.current ??= createIdempotencyKey(); putKey.current ??= createIdempotencyKey();
    try {
      await onSubmit({ secretId, name, value, masterPassword: password, passwordCommandKey: passwordKey.current, putCommandKey: putKey.current });
      setValue(""); setPassword(""); setConfirmation(""); passwordKey.current = null; putKey.current = null;
    } catch { /* retain write-only fields for an explicit retry */ }
  };
  const changePassword = (next: string) => { setPassword(next); passwordKey.current = null; };
  const changeSecret = (next: string) => { setValue(next); putKey.current = null; };
  return (
    <Card className="p-5">
      <div className="flex items-center gap-2"><KeyRound className="size-5 text-accent" /><h2 className="font-semibold">1. 安全保存模型凭据</h2><Badge className="ml-auto" tone={secrets.length ? "success" : "warning"}>{secrets.length ? `${secrets.length} 个 SecretRef` : "尚未配置"}</Badge></div>
      <p className="mt-2 text-sm leading-6 text-secondary">API key 只进入专用 Secret Store 写入请求。页面只保留 SecretRef，提交成功后清空明文。</p>
      <form className="mt-4 grid gap-3 sm:grid-cols-2" onSubmit={submit}>
        <Field label="Secret ID"><Input value={secretId} onChange={(event) => { setSecretId(event.target.value); putKey.current = null; }} autoComplete="off" /></Field>
        <Field label="显示名称"><Input value={name} onChange={(event) => { setName(event.target.value); putKey.current = null; }} /></Field>
        <Field label="API key"><Input type="password" value={value} onChange={(event) => changeSecret(event.target.value)} autoComplete="off" /></Field>
        <Field label={initializing ? "设置主密码" : "主密码（取得本次写入会话）"}><Input type="password" value={password} onChange={(event) => changePassword(event.target.value)} minLength={12} maxLength={1024} autoComplete={initializing ? "new-password" : "current-password"} /></Field>
        {initializing && <Field label="确认主密码"><Input type="password" value={confirmation} onChange={(event) => setConfirmation(event.target.value)} minLength={12} maxLength={1024} autoComplete="new-password" /></Field>}
        <div className="flex items-end"><Button type="submit" disabled={!valid || pending}>{pending ? "正在安全保存…" : <><ShieldCheck className="size-4" />保存凭据</>}</Button></div>
      </form>
      {initializing && confirmation && password !== confirmation && <p className="mt-2 text-xs text-danger">两次主密码输入不一致。</p>}
      {secrets.length > 0 && <div className="mt-4 flex flex-wrap gap-2">{secrets.map((secret) => <Badge key={secret.secretRef.id}>{secret.name || secret.secretRef.id} · {secret.secretRef.id}</Badge>)}</div>}
    </Card>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return <label className="block text-xs font-semibold text-secondary">{label}<div className="mt-1.5">{children}</div></label>;
}
