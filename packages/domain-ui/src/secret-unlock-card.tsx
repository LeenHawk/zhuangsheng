import { useState, type FormEvent } from "react";
import { LockKeyhole } from "lucide-react";

import type { SecretStoreStatusView, WaitView } from "@zhuangsheng/api-client";
import { Badge, Button, Card, Input } from "@zhuangsheng/ui";

interface SecretUnlockCardProps {
  wait: WaitView;
  status: SecretStoreStatusView | null;
  pending: boolean;
  error: string | null;
  onSubmit: (wait: WaitView, mode: "initialize" | "unlock", password: string) => Promise<void>;
}

export function SecretUnlockCard(props: SecretUnlockCardProps) {
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const mode = props.status?.initialized === false ? "initialize" : "unlock";
  const valid = password.length >= 12 && password.length <= 1024 &&
    (mode === "unlock" || password === confirmation);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!valid || props.pending || props.status?.locked === false) return;
    try {
      await props.onSubmit(props.wait, mode, password);
      setPassword("");
      setConfirmation("");
    } catch {
      // Keep the write-only form in memory so the user can correct and retry it.
    }
  };
  return (
    <Card className="border-warning/30 p-5">
      <div className="flex items-center gap-2">
        <LockKeyhole className="size-5 text-warning" />
        <h2 className="font-semibold">需要安全存储</h2>
        <Badge className="ml-auto" tone="warning">本机凭据</Badge>
      </div>
      <p className="mt-2 text-sm leading-6 text-secondary">
        角色需要已授权的模型连接。主密码只发送到专用解锁接口，不会进入故事、事件或浏览器持久化缓存。
      </p>
      {!props.status ? (
        <p className="mt-4 text-sm text-muted">正在读取安全存储状态…</p>
      ) : !props.status.locked ? (
        <p className="mt-4 rounded-xl bg-elevated p-3 text-sm text-success">安全存储已解锁，正在恢复本次回复。</p>
      ) : (
        <form className="mt-4 space-y-3" onSubmit={submit}>
          <label className="block text-xs font-semibold text-secondary">
            {mode === "initialize" ? "设置主密码" : "主密码"}
            <Input
              className="mt-1.5"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              minLength={12}
              maxLength={1024}
              autoComplete={mode === "initialize" ? "new-password" : "current-password"}
            />
          </label>
          {mode === "initialize" && (
            <label className="block text-xs font-semibold text-secondary">
              确认主密码
              <Input className="mt-1.5" type="password" value={confirmation} onChange={(event) => setConfirmation(event.target.value)} minLength={12} maxLength={1024} autoComplete="new-password" />
            </label>
          )}
          {mode === "initialize" && confirmation && password !== confirmation && <p className="text-xs text-danger">两次输入不一致。</p>}
          {props.error && <p className="text-sm text-danger">{props.error}</p>}
          <Button type="submit" disabled={!valid || props.pending}>
            {props.pending ? "正在安全处理…" : mode === "initialize" ? "初始化并解锁" : "解锁并继续"}
          </Button>
        </form>
      )}
    </Card>
  );
}
