# Runtime Journal 与流式事件

## 定位

Runtime event 是执行事实、恢复游标和 UI 观察接口，不是 LLM token 的别名。

```text
state transition + durable event（同一事务）
  -> commit
  -> publish wake hint（不携带可转发的 durable payload）
  -> subscriber 按自己的数据库 cursor drain
  -> SSE / Tauri / WebSocket client
```

阶段一的 `RunEvent` 只属于一个 GraphRun。Run 外的 context commit、memory proposal 审批和 artifact lifecycle 使用各自的 audit log；如果 UI 需要统一时间线，由查询层合并，不能强迫所有领域事件伪造 `runId`。

下文列出的 branch/proposal/artifact event 只有在操作具有 `originRunId` 时才同时写 RunEvent；纯 run 外操作只写 version log + domain audit/outbox event。

## Durable 与 Live Envelope

可恢复事件：

```ts
type DurableRunEvent<T = JsonValue> = {
  id: string
  runId: string
  durableSeq: number
  type: string
  schemaVersion: number
  timestamp: string
  contextBranchId?: string
  nodeInstanceId?: string
  attemptId?: string
  correlationId?: string
  causationEventId?: string
  importance: "debug" | "info" | "critical"
  payload: T | { payloadRef: string }
}
```

只在当前连接可见的高频事件：

```ts
type EphemeralRunEvent<T = JsonValue> = {
  id: string
  runId: string
  type: string
  schemaVersion: number
  timestamp: string
  nodeInstanceId?: string
  attemptId?: string
  callId?: string
  liveOrdinal?: number
  payload: T
}
```

Ephemeral event 没有 `durableSeq`，不能作为 `Last-Event-ID`。`liveOrdinal` 只在一个 call/item 内检测重复或乱序，不承诺跨进程连续。

`correlationId` 关联同一 wait/tool/model/command；`causationEventId` 指向直接触发当前事实的 durable event。层级展示不能只依赖 timestamp。

## Durable Sequence

`durableSeq` 的规则：

1. 在数据库事务内按 run 分配，唯一且严格递增。
2. 允许空洞，不允许复用；排序不依赖 timestamp 或事件 ID。
3. 并发事务以成功提交的序列化顺序为准。
4. `afterDurableSeq` 只读取更大的 durable event。
5. 事件删除或物理压缩后仍不复用旧 sequence。

SQLite 可以在更新 `graph_runs.next_event_seq` 的同一写事务中分配；PostgreSQL 可以锁 run counter row。业务代码和 worker 内存都不能自行维护权威 counter。

## 权威关系

三种记录职责不同：

```text
durable runtime journal
执行状态转换的历史权威和订阅来源。

normalized runtime rows
run、instance、attempt、wait、queue、effect 的当前物化投影，用于调度。

RuntimeCheckpoint
一致切点的恢复优化，不替代 journal。
```

每个关键状态转换必须同时更新 normalized rows 并追加足以解释该转换的 durable event。二者不允许分事务提交。发现投影与 journal 不一致时停止该 run 并进入 recovery/error，而不是猜测或再次执行外部副作用。

重放有两个含义：

- delivery replay：按 cursor 重新发送已持久化事件；
- projection replay：从 checkpoint 后的事件重建/校验运行投影。

重放绝不重新调用 LLM、tool 或其他外部 effect。Effect 结果使用已持久化 result ref；未知结果进入协调状态。

## 初始事件集合

阶段一 critical journal 至少包括：

```text
run.created / run.started
run.interrupt.requested / run.interrupted / run.resumed
run.cancel.requested / run.cancelled
run.completed / run.failed

node.scheduled / node.started
node.waiting / node.resumed
node.retry.scheduled
node.completed / node.failed / node.cancelled
node.lease.expired

edge.value.enqueued / edge.value.consumed / edge.value.stranded
run.output.committed

state.patch.committed
memory.proposal.created / memory.proposal.status_changed
router.decision

wait.created / wait.satisfied / wait.expired
effect.prepared / effect.succeeded / effect.failed / effect.outcome_unknown
branch.forked / branch.merge.committed / branch.merge.conflicted
checkpoint.created
```

LLM/tool 语义事件：

```text
llm.call.started / llm.call.completed / llm.call.failed
tool.call.requested / tool.call.awaiting_approval
tool.call.started / tool.call.completed / tool.call.failed
artifact.committed
```

每个 attempt、model call 和 tool call 恰有一个 durable terminal event。数据库唯一约束和状态 CAS 负责阻止重复 terminal。

## Streaming Delta

