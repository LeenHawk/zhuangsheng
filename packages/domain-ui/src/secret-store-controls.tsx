import { useRef, useState, type FormEvent } from "react";
import { KeyRound, Lock, LockOpen, RefreshCw } from "lucide-react";

import { createIdempotencyKey, type SecretStoreStatusView } from "@zhuangsheng/api-client";
import { Badge, Button, Card, Dialog, DialogContent, DialogDescription, DialogTitle, Input } from "@zhuangsheng/ui";

type Action = "unlock" | "change";

interface Props {
  status: SecretStoreStatusView;
  pending: boolean;
  onUnlock: (password: string, idempotencyKey: string) => Promise<void>;
  onLock: (idempotencyKey: string) => Promise<void>;
  onChangePassword: (currentPassword: string, newPassword: string, unlockKey: string, changeKey: string) => Promise<void>;
}

export function SecretStoreControls({ status, pending, onUnlock, onLock, onChangePassword }: Props) {
  const [action, setAction] = useState<Action | null>(null);
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const firstKey = useRef<string | null>(null);
  const secondKey = useRef<string | null>(null);
  const lockKey = useRef<string | null>(null);
  const reset = () => {
    setCurrent(""); setNext(""); setConfirmation("");
    firstKey.current = null; secondKey.current = null;
  };
  const close = () => { reset(); setAction(null); };
  const lock = async () => {
    if (pending) return;
    lockKey.current ??= createIdempotencyKey();
    try { await onLock(lockKey.current); lockKey.current = null; }
    catch { /* keep the command key stable for an explicit retry */ }
  };
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (pending || current.length < 12 || (action === "change" && (next.length < 12 || next !== confirmation))) return;
    firstKey.current ??= createIdempotencyKey();
    try {
      if (action === "unlock") await onUnlock(current, firstKey.current);
      if (action === "change") {
        secondKey.current ??= createIdempotencyKey();
        await onChangePassword(current, next, firstKey.current, secondKey.current);
      }
      close();
    } catch { /* keep the dedicated form available for an explicit retry */ }
  };
  return (
    <Card className="p-5">
      <div className="flex flex-wrap items-center gap-2"><KeyRound className="size-5 text-accent" /><h2 className="font-semibold">Secret Store 状态</h2><Badge className="ml-auto" tone={status.locked ? "warning" : "success"}>{status.locked ? "已锁定" : "当前进程已解锁"}</Badge></div>
      <p className="mt-2 text-sm text-secondary">解锁会话只存在于当前进程；锁定和修改主密码不会把明文写入页面状态之外的共享存储。</p>
      <div className="mt-4 flex flex-wrap gap-2">
        {status.locked ? <Button size="compact" onClick={() => setAction("unlock")}><LockOpen className="size-3.5" />解锁</Button> : <Button size="compact" variant="secondary" disabled={pending} onClick={() => void lock()}><Lock className="size-3.5" />立即锁定</Button>}
        <Button size="compact" variant="secondary" onClick={() => setAction("change")}><RefreshCw className="size-3.5" />修改主密码</Button>
      </div>
      <Dialog open={action !== null} onOpenChange={(open) => { if (!open) close(); }}>
        <DialogContent onCloseAutoFocus={reset}>
          <DialogTitle>{action === "change" ? "修改 Secret Store 主密码" : "解锁 Secret Store"}</DialogTitle>
          <DialogDescription>敏感字段只用于本次命令。关闭弹窗或提交成功后会立即从组件状态清空。</DialogDescription>
          <form className="mt-5 space-y-3" onSubmit={submit}>
            <Field label="当前主密码"><Input autoFocus type="password" value={current} onChange={(event) => { setCurrent(event.target.value); firstKey.current = null; }} minLength={12} maxLength={1024} autoComplete="current-password" /></Field>
            {action === "change" && <><Field label="新主密码"><Input type="password" value={next} onChange={(event) => { setNext(event.target.value); secondKey.current = null; }} minLength={12} maxLength={1024} autoComplete="new-password" /></Field><Field label="确认新主密码"><Input type="password" value={confirmation} onChange={(event) => setConfirmation(event.target.value)} minLength={12} maxLength={1024} autoComplete="new-password" /></Field>{confirmation && next !== confirmation && <p className="text-xs text-danger">两次新主密码输入不一致。</p>}</>}
            <div className="flex justify-end gap-2 pt-2"><Button type="button" variant="ghost" onClick={close}>取消</Button><Button type="submit" disabled={pending || current.length < 12 || (action === "change" && (next.length < 12 || next !== confirmation))}>{pending ? "处理中…" : action === "change" ? "确认修改" : "解锁"}</Button></div>
          </form>
        </DialogContent>
      </Dialog>
    </Card>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return <label className="block text-xs font-semibold text-secondary">{label}<div className="mt-1.5">{children}</div></label>;
}
