# 流式事件

## 基本定位

流式事件应该是 runtime 的一等输出接口，而不是 LLMNode 的附属能力。

整个框架对外暴露的是 run event stream：

```text
GraphRun Event Stream
  包含 run、node、LLM、tool、memory、state、branch 和 output events
```

Token streaming 只是其中一种事件。

## 事件分层

可以分为四层：

```text
Run-level events
整个 run 的生命周期。

Node-level events
节点调度、开始、完成、失败和等待。

Step-level events
LLMNode 内部的 model call、tool call、memory call。

Data-level events
token、partial JSON、artifact、memory patch。
```

## Event Envelope

所有事件使用统一 envelope。

```ts
type StreamEvent<T = unknown> = {
  id: string
  runId: string
  branchId: string
  nodeInstanceId?: string
  parentEventId?: string
  type: string
  seq: number
  timestamp: string
  payload: T
}
```

关键字段：

```text
id
全局事件 ID，用于去重和重连。

seq
run 内单调递增序号，用于排序。

branchId
事件所属分支。

nodeInstanceId
事件所属节点执行实例。

parentEventId
表达层级关系，例如 tool call 属于某次 LLM call。
```

## Seq 分配规则

`seq` 是 replay 和断线重连的地基，分配规则必须定死：

```text
1. seq 由持久化层分配，不由业务代码或并发任务自行计数。
   SQLite 用单表 autoincrement 或 per-run counter 事务内递增。
2. 单个 run 内 seq 严格单调递增，无空洞要求，但不允许重复。
3. 不要求跨 run 全局有序。
4. 并发节点的事件由写入顺序决定 seq，先落盘者先编号。
5. replay 和 afterSeq 重连都只依赖 run 内 seq，不依赖 timestamp。
```

timestamp 只用于展示和诊断，不参与排序。

## 事件持久化

事件应该通过统一路径发出：

```text
emit(event)
  -> persist if durable
  -> publish to subscribers
```

这样可以支持：

- UI 实时流式展示
- 从 `lastEventId` 或 `afterSeq` 断线重连
- debug
- replay
- audit
- run 完成后的 trace 查看

示例 API：

```ts
subscribeRun(runId, { afterSeq?: number })
```

HTTP 形式：

```text
GET /runs/:runId/events?after=123
```

## 初始事件类型

第一版可以支持：

```text
run.started
run.completed
run.failed
run.interrupt.requested
run.interrupted
run.resumed

node.scheduled
node.started
node.output.delta
node.completed
node.failed
node.waiting
node.resumed
node.cancelled

edge.value.enqueued
edge.value.consumed

llm.started
llm.token
llm.completed
llm.failed

tool.started
tool.completed
tool.failed

memory.read
memory.patch.proposed
memory.patch.applied
memory.patch.rejected

router.decision
branch.created
branch.merged
branch.abandoned
```

## Token Events

LLM token event 应该归属到具体 LLM call 和 node instance。

```ts
type LLMTokenPayload = {
  callId: string
  text: string
  index: number
}
```

典型顺序：

```text
llm.started(callId)
llm.token(callId, "...")
llm.token(callId, "...")
llm.completed(callId, finalMessage)
```

Token event 不应该直接修改 working memory。它是观察事件。

持久化上下文只应该通过语义化 memory patch 改变，例如节点完成后的 `memory.patch.applied`。

## Partial Object

如果 LLM 输出结构化 JSON，流式过程中不应该假设每个 token 都可解析。

可以提供 best-effort 的 partial object 事件：

```text
llm.token
原始 token。

llm.partial_object
runtime 尽力解析出的局部结构。
```

示例：

```ts
{
  type: "llm.partial_object",
  payload: {
    callId: "call_1",
    path: "/steps/0/title",
    value: "Search docs"
  }
}
```

`partial_object` 只用于 UI 和观察，不参与最终状态提交。最终以 `node.completed` 的结构化输出为准。

## Event Durability

不同事件需要不同持久化级别。

```ts
type EventDurability = "ephemeral" | "compact" | "persistent"
type EventImportance = "debug" | "info" | "critical"
```

推荐默认值：

```text
llm.token
ephemeral 或 compact

llm.completed
persistent

node.completed
persistent + critical

edge.value.enqueued / edge.value.consumed
persistent + critical

memory.patch.applied
persistent + critical
```

默认 token 处理建议为 `compact`：实时推送 token，但持久化最终 message，而不是保存每个 token。

## 背压

流式事件可能非常密集，需要背压策略。

可用策略：

```text
coalesce
把多个 token 合并成较大 chunk。

sample
对高频 debug event 采样。

drop_noncritical
消费者太慢时丢弃 token/debug event，但不能丢 critical event。

persist_critical_only
只持久化语义事件和最终 compacted output。
```

runtime 绝不能丢弃 critical event，例如 node completion、memory patch、run completion 和 failure。

## Streaming API

runtime 可以同时暴露事件流和最终结果等待。

```ts
const run = await graph.start(input)

for await (const event of graph.events(run.id)) {
  // render or inspect events
}

const result = await graph.wait(run.id)
```

便捷 API：

```ts
for await (const event of graph.stream(input)) {
  if (event.type === "node.output.delta") {
    render(event.payload.text)
  }
}
```

传输方式：

```text
SSE
适合浏览器只读事件流。

WebSocket
适合双向控制，比如 interrupt、resume 和 user input。

Polling event log
适合恢复、后台任务和 server-to-server 消费。
```
