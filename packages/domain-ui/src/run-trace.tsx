import type { RunStreamProjection } from "@zhuangsheng/api-client";
import { Badge, Card } from "@zhuangsheng/ui";

export function RunTrace({ projection }: { projection: RunStreamProjection }) {
  const events = projection.recentEvents;
  return (
    <Card className="overflow-hidden">
      <div className="flex items-center justify-between border-b border-default px-5 py-4"><h2 className="font-semibold">Durable timeline</h2><span className="text-xs text-muted">最近 {events.length} / 500 条</span></div>
      {events.length === 0 ? (
        <p className="p-6 text-sm text-muted">正在读取保留期内的 durable events…</p>
      ) : (
        <ol className="max-h-[44rem] divide-y divide-default overflow-auto">
          {events.map((event) => (
            <li key={event.durableSeq} className="grid gap-2 px-5 py-3 text-xs sm:grid-cols-[5rem_minmax(0,1fr)_auto] sm:items-center">
              <span className="font-mono text-muted">#{event.durableSeq}</span>
              <div className="min-w-0"><p className="truncate font-mono text-sm text-primary">{event.type}</p><p className="mt-1 truncate text-muted">{event.nodeInstanceId ? `node ${event.nodeInstanceId}` : "run scope"}{event.attemptId ? ` · attempt ${event.attemptId}` : ""}</p></div>
              <div className="flex items-center gap-2"><Badge tone={event.importance === "critical" ? "warning" : "neutral"}>{event.importance}</Badge><time className="text-muted">{new Date(event.timestamp).toLocaleTimeString()}</time></div>
            </li>
          ))}
        </ol>
      )}
    </Card>
  );
}
