import { ArrowLeft, Bot, GitBranch, Image, Paperclip, RefreshCw, UserRound } from "lucide-react";

import type { ConversationTimelineView, ConversationView, LlmContentPart } from "@zhuangsheng/api-client";
import { Badge, Button, Card, cn } from "@zhuangsheng/ui";

interface StoryDetailProps {
  story: ConversationView | null;
  timeline: ConversationTimelineView | null;
  loading: boolean;
  error: string | null;
  onBack: () => void;
  onReload: () => void;
}

const statusLabel = {
  running: ["角色正在回应", "running"], ready: ["已完成", "success"], failed: ["运行失败", "danger"],
  cancelled: ["已取消", "neutral"], projection_conflicted: ["回复与故事分支冲突", "warning"],
  projection_failed: ["回复无法写入故事", "danger"], projection_abandoned: ["已放弃此回复", "neutral"],
} as const;

export function StoryDetail({ story, timeline, loading, error, onBack, onReload }: StoryDetailProps) {
  return <div className="mx-auto grid max-w-7xl gap-6 pb-24 lg:grid-cols-[minmax(0,1fr)_300px]">
    <section className="min-w-0"><div className="flex items-center gap-3"><Button variant="ghost" size="icon" onClick={onBack} aria-label="返回故事列表"><ArrowLeft className="size-5" /></Button><div className="min-w-0"><h1 className="truncate font-display text-2xl font-bold">{story?.title || "未命名故事"}</h1><p className="mt-0.5 text-xs text-muted">active ancestry · {timeline ? shortId(timeline.activeHeadCommitId) : "加载中"}</p></div><Button className="ml-auto" variant="secondary" size="compact" onClick={onReload}><RefreshCw className="size-3.5" />刷新</Button></div>
      {error && <Card className="mt-5 border-danger/30 p-4 text-sm text-danger">{error}</Card>}
      <div className="mx-auto mt-8 max-w-3xl space-y-5" aria-live="polite">{loading ? <div className="space-y-4">{[0,1].map((item) => <div key={item} className="h-28 animate-pulse rounded-2xl bg-elevated" />)}</div> : timeline?.messages.length === 0 ? <Card className="p-10 text-center"><Bot className="mx-auto size-7 text-muted" /><h2 className="mt-3 font-semibold">故事已经建立</h2><p className="mt-1 text-sm text-muted">还没有正式消息。先在故事设置中选择可兼容的角色模板与 Graph。</p></Card> : timeline?.messages.map((message) => <article key={message.id} className={cn("flex gap-3", message.role === "user" && "flex-row-reverse")}><div className={cn("grid size-9 shrink-0 place-items-center rounded-xl", message.role === "assistant" ? "bg-accent-soft text-accent" : "bg-elevated text-secondary")}>{message.role === "assistant" ? <Bot className="size-4" /> : <UserRound className="size-4" />}</div><div className={cn("max-w-[min(82%,42rem)]", message.role === "user" && "text-right")}><div className={cn("rounded-2xl border border-default bg-surface px-4 py-3 text-left shadow-soft", message.role === "user" && "border-accent/20 bg-accent-soft/60")}>{message.content.map((part, index) => <ContentPart key={index} part={part} />)}</div><div className="mt-1.5 flex items-center gap-2 px-1 text-[11px] text-muted"><span>{message.role === "assistant" ? "角色回复" : "你"}</span>{message.source === "saved_partial" && <Badge tone="warning">保存的未完成回复</Badge>}{message.originRunId && <span>Run {shortId(message.originRunId)}</span>}</div></div></article>)}</div>
    </section>
    <aside className="space-y-4 lg:sticky lg:top-24 lg:self-start"><Card className="p-5"><div className="flex items-center gap-2 font-semibold"><GitBranch className="size-4 text-accent" />当前故事分支</div><dl className="mt-4 space-y-3 text-xs"><div><dt className="text-muted">Branch</dt><dd className="mt-1 break-all font-mono text-secondary">{timeline?.activeBranchId || "—"}</dd></div><div><dt className="text-muted">Head</dt><dd className="mt-1 break-all font-mono text-secondary">{timeline?.activeHeadCommitId || "—"}</dd></div></dl></Card><Card className="p-5"><h2 className="font-semibold">回复候选</h2><div className="mt-3 space-y-2">{timeline?.turns.flatMap((turn) => turn.candidates).map((candidate) => { const [label, tone] = statusLabel[candidate.status]; return <div key={candidate.runId} className="rounded-xl border border-default p-3"><div className="flex items-center justify-between gap-2"><span className="font-mono text-xs text-secondary">{shortId(candidate.runId)}</span><Badge tone={tone}>{label}</Badge></div>{candidate.projectionError && <p className="mt-2 text-xs text-warning">{candidate.projectionError.safeMessage}</p>}</div>; })}{timeline && timeline.turns.length === 0 && <p className="text-sm text-muted">暂无候选</p>}</div></Card></aside>
  </div>;
}

function ContentPart({ part }: { part: LlmContentPart }) {
  if (part.type === "text") return <p className="whitespace-pre-wrap leading-7">{part.text}</p>;
  const Icon = part.type === "image" ? Image : Paperclip;
  return <div className="flex items-center gap-2 rounded-xl bg-elevated p-3 text-sm text-secondary"><Icon className="size-4" /><span>{part.type === "image" ? "图片" : "文件"}</span><span className="truncate font-mono text-xs">{part.artifactRef.artifactId}</span></div>;
}

const shortId = (value: string) => value.length > 18 ? `${value.slice(0, 10)}…${value.slice(-5)}` : value;
