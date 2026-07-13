import { Download, FileArchive } from "lucide-react";

import type { ArtifactView } from "@zhuangsheng/api-client";
import { Badge, Button } from "@zhuangsheng/ui";

interface Props {
  items: ArtifactView[];
  contentUrl: (artifactId: string) => string;
  onDownload?: (artifactId: string) => Promise<void>;
}

export function ArtifactList({ items, contentUrl, onDownload }: Props) {
  if (items.length === 0) {
    return <div className="rounded-2xl border border-dashed border-default p-8 text-center text-sm text-muted">还没有 committed artifact。</div>;
  }
  return <div className="grid gap-3">{items.map(({ metadata, metadataHeadCommitId }) => (
    <article key={metadata.artifactId} className="rounded-2xl border border-default bg-surface p-4 shadow-sm">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex min-w-0 items-start gap-3">
          <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-elevated"><FileArchive className="size-5 text-muted" /></div>
          <div className="min-w-0">
            <h3 className="truncate font-semibold">{metadata.name ?? metadata.artifactId}</h3>
            <p className="mt-1 text-xs text-muted">{metadata.content.mediaType} · {formatBytes(metadata.content.byteSize)}</p>
            <p className="mt-1 truncate font-mono text-[11px] text-muted" title={metadata.content.contentHash}>{metadata.content.contentHash}</p>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Badge tone={metadata.classification === "sensitive" ? "danger" : metadata.classification === "public" ? "success" : "info"}>{classification(metadata.classification)}</Badge>
          <Badge>{retention(metadata.retention)}</Badge>
          {onDownload ? <Button variant="secondary" onClick={() => {
            const confirmSensitive = document.documentElement.dataset.confirmSensitiveDownloads !== "false";
            if (confirmSensitive && metadata.classification === "sensitive" && !window.confirm("这是敏感 Artifact。确认下载到本地设备？")) return;
            void onDownload(metadata.artifactId);
          }}><Download className="size-4" />下载</Button> : <Button asChild variant="secondary">
            <a href={contentUrl(metadata.artifactId)} download={metadata.name ?? "artifact"} onClick={(event) => {
              const confirmSensitive = document.documentElement.dataset.confirmSensitiveDownloads !== "false";
              if (confirmSensitive && metadata.classification === "sensitive" && !window.confirm("这是敏感 Artifact。确认下载到本地设备？")) event.preventDefault();
            }}><Download className="size-4" />下载</a>
          </Button>}
        </div>
      </div>
      <p className="mt-3 truncate font-mono text-[11px] text-muted">metadata head {metadataHeadCommitId}</p>
    </article>
  ))}</div>;
}

const formatBytes = (bytes: number) => bytes < 1024
  ? `${bytes} B`
  : bytes < 1024 * 1024
    ? `${(bytes / 1024).toFixed(1)} KiB`
    : `${(bytes / 1024 / 1024).toFixed(1)} MiB`;

const classification = (value: ArtifactView["metadata"]["classification"]) =>
  value === "public" ? "公开" : value === "sensitive" ? "敏感" : "私有";

const retention = (value: ArtifactView["metadata"]["retention"]) => {
  if (value.type === "pinned") return "固定保留";
  if (value.type === "ephemeral") return "临时";
  if (value.type === "audit_until") return "审计保留";
  return value.type === "run" ? "Run 生命周期" : "Context 生命周期";
};
