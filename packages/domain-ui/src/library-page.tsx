import { useMemo, useState } from "react";
import { BookUser, Boxes, FileArchive, RefreshCw, Search, Workflow } from "lucide-react";

import { stringifyJsonExact, type ArtifactView, type ChannelView, type ContextPresetVersionView, type ContextPresetView, type RolePlayGraphOptionView } from "@zhuangsheng/api-client";
import { Badge, Button, Card, Input } from "@zhuangsheng/ui";

import { ArtifactList } from "./artifact-list";
import { SillyTavernImportCard, type SillyTavernImportActions } from "./sillytavern-import-card";

type Tab = "presets" | "templates" | "assets";

export function LibraryPage(props: {
  presets: ContextPresetView[];
  channels: ChannelView[];
  versions: Record<string, ContextPresetVersionView>;
  templates: RolePlayGraphOptionView[];
  artifacts: ArtifactView[];
  loading: boolean;
  error: string | null;
  onReload: () => void;
  onOpenSettings: () => void;
  onOpenArtifacts: () => void;
  contentUrl: (artifactId: string) => string;
  onDownloadArtifact?: (artifactId: string) => Promise<void>;
  sillyTavern?: SillyTavernImportActions;
}) {
  const [tab, setTab] = useState<Tab>("presets");
  const [query, setQuery] = useState("");
  const [exportError, setExportError] = useState<string | null>(null);
  const needle = query.trim().toLocaleLowerCase();
  const presets = useMemo(() => props.presets.filter((item) => !needle || item.name.toLocaleLowerCase().includes(needle)), [props.presets, needle]);
  const templates = useMemo(() => props.templates.filter((item) => !needle || item.graphName.toLocaleLowerCase().includes(needle)), [props.templates, needle]);
  const artifacts = useMemo(() => props.artifacts.filter((item) => !needle || (item.metadata.name ?? item.metadata.artifactId).toLocaleLowerCase().includes(needle)), [props.artifacts, needle]);
  return <div className="mx-auto max-w-6xl pb-24">
    <header className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between"><div><Badge tone="info">Versioned resources</Badge><h1 className="mt-3 flex items-center gap-2 font-display text-3xl font-bold"><Boxes className="size-7" />资料库</h1><p className="mt-2 text-secondary">ContextPreset、Agent 模板和 Artifact 的安全投影；内容不复制到浏览器 source of truth。</p></div><Button variant="secondary" onClick={props.onReload}><RefreshCw className="size-4" />刷新</Button></header>
    {(props.error || exportError) && <Card role="alert" className="mt-5 border-danger/30 p-4 text-sm text-danger">{props.error || exportError}</Card>}
    <div className="mt-5 flex flex-col gap-3 rounded-2xl border border-default bg-surface p-3 sm:flex-row sm:items-center"><div className="flex gap-1" role="tablist" aria-label="资料类型">{(["presets", "templates", "assets"] as const).map((value) => <button key={value} role="tab" aria-selected={tab === value} className={`min-h-10 rounded-xl px-3 text-sm font-semibold ${tab === value ? "bg-elevated text-primary" : "text-muted"}`} onClick={() => setTab(value)}>{value === "presets" ? "角色与 Context" : value === "templates" ? "Agent 模板" : "Assets"}</button>)}</div><label className="relative ml-auto w-full sm:max-w-72"><Search className="pointer-events-none absolute left-3 top-3.5 size-4 text-muted" /><Input className="pl-9" aria-label="搜索资料库" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索名称" /></label></div>
    {props.loading ? <div className="mt-6 grid gap-3 sm:grid-cols-2">{[0, 1, 2, 3].map((item) => <div key={item} className="h-32 animate-pulse rounded-2xl bg-elevated" />)}</div> : <div className="mt-6">
      {tab === "presets" && props.sillyTavern && <SillyTavernImportCard presets={props.presets} channels={props.channels} actions={props.sillyTavern} onImported={props.onReload} />}
      {tab === "presets" && <ResourceGrid empty="还没有角色或 Context preset。">{presets.map((preset) => { const version = preset.headVersionId ? props.versions[preset.headVersionId] : undefined; return <ResourceCard key={preset.id} icon={<BookUser className="size-5" />} title={preset.name} id={preset.id} status={version ? `published v${version.versionNo}` : "draft only"} detail={version ? `semantic policy ${version.semanticPolicyVersion} · ${version.contentHash}` : "尚未发布，不能被新 Run 固定"} action={version && props.sillyTavern ? { label: "导出酒馆 JSON", run: () => downloadSillyTavern(props.sillyTavern!, version.id).catch((cause) => setExportError(cause instanceof Error ? cause.message : "导出失败。")) } : undefined} />; })}</ResourceGrid>}
      {tab === "templates" && <ResourceGrid empty="还没有 Agent 模板。">{templates.map((template) => <ResourceCard key={template.revisionId} icon={<Workflow className="size-5" />} title={template.graphName} id={template.revisionId} status={`revision ${template.revisionNo}`} detail={template.compatibility.mode === "editable" ? "用户模式可完整编辑" : template.compatibility.mode === "partial" ? `部分兼容 · ${template.compatibility.lockedReasons.join("、")}` : `专家专用 · ${template.compatibility.reasons.join("、")}`} />)}</ResourceGrid>}
      {tab === "assets" && <ArtifactList items={artifacts} contentUrl={props.contentUrl} onDownload={props.onDownloadArtifact} />}
    </div>}
    <div className="mt-6 flex flex-wrap gap-2"><Button variant="secondary" onClick={props.onOpenSettings}>{tab === "templates" ? "创建 Agent 模板" : "创建角色 / Context"}</Button><Button variant="ghost" onClick={props.onOpenArtifacts}><FileArchive className="size-4" />导入或管理 Artifact</Button></div>
  </div>;
}

function ResourceGrid({ children, empty }: { children: React.ReactNode; empty: string }) { return <div className="grid gap-3 sm:grid-cols-2">{Array.isArray(children) && children.length === 0 ? <Card className="p-8 text-center text-sm text-muted sm:col-span-2">{empty}</Card> : children}</div>; }
function ResourceCard({ icon, title, id, status, detail, action }: { icon: React.ReactNode; title: string; id: string; status: string; detail: string; action?: { label: string; run: () => void } }) { return <Card className="p-5"><div className="flex items-center gap-3"><div className="grid size-10 place-items-center rounded-xl bg-elevated text-info">{icon}</div><div className="min-w-0"><h2 className="truncate font-semibold">{title}</h2><p className="truncate font-mono text-[11px] text-muted">{id}</p></div></div><Badge className="mt-4" tone={status.includes("draft") ? "warning" : "success"}>{status}</Badge><p className="mt-2 break-words text-xs text-secondary">{detail}</p>{action && <Button className="mt-3" size="compact" variant="secondary" onClick={action.run}>{action.label}</Button>}</Card>; }

async function downloadSillyTavern(actions: SillyTavernImportActions, versionId: string) {
  const result = await actions.export({ presetVersionId: versionId });
  for (const document of result.bundle.documents) {
    const url = URL.createObjectURL(new Blob([stringifyJsonExact(document.document, 2)], { type: "application/json" }));
    const link = window.document.createElement("a");
    link.href = url; link.download = document.fileName; link.click(); URL.revokeObjectURL(url);
  }
}
