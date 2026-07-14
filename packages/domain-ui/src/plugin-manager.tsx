import { useState } from "react";
import { GitBranch, Puzzle, RefreshCw } from "lucide-react";

import type { PluginClient, PluginInstallationView, PluginUpdatePolicy, SecretMetadataView } from "@zhuangsheng/api-client";
import { Badge, Button, Card, Input } from "@zhuangsheng/ui";
import { loadPluginRendererPreference, savePluginRendererPreference, usePluginHost } from "@zhuangsheng/ui-extension-host";

import { usePluginManager } from "./use-plugin-manager";

export function PluginManager({ client, secrets }: { client: PluginClient; secrets: SecretMetadataView[] }) {
  const manager = usePluginManager(client);
  const host = usePluginHost();
  const [source, setSource] = useState({ sourceUrl: "", sourceRef: "", credentialSecretId: "", credentialUsername: "" });
  const [candidatePolicy, setCandidatePolicy] = useState<PluginUpdatePolicy>("notify");
  const [renderer, setRenderer] = useState(() => loadPluginRendererPreference() ?? "auto");
  const inspect = () => void manager.inspect({
    sourceUrl: source.sourceUrl.trim(), sourceRef: source.sourceRef.trim() || null,
    credentialSecretId: source.credentialSecretId || null,
    credentialUsername: source.credentialUsername.trim() || null,
  });
  const chooseRenderer = (value: string) => {
    setRenderer(value); savePluginRendererPreference(value === "auto" ? null : value);
  };
  return <Card className="p-5">
    <div className="flex flex-wrap items-start gap-3"><div><Badge tone="info">Git · sandbox UI extension</Badge><h2 className="mt-3 flex items-center gap-2 font-display text-xl font-bold"><Puzzle className="size-5" />插件与消息渲染</h2><p className="mt-1 text-xs text-secondary">仓库必须提交 manifest.json 与已打包的 JS；安装过程不会执行构建脚本。</p></div><Button className="ml-auto" variant="secondary" size="compact" onClick={() => void manager.reload()} disabled={manager.loading}><RefreshCw className="size-4" />刷新</Button></div>
    <div className="mt-5 grid gap-3 rounded-2xl border border-default p-4 md:grid-cols-2">
      <Field label="HTTPS Git URL"><Input value={source.sourceUrl} onChange={(event) => setSource({ ...source, sourceUrl: event.target.value })} placeholder="https://github.com/user/plugin.git" /></Field>
      <Field label="Git ref（可选）"><Input value={source.sourceRef} onChange={(event) => setSource({ ...source, sourceRef: event.target.value })} placeholder="branch / tag；安装时固定实际 commit" /></Field>
      <Field label="凭据 Secret（可选）"><select className={selectClass} value={source.credentialSecretId} onChange={(event) => setSource({ ...source, credentialSecretId: event.target.value })}><option value="">公开仓库</option>{secrets.map((secret) => <option key={secret.secretRef.id} value={secret.secretRef.id}>{secret.name || secret.secretRef.id}</option>)}</select></Field>
      <Field label="Git 用户名（可选）"><Input value={source.credentialUsername} onChange={(event) => setSource({ ...source, credentialUsername: event.target.value })} placeholder="默认 git" /></Field>
      <div className="md:col-span-2"><Button onClick={inspect} disabled={!source.sourceUrl.trim() || manager.busy !== null}><GitBranch className="size-4" />{manager.busy === "inspect" ? "正在拉取并校验…" : "检查仓库"}</Button></div>
    </div>
    {manager.candidate && <div className="mt-4 rounded-2xl border border-accent/30 bg-accent-soft/30 p-4"><div className="flex flex-wrap items-start gap-3"><div><p className="font-semibold">{manager.candidate.manifest.name} · {manager.candidate.manifest.version}</p><p className="mt-1 font-mono text-[11px] text-muted">{manager.candidate.resolvedCommit}</p></div><Button className="ml-auto" variant="ghost" size="compact" onClick={manager.clearCandidate}>取消</Button></div><p className="mt-3 text-xs font-semibold">请求权限</p><div className="mt-2 flex flex-wrap gap-2">{manager.candidate.manifest.permissions.map((permission) => <Badge key={permission} tone={manager.candidate?.addedPermissions.includes(permission) ? "warning" : "neutral"}>{permission}</Badge>)}</div>{manager.candidate.addedPermissions.length > 0 && <p className="mt-2 text-xs text-warning">黄色权限为本次新增，自动更新不会替你确认。</p>}<div className="mt-4 flex flex-wrap items-end gap-3"><Field label="更新策略"><Policy value={candidatePolicy} onChange={setCandidatePolicy} /></Field><Button onClick={() => void manager.activate(candidatePolicy)} disabled={manager.busy !== null}>{manager.busy === "activate" ? "正在激活…" : "确认全部权限并激活"}</Button></div></div>}
    <div className="mt-5 grid gap-3 md:grid-cols-2">{manager.installations.map((item) => <Installation key={item.pluginId} item={item} busy={manager.busy} onConfigure={manager.configure} onCheck={manager.checkUpdate} onRollback={manager.rollback} />)}{!manager.loading && manager.installations.length === 0 && <p className="text-sm text-muted">尚未安装插件。</p>}</div>
    <label className="mt-5 grid gap-1.5 text-xs font-semibold text-secondary">消息正文 renderer<select className={selectClass} value={renderer} onChange={(event) => chooseRenderer(event.target.value)}><option value="auto">自动选择最高优先级</option><option value="native">庄生原生渲染</option>{host.available.map((item) => <option key={item.key} value={item.key}>{item.pluginName} · {item.rendererId}</option>)}</select><span className="font-normal text-muted">此选择只改变当前设备的前端呈现，不改写消息、Context 或 event log。</span></label>
    {host.error && <p className="mt-3 text-xs text-warning">扩展宿主：{host.error}</p>}{manager.error && <p className="mt-3 text-sm text-danger">{manager.error}</p>}{manager.notice && <p className="mt-3 text-sm text-success">{manager.notice}</p>}
  </Card>;
}

