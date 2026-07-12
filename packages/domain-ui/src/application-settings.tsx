import { useEffect, useState } from "react";
import { MonitorCog, Save } from "lucide-react";

import { Badge, Button, Card } from "@zhuangsheng/ui";

export interface ApplicationPreferenceView {
  theme: "system" | "light" | "dark" | "contrast"; fontScale: "small" | "normal" | "large";
  density: "comfortable" | "compact"; reducedMotion: boolean; defaultMode: "user" | "expert";
  startPage: "stories" | "library"; streamPresentation: boolean; autoScroll: boolean;
  candidateGestures: boolean; notifications: boolean; confirmSensitiveDownloads: boolean;
  localBackups: boolean; diagnostics: boolean;
}

export function ApplicationSettings(props: { value: ApplicationPreferenceView; onSave: (value: ApplicationPreferenceView) => void }) {
  const [value, setValue] = useState(props.value);
  const [saved, setSaved] = useState(true);
  useEffect(() => { setValue(props.value); setSaved(true); }, [props.value]);
  const set = <K extends keyof ApplicationPreferenceView>(key: K, next: ApplicationPreferenceView[K]) => { setValue((current) => ({ ...current, [key]: next })); setSaved(false); };
  const save = () => { props.onSave(value); setSaved(true); };
  return <Card className="p-5">
    <div className="flex flex-wrap items-start gap-3"><div><Badge tone="info">作用域：当前设备 / 浏览器</Badge><h2 className="mt-3 flex items-center gap-2 font-display text-xl font-bold"><MonitorCog className="size-5" />Appearance 与 Behavior</h2><p className="mt-1 text-xs text-secondary">只改变界面偏好，不修改既有 GraphRun、Context 或历史 Candidate。</p></div><Button className="ml-auto" onClick={save} disabled={saved}><Save className="size-4" />{saved ? "已保存" : "保存应用设置"}</Button></div>
    <div className="mt-5 grid gap-5 md:grid-cols-2">
      <Group title="Appearance"><Select label="主题" value={value.theme} options={["system", "light", "dark", "contrast"]} onChange={(next) => set("theme", next as ApplicationPreferenceView["theme"])} /><Select label="字号" value={value.fontScale} options={["small", "normal", "large"]} onChange={(next) => set("fontScale", next as ApplicationPreferenceView["fontScale"])} /><Select label="密度" value={value.density} options={["comfortable", "compact"]} onChange={(next) => set("density", next as ApplicationPreferenceView["density"])} /><Check label="减少动画" checked={value.reducedMotion} onChange={(next) => set("reducedMotion", next)} /></Group>
      <Group title="Behavior"><Select label="默认模式" value={value.defaultMode} options={["user", "expert"]} onChange={(next) => set("defaultMode", next as ApplicationPreferenceView["defaultMode"])} /><Select label="启动页" value={value.startPage} options={["stories", "library"]} onChange={(next) => set("startPage", next as ApplicationPreferenceView["startPage"])} /><Check label="显示流式文本" checked={value.streamPresentation} onChange={(next) => set("streamPresentation", next)} /><Check label="新内容自动滚动" checked={value.autoScroll} onChange={(next) => set("autoScroll", next)} /><Check label="启用候选手势（按钮始终保留）" checked={value.candidateGestures} onChange={(next) => set("candidateGestures", next)} /><Check label="系统通知" checked={value.notifications} onChange={(next) => set("notifications", next)} /></Group>
      <Group title="Storage & Backup"><Check label="敏感下载前再次确认" checked={value.confirmSensitiveDownloads} onChange={(next) => set("confirmSensitiveDownloads", next)} /><Check label="启用本地备份提醒" checked={value.localBackups} onChange={(next) => set("localBackups", next)} /></Group>
      <Group title="Privacy & Diagnostics"><Check label="保存匿名本地诊断信息" checked={value.diagnostics} onChange={(next) => set("diagnostics", next)} /><p className="text-xs text-muted">SecretValue、prompt 正文和 provider raw response 不进入此偏好。</p></Group>
    </div>
  </Card>;
}

function Group({ title, children }: { title: string; children: React.ReactNode }) { return <fieldset className="space-y-3 rounded-xl border border-default p-4"><legend className="px-1 text-sm font-bold">{title}</legend>{children}</fieldset>; }
function Select({ label, value, options, onChange }: { label: string; value: string; options: string[]; onChange: (value: string) => void }) { return <label className="grid gap-1.5 text-xs font-semibold text-secondary">{label}<select className="min-h-10 rounded-xl border border-default bg-canvas px-3 text-sm text-primary" value={value} onChange={(event) => onChange(event.target.value)}>{options.map((option) => <option key={option} value={option}>{option}</option>)}</select></label>; }
function Check({ label, checked, onChange }: { label: string; checked: boolean; onChange: (value: boolean) => void }) { return <label className="flex min-h-10 items-center gap-3 text-sm text-secondary"><input className="size-4" type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />{label}</label>; }
