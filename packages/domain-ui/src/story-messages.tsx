import { Bot, Image, Paperclip, UserRound } from "lucide-react";

import type { ConversationTimelineView, LlmContentPart } from "@zhuangsheng/api-client";
import { Badge, Card, cn } from "@zhuangsheng/ui";

import { shortId } from "./story-format";
import type { StoryLiveCandidate } from "./story-detail";

export function StoryMessages({
  timeline,
  loading,
  liveCandidates,
}: {
  timeline: ConversationTimelineView | null;
  loading: boolean;
  liveCandidates: StoryLiveCandidate[];
}) {
  return (
    <div className="mx-auto mt-8 max-w-3xl space-y-5" aria-live="polite">
      {loading ? (
        <div className="space-y-4">
          {[0, 1].map((item) => <div key={item} className="h-28 animate-pulse rounded-2xl bg-elevated" />)}
        </div>
      ) : timeline?.messages.length === 0 ? (
        <Card className="p-10 text-center">
          <Bot className="mx-auto size-7 text-muted" />
          <h2 className="mt-3 font-semibold">故事已经建立</h2>
          <p className="mt-1 text-sm text-muted">先在故事设置中选择角色运行模板，然后写下第一句话。</p>
        </Card>
      ) : (
        timeline?.messages.map((message) => (
          <article key={message.id} className={cn("flex gap-3", message.role === "user" && "flex-row-reverse")}>
            <div className={cn("grid size-9 shrink-0 place-items-center rounded-xl", message.role === "assistant" ? "bg-accent-soft text-accent" : "bg-elevated text-secondary")}>
              {message.role === "assistant" ? <Bot className="size-4" /> : <UserRound className="size-4" />}
            </div>
            <div className={cn("max-w-[min(82%,42rem)]", message.role === "user" && "text-right")}>
              <div className={cn("rounded-2xl border border-default bg-surface px-4 py-3 text-left shadow-soft", message.role === "user" && "border-accent/20 bg-accent-soft/60")}>
                {(message.displayContent ?? message.content).map((part, index) => <ContentPart key={index} part={part} />)}
              </div>
              <div className="mt-1.5 flex items-center gap-2 px-1 text-[11px] text-muted">
                <span>{message.role === "assistant" ? "角色回复" : "你"}</span>
                {message.source === "saved_partial" && <Badge tone="warning">保存的未完成回复</Badge>}
                {message.originRunId && <span>Run {shortId(message.originRunId)}</span>}
              </div>
            </div>
          </article>
        ))
      )}
      <div className="space-y-3" aria-live="off">
        {liveCandidates.map((live) => (
          <Card key={live.runId} className="border-running/30 bg-accent-soft/30 p-4">
            <div className="flex items-center justify-between gap-3">
              <Badge tone="running">未提交实时预览</Badge>
              <span className="text-xs text-muted">{connectionText(live.connection)}</span>
            </div>
            {live.text ? (
              <p className="mt-3 whitespace-pre-wrap leading-7 text-secondary">{live.text}</p>
            ) : (
              <p className="mt-3 text-sm text-muted">角色正在组织回复…</p>
            )}
            {live.truncated && <p className="mt-2 text-xs text-warning">实时预览已达到本地上限，最终回复不受影响。</p>}
            {live.error && <p className="mt-2 text-xs text-warning">{live.error}</p>}
            <p className="mt-2 font-mono text-[11px] text-muted">Run {shortId(live.runId)}</p>
          </Card>
        ))}
      </div>
    </div>
  );
}

const connectionText = (state: StoryLiveCandidate["connection"]) => ({
  idle: "未连接",
  connecting: "正在连接",
  live: "实时连接",
  reconnecting: "正在恢复连接",
  incompatible: "事件版本不兼容",
  closed: "等待正式结果",
}[state]);

function ContentPart({ part }: { part: LlmContentPart }) {
  if (part.type === "text") return <p className="whitespace-pre-wrap leading-7">{part.text}</p>;
  const Icon = part.type === "image" ? Image : Paperclip;
  return (
    <div className="flex items-center gap-2 rounded-xl bg-elevated p-3 text-sm text-secondary">
      <Icon className="size-4" />
      <span>{part.type === "image" ? "图片" : "文件"}</span>
      <span className="truncate font-mono text-xs">{part.artifactRef.artifactId}</span>
    </div>
  );
}