默认 live-only：

```text
llm.text.delta
llm.reasoning.delta
llm.tool_arguments.delta
node.output.delta
llm.partial_object
```

Delta 只服务实时 UI/trace，不进入 graph edge、StatePatch 或 WorkingContext。工具参数必须等待完整 item 和 schema validation 后才能执行；partial object 不作为节点结果。

每个 stream finalizer 必须产生恰好一个 durable `llm.call.completed` 或 `llm.call.failed`。断线客户端可能看不到中间 delta，但可从 terminal event/ref 取得最终文本、tool transcript 和 usage。

可选 `compact` policy 可以把若干 delta 合成 chunk 后持久化；chunk 获得正常 `durableSeq`。阶段一默认不持久化 token chunk，只保存最终 message。不能为了给每个 token 分配 sequence 而每 token 写数据库。

## 原子发出与发布

关键事务示例：

```text
complete NodeInstance
-> CAS run epoch 与 attempt fencing token
-> commit StatePatch/context head
-> 写 run output 和 edge queue values
-> 写 node.completed 等 durable events并分配 sequence
-> commit transaction
-> notifier 唤醒 scheduler/subscriber
```

Notifier 是 best-effort 加速层，只能表示“某个 run 可能已有新 durable row”；hint 可以重复、合并、丢失或乱序，subscriber 不得直接转发其中的 event/payload/sequence。进程在 commit 后、publish 前崩溃时，scheduler 扫描和 subscriber 的定期补读仍能发现事件。禁止先 publish 再 persist。

每个 durable subscriber 独占一个数据库 cursor。建立连接时先注册 wake-hint channel，再反复执行 `WHERE run_id = ? AND durable_seq > cursor ORDER BY durable_seq LIMIT ?`，只按查询结果发出并单调推进 cursor；drain 为空后等待下一 hint 或有界 poll deadline，再从数据库继续 drain。Sequence 允许空洞，因此只要求严格升序，不等待 `cursor + 1`。这个单-reader loop 是该连接唯一的 durable 发送入口，多个 notifier callback 只能唤醒它，不能并行写连接或绕过 cursor。Ephemeral live channel 独立，可直接 coalesce/drop，但没有 durable cursor，也不能推进、阻塞或重排 durable drain。完整 adapter 规则见 `21-adapters-api.md`。

## 背压

每个订阅者使用有界队列：

- ephemeral delta 可以 coalesce 或 drop；
- durable event 不能静默 drop；队列将满时断开慢消费者；
- 消费者凭最后 durable cursor 重连；
- durable queue 只接收上述单一数据库 drain loop 的有序结果，不接收 notifier payload；
- subscriber 永远不能阻塞 node completion transaction；
- UI 对高频 delta 做批量渲染或虚拟化。

Runtime 内部 scheduler 不通过面向 UI 的 subscriber 驱动；它读取 durable work rows，event notification 只是提示。

## Payload、版本与安全

- `type + schemaVersion` 决定 payload decoder；旧版本在保留期内必须仍可读。
- 大 payload、raw provider response 和 artifact 使用 content ref；event 只保存有界 preview/ref。
- 未知 `JsonValue` 在持久化前执行大小、深度和敏感字段过滤。
- Secret、Authorization header、主密码和 tool credential 永不进入 event。
- Prompt、tool 参数、memory 内容和 provider error 按权限读取，默认只记录 hash/ref 或脱敏摘要。
- `timestamp` 使用 UTC，展示时再转换时区。

## Retention 与 Compaction

阶段一不删除 critical journal。Ephemeral event 从未落盘；debug/info event 可以配置保留期。

后续 compaction 必须：

1. 先创建覆盖到 `throughSeq` 的有效 RuntimeCheckpoint；
2. 保留 terminal、effect、state/context commit、wait、control、branch 和 audit 事件；
3. 只压缩允许丢弃的 delta/debug payload；
4. 保留 sequence 空洞和 payload hash，不能重写历史含义；
5. 把 checkpoint、branch、proposal、evidence 引用纳入 object GC roots。

Event compaction 与 context version snapshot 是两种操作，不能共用一个模糊的 `Checkpoint` 类型。

## 对外接口

Core 提供：

```ts
subscribeRun(runId, { afterDurableSeq?: number })
waitRun(runId)
getRunEvents(runId, { afterDurableSeq?: number, limit?: number })
```

SSE 适合只读流；HTTP command 或 WebSocket 负责 interrupt、resume、cancel 和 wait response；Tauri event channel 使用同一 durable cursor 语义。