function Installation({ item, busy, onConfigure, onCheck, onRollback }: { item: PluginInstallationView; busy: string | null; onConfigure: (item: PluginInstallationView, enabled: boolean, policy: PluginUpdatePolicy) => unknown; onCheck: (id: string) => unknown; onRollback: (item: PluginInstallationView, target: string) => unknown }) {
  const [policy, setPolicy] = useState(item.updatePolicy); const [target, setTarget] = useState(item.previousVersions[0]?.id ?? "");
  return <div className="rounded-2xl border border-default p-4"><div className="flex items-start gap-2"><div><p className="font-semibold">{item.activeVersion.manifest.name}</p><p className="mt-1 text-xs text-muted">{item.activeVersion.version} · {item.pluginId}</p></div><Badge className="ml-auto" tone={item.enabled ? "success" : "neutral"}>{item.enabled ? "启用" : "停用"}</Badge></div><div className="mt-3 flex flex-wrap gap-2"><Policy value={policy} onChange={setPolicy} /><Button size="compact" variant="secondary" disabled={busy !== null || policy === item.updatePolicy} onClick={() => void onConfigure(item, item.enabled, policy)}>保存策略</Button><Button size="compact" variant="secondary" disabled={busy !== null} onClick={() => void onConfigure(item, !item.enabled, policy)}>{item.enabled ? "停用" : "启用"}</Button><Button size="compact" variant="ghost" disabled={busy !== null} onClick={() => void onCheck(item.pluginId)}>检查更新</Button></div>{item.previousVersions.length > 0 && <div className="mt-3 flex gap-2"><select aria-label="回滚版本" className={selectClass} value={target} onChange={(event) => setTarget(event.target.value)}>{item.previousVersions.map((version) => <option key={version.id} value={version.id}>{version.version} · {version.resolvedCommit.slice(0, 8)}</option>)}</select><Button size="compact" variant="secondary" disabled={!target || busy !== null} onClick={() => void onRollback(item, target)}>回滚</Button></div>}</div>;
}

const Policy = ({ value, onChange }: { value: PluginUpdatePolicy; onChange: (value: PluginUpdatePolicy) => void }) => <select aria-label="更新策略" className={selectClass} value={value} onChange={(event) => onChange(event.target.value as PluginUpdatePolicy)}><option value="manual">手动</option><option value="notify">通知</option><option value="automatic">自动（权限不增加时）</option></select>;
const Field = ({ label, children }: { label: string; children: React.ReactNode }) => <label className="grid gap-1.5 text-xs font-semibold text-secondary">{label}{children}</label>;
const selectClass = "min-h-10 rounded-xl border border-default bg-canvas px-3 text-sm text-primary";
