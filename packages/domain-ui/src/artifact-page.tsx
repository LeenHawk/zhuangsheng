import { useState, type FormEvent } from "react";
import { RefreshCw, Upload } from "lucide-react";

import type { ArtifactClassification, ArtifactStagingView, ArtifactView, UploadArtifactInput } from "@zhuangsheng/api-client";
import { Badge, Button, Input } from "@zhuangsheng/ui";

import { ArtifactList } from "./artifact-list";

interface Props {
  items: ArtifactView[];
  loading: boolean;
  pending: boolean;
  pendingCommit: ArtifactStagingView | null;
  error: string | null;
  onReload: () => void;
  onUpload: (input: UploadArtifactInput) => Promise<void>;
  onRetryCommit: () => void;
  contentUrl: (artifactId: string) => string;
}

export function ArtifactPage(props: Props) {
  const [file, setFile] = useState<File | null>(null);
  const [classification, setClassification] = useState<ArtifactClassification>("private");
  const [retention, setRetention] = useState<"pinned" | "ephemeral">("pinned");
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!file) return;
    await props.onUpload({
      object: file,
      name: file.name,
      declaredMediaType: file.type || null,
      classification,
      retention: retention === "pinned"
        ? { type: "pinned" }
        : { type: "ephemeral", expiresAt: Date.now() + 24 * 60 * 60 * 1_000 },
    });
    setFile(null);
  };
  return <div className="mx-auto max-w-6xl space-y-5 pb-24">
    <header className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
      <div><Badge tone="info">Immutable object workspace</Badge><h1 className="mt-3 font-display text-3xl font-bold">Artifacts</h1><p className="mt-2 text-secondary">上传先进入 staging；只有校验并 commit 后才产生可引用 ArtifactRef。</p></div>
      <Button variant="secondary" onClick={props.onReload}><RefreshCw className="size-4" />刷新</Button>
    </header>
    {props.error && <div role="alert" className="rounded-xl border border-danger/25 bg-danger/5 p-3 text-sm text-danger">{props.error}</div>}
    {props.pendingCommit && <div className="flex flex-col gap-3 rounded-xl border border-warning/30 bg-warning/5 p-4 text-sm sm:flex-row sm:items-center sm:justify-between"><span>Artifact bytes 已验证，但 commit 尚未确认。重试会复用 staging generation 与原 idempotency key。</span><Button variant="secondary" onClick={props.onRetryCommit} disabled={props.pending}>重试 commit</Button></div>}
    <form className="grid gap-4 rounded-2xl border border-default bg-surface p-5 sm:grid-cols-[minmax(0,1fr)_10rem_10rem_auto] sm:items-end" onSubmit={(event) => void submit(event)}>
      <label className="grid gap-2 text-sm font-medium">选择文件<Input aria-label="Artifact 文件" type="file" onChange={(event) => setFile(event.target.files?.[0] ?? null)} /></label>
      <label className="grid gap-2 text-sm font-medium">Classification<select aria-label="Artifact classification" className="min-h-10 rounded-xl border border-default bg-elevated px-3" value={classification} onChange={(event) => setClassification(event.target.value as ArtifactClassification)}><option value="private">私有</option><option value="public">公开</option><option value="sensitive">敏感</option></select></label>
      <label className="grid gap-2 text-sm font-medium">Retention<select aria-label="Artifact retention" className="min-h-10 rounded-xl border border-default bg-elevated px-3" value={retention} onChange={(event) => setRetention(event.target.value as typeof retention)}><option value="pinned">固定保留</option><option value="ephemeral">临时 24 小时</option></select></label>
      <Button type="submit" disabled={!file || props.pending || props.pendingCommit !== null}><Upload className="size-4" />上传并 commit</Button>
    </form>
    <section><div className="mb-3 flex items-center justify-between"><h2 className="font-display text-xl font-bold">Committed artifacts</h2><span className="text-xs text-muted">{props.items.length} 项</span></div>{props.loading ? <p className="text-sm text-muted">正在读取 metadata projection…</p> : <ArtifactList items={props.items} contentUrl={props.contentUrl} />}</section>
  </div>;
}
