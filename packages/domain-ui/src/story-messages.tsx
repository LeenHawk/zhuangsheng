import { Bot, Image, Paperclip, UserRound } from "lucide-react";

import type { ConversationTimelineView, LlmContentPart } from "@zhuangsheng/api-client";
import { Badge, Card, cn } from "@zhuangsheng/ui";

import { shortId } from "./story-format";

export function StoryMessages({
  timeline,
  loading,
}: {
  timeline: ConversationTimelineView | null;
  loading: boolean;
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
                {message.content.map((part, index) => <ContentPart key={index} part={part} />)}
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
    </div>
  );
}

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
