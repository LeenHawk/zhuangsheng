# 异步图 Runtime

## 定位

Runtime 是异步、事件驱动、可持久恢复的 firing 系统，不是同步 DAG 遍历器。它执行固定的 `GraphRevision`，维护 `ExecutionState`，并通过明确的 commit 协议读写 `WorkingContext`。

领域边界遵循 `16-domain-consistency.md`：

- GraphRun 是调度与因果边界；
- branch 属于 Context，不属于 run；
- State/Memory 版本使用 `commitId`，不用裸整数 version；
- finalized node transition、context commit、edge queue 和 durable journal 必须原子可见。

控制、wait、retry、lease、effect 和 cancel 的完整状态机见 `17-runtime-control.md`。Merge、JoinByKey、Aggregator、Expand 的消费规则见 `18-coordination-nodes.md`。

## 总体组件

```text
GraphRun Store
  -> Durable Wakeup Queue
  -> Scheduler
  -> Node Executor / Built-in Node
  -> Edge Queue + Run Output Store
  -> Context Commit Store
  -> Durable Runtime Journal / Outbox
```

所有 durable 组件可以在阶段一共享 SQLite transaction。内存 channel 只用于降低唤醒延迟，不是事实来源。

上层 API 可以保留：

```ts
const run = await graph.start(input, contextBinding)
const result = await graph.wait(run.id)
```

`invoke` 只是 `start + wait`，不会采用另一套执行语义。

## GraphRun

```ts
type GraphRun = {
  id: RunId
  graphRevisionId: GraphRevisionId
  graphContentHash: ContentHash
  runInputRef: ValueRef
  executionManifestRef: ValueRef

  contextId: string
  branchId: string
  inputCommitId: string
  outputCommitId?: string

  status: RunLifecycleStatus
  controlEpoch: number
  drainEpoch?: number
  limits: RunLimits
  startedAt?: string
  deadlineAt: string
  terminalErrorRef?: ValueRef
  finishedAt?: string
  createdAt: string
  updatedAt: string
}
```

`runInputRef` 指向创建前已持久化的不可变输入。普通 workflow 也创建临时 Context，因此 core 不需要把 context 字段改成 Conversation/Turn 字段。

`branchId` 只标识 `16-domain-consistency.md` 的 ContextBranch。一个 GraphRun 固定绑定一个 context branch 和 input commit，内部只有一个 execution namespace。Router fan-out 产生同一 run 内的并行数据流，不创建 execution branch 或 context branch。

`executionManifestRef` 固定 graph-level executor compatibility、policy、配置解析规则和可选的显式 revision pins。默认策略是 NodeInstance 首次执行时解析 preset/channel/registry 的当前 revision，并写入该 instance 的不可变 execution snapshot；LLM instance 还必须写入 `07-llm-channels-counting.md` 的 exact `LlmOperationExecutionPin`。因此尚未激活的节点可以看到后来发布的 preset 内容。已有 NodeInstance 的 retry/resume/recovery 只能使用自身 snapshot，不能读取“最新版”替代；未知 taxonomy/decoder version 必须 fail closed，不能猜当前 adapter。

## ValueRef 与大对象

Runtime table、event、queue 和 node record 保存不可变引用：

```ts
type ValueRef = {
  id: ValueId
  contentHash: ContentHash
  encoding: "canonical_json_v1" | "bytes"
  sizeBytes: number
}
```

小 JSON 可以由 storage 内联实现，大对象存入 content-addressed object store，但对 runtime 都表现为相同的 `ValueRef`。同一个 hash 的 value 可去重；Secret 不能进入 ValueRef、event、StatePatch 或 object store。

Executor 调用边界可以解析为 `JsonValue`，持久化记录仍保存引用。Selector 生成的新值也先写成不可变对象，再由 activation transaction 引用。

## NodeInstance 是 Activation

一次 firing 创建一个 NodeInstance。它表示“输入被消费后产生的一次语义 activation”，不表示某一次进程调用：

```ts
type NodeInstance = {
  id: NodeInstanceId
  runId: RunId
  nodeId: NodeId
  activationSeq: number
  graphRevisionId: GraphRevisionId
  executionSnapshotRef?: ValueRef
  status:
    | "ready"
    | "running"
    | "waiting"
    | "completed"
    | "failed"
    | "cancelled"
  inputs: Record<PortName, ValueRef>
  inputQueueItemRefs: Record<PortName, EdgeQueueItemRef[]>
  finalOutputs?: Record<PortName, ValueRef[]>
  finalReadSet?: ReadSetEntry[]
  outputCommitIds: string[]
  createdAt: string
  updatedAt: string
}
```

普通节点每个 input port 消费一个 queue item，因此 ref 数组长度为一；协调节点可以按自身规则消费零个、一个或多个 item。`activationSeq` 在 `(runId, nodeId)` 下原子递增。

阶段一同一 `(runId, nodeId)` 最多一个非终态 NodeInstance。它完成后只要输入再次满足条件，就可创建下一个 activation。不同 node 可以并发。

## NodeAttempt 是一次执行调用

Retry、wait resume 或 lease recovery 不重新消费 edge queue，而是在同一 NodeInstance 下创建新的 NodeAttempt。Executor-backed node 和需要独立求值/finalize 的 built-in node 都使用 attempt；纯事务型 built-in transition可以在同一事务创建并终结一个 attempt，但不能绕过 attempt identity、control epoch和journal：

```ts
type NodeAttempt = {
  id: NodeAttemptId
  nodeInstanceId: NodeInstanceId
  attemptNo: number
  invocationKind: "start" | "retry" | "resume" | "reconcile"
  status: AttemptStatus
  runControlEpoch: number
  leaseFence: number
  executorRef: ValueRef
  readSet: ReadSetEntry[]
  continuationRef?: ValueRef
  startedAt?: string
  deadlineAt?: string
  finishedAt?: string
}
```

每次 attempt 在真正调用 executor 前解析 manifest 和全部 deterministic bindings，并记录完整 ReadSet。阶段一普通 executor 的 retry/resume 始终复用 NodeInstance 已 pin 的 binding envelope/read set，不在同一 activation 中刷新外部读。唯一例外是内建 Router `validate_on_commit` 冲突：它可按 `14-router-node.md` 在有界 reconcile attempt 中重解析 Router memory reads，不重消费 input/不增 visit。其他需要新 snapshot 的情况必须创建新 activation/run，不把它伪装为 retry。

同一个 NodeInstance 的 attempts 串行。只有持有当前 lease fence 且符合 run control epoch 的 attempt 能 finalize；细节见 `17-runtime-control.md`。

## NodeResult

普通 executor 返回统一结构：

```ts
type NodeResult =
  | {
      status: "completed"
      outputs: Record<PortName, JsonValue>
      transition?: NodeTransition
    }
  | {
      status: "waiting"
      wait: WaitRequest
      continuation: JsonValue
      transition?: NodeTransition
    }
  | {
      status: "failed"
      error: NodeError
    }
```

```ts
type NodeTransition = {
  statePatches?: StatePatch[]
  memoryProposalRefs?: string[]
  artifactRefs?: ArtifactRef[]
}
```

Transition 只是 executor 的待提交计划；runtime 重新校验 grant、read set、base head 和引用。多个 StatePatch 按确定性声明/call order 组合，目标或 path 冲突时整个 finalize 失败，不能部分提交。

字段统一为 `outputs`。普通节点和 Router 每次 activation 对每个 output port 最多一个 finalized emission。`ExpandNode` 是阶段一唯一允许单 activation 对同一 port 产生多个 finalized emissions 的内建节点，其有界批量协议见 `18-coordination-nodes.md`。

Streaming token、tool progress 和 observation 不是 NodeResult output，不进入 edge queue。只有 finalized value 才能传播。

错误是有版本、可序列化且安全的领域值：

```ts
type NodeError = {
  code: string
  category: "contract" | "permission" | "timeout" | "external"
          | "conflict" | "control" | "integrity" | "internal"
  phase: string
  safeMessage: string
  retryClass: "never" | "policy" | "reconcile"
  detailsRef?: ValueRef
  causedByEventId?: string
}

type RunError = NodeError & {
  nodeInstanceId?: NodeInstanceId
  attemptId?: NodeAttemptId
}
```

`safeMessage` 和 inline metadata 有长度/敏感字段限制；provider/SQL 原始正文、secret、prompt 和 tool arguments 只能保存为受控 ref或直接丢弃。`retryClass=policy` 仍需 error code 命中 RetryPolicy 且 effect 安全；`reconcile` 不能按普通失败盲目 retry。

## Edge Queue

```ts
type EdgeQueueItem = {
  id: EdgeQueueItemId
  runId: RunId
  edgeId: EdgeId
  enqueueSeq: number
  valueRef: ValueRef
  producerNodeInstanceId: NodeInstanceId
  producerEmissionIndex: number
  consumedByNodeInstanceId?: NodeInstanceId
  createdAt: string
}
```

每条 edge 是持久化 FIFO queue。`enqueueSeq` 由 storage 在 run 内单调分配且全局唯一，用于跨 edge 的稳定到达顺序。单 port 广播时按 emission index、再按 applied edge id 排序分配 seq；每条 edge 得到独立 queue item，但可以引用同一个 ValueRef。

Queue item 只能在创建 NodeInstance 的同一事务中标记 consumed。删除已消费 item 是后续 compaction，不影响 trace。

## Run 创建与 InputNode Activation

创建流程：

```text
1. 持久化 immutable run input、run execution manifest，以及各 InputNode selector得到的 source ValueRef。
2. 开事务，校验 GraphRevisionId/contentHash 与 Context branch/inputCommitId。
3. 创建 GraphRun、run-local counters 和每个 InputNode 的 source activation。
4. 重新校验各 InputNode selector/schema计划，并把预写的 selected ValueRef绑定到 source activation。
5. 追加 durable journal，并写 scheduler wakeup。
6. commit 后发布 wakeup。
```

如果 source selector/schema 失败，创建 failed NodeInstance 并按默认失败规则终止 run。恢复从 `runInputRef` 读取，不重新调用 adapter。

## `activate_if_ready`

Scheduler 只能通过一个 storage 原语创建普通或协调 activation：

```ts
activate_if_ready(runId, nodeId, expectedWakeupSeq)
  -> Activated(NodeInstanceId)
   | NotReady
   | SuppressedByControl
   | LimitExceeded
```

实现可以先在事务外读取候选 queue heads、计算 selector 并写 selected objects，但最终事务必须重新 CAS 相同 heads。事务内原子执行：

1. 锁定 run row 和 `(runId, nodeId)` scheduling cursor。
2. 校验 revision、run status/control epoch、hard limits，且当前无非终态 NodeInstance。
3. 按节点 readiness 重新检查 queue heads。普通节点使用 all/zip；协调节点使用 `18` 的规则。
4. 原子标记所选 queue items consumed，分配 activationSeq。
5. 创建 NodeInstance，保存 selected ValueRefs 和原始 queue item refs。
6. selector/schema 错误则创建并 finalize failed instance；否则创建 queued start attempt 和 durable `attempt_ready` wakeup。允许事务型 built-in在该事务内创建并终结 attempt。
7. 追加 run-local sequenced journal/outbox。

多个 scheduler 对同一 node 竞争时，只有一个事务成功。禁止先消费 queue 再异步创建 NodeInstance。

## Durable Wakeup

```ts
type SchedulerWakeup = {
  id: string
  runId: RunId
  nodeId?: NodeId
  kind: "node_maybe_ready" | "attempt_ready" | "timer" | "settle_run"
  causedBySeq: number
  status: "pending" | "claimed" | "done"
}
```

Wakeup 与导致它的状态变化同事务写入，worker claim 使用 lease。重复 wakeup 合法且应廉价；readiness 与唯一约束保证幂等。内存通知丢失后，pending wakeup 仍可扫描。

## Finalize 原子提交

Executor 结果先完成 schema validation，并把 output、continuation、error detail 和可能的 patch object 写为不可变对象。随后只能通过 `finalize_attempt` 事务提交：

```text
1. 检查 result idempotency key、run control epoch、attempt lease fence 和 deadline。
2. 检查 NodeInstance/attempt 仍处于允许 finalize 的状态。
3. 校验完整 ReadSet；若有 StatePatches，按 `(aggregateKind, aggregateId, lineageKey)` 稳定顺序锁定目标，但每个目标内保留 `statePatches[]` 声明顺序（tool 产生的 patch 使用 callIndex/part order）。Head 已推进时只允许 `04-state-branching.md` 定义的非重叠 path 或 operationId append 做确定性 rebase；其他冲突使全部失败。
4. 每个 patch 都保留自己的 operationId/author 并单独创建一个 Commit；同 target 后一 patch 以事务内前一新 head 为基础顺序验证/应用。全部 patches/Commits、head CAS 和 projections 仍是一个原子事务，不把多个 patch 合成一个丢失身份的 commit。`node_output_commits.output_order` 按原 transition 顺序记录全部 commit IDs；GraphRun `outputCommitId` 取该 run 绑定 WorkingContext branch 在事务后的最后 head。
5. finalize attempt 和 NodeInstance，写 final output ValueRefs。
6. 按 output port/emission/edge 的稳定顺序追加 edge queue items。
7. 若是 OutputNode，按 output contract 写 run output 和 outputSeq。
8. 同事务追加 router decision、node finalized 等 durable journal/outbox。
9. 为当前 node、所有受影响下游 node 和 settle_run 写 durable wakeup。
```

第 3 至 9 步与 `16-domain-consistency.md` 的 context commit/projection 必须在同一数据库事务中。Object bytes 可以预写；事务失败后未引用 object 由 GC 清理。

Head 检查/CAS 失败时，任何 node output、edge emission 和 completion event 都不可见。Storage 可以在同一事务按 canonical policy 对非重叠/append-only stale patch 做确定性 rebase，并在 commit provenance 记录原 base；不能处理的重叠记为 `state_conflict` 并终止该 activation。阶段一不在同一 activation 刷新 ReadSet/retry；调用方需要时从新 head 创建新 activation/run。禁止 arbitrary/LWW 式静默覆盖。

相同 result idempotency key 重放返回既有 finalize 结果。旧 fence、hard cancel 后或不被 drain policy 接受的 late result 只记录隔离诊断，不改变 NodeInstance、Context、queue 或 run output。

## 必须重检当前节点

同一 node 运行期间可以继续收到 queue items，但串行约束阻止新 activation。因而任何 NodeInstance 进入终态时，completion transaction 必须同时写：

```text
node_maybe_ready(current node)
node_maybe_ready(each downstream node that received an emission)
settle_run
```

只检查 downstream 会使运行期间已经积压齐全的当前节点永久 stranded，这是禁止的实现。

## Context Read 与 Commit

GraphRun 从 `inputCommitId` 开始，但 NodeAttempt 必须记录每个实际读取聚合的 `ReadSetEntry`。WorkingContext 写入使用 `StatePatch.baseCommitId` 与所绑定 ContextBranch head CAS；LongTermMemory proposal 不直接改变该 ContextBranch。

阶段一允许并发只读 attempt。并发写 attempt 由 commit transaction 串行化：先提交者推进 ContextBranch head，后提交者若基于旧 commit，仅在 paths 不相交或 append operation 可去重时确定性 rebase，否则显式 conflict/retry。GraphRun 终态时把其最后确认的 Context commit 记录为 `outputCommitId`；candidate selection 或 branch merge仍由上层使用 expected-head CAS 完成。

Runtime 不把 failed/cancelled run 的 commit 自动提升为 Conversation active head。需要 speculative 隔离时，adapter 在创建 run 前提供 sibling ContextBranch；branch 创建和选择不属于 Router fan-out。

## Coordination Node Readiness

普通节点只实现 FIFO all/zip。以下节点覆盖 readiness/consumption，但仍通过同一 `activate_if_ready`、NodeInstance 和 finalize 协议：

- Merge：跨 input heads 选择最小 durable enqueueSeq；
- JoinByKey：按 scalar key 建 per-port FIFO；
- Aggregator：维护 durable count/timeout window；
- Expand：一次消费一个值并有界地产生多 emission。

它们不能绕过 hard limits、control epoch、journal 或 context transaction。具体规则见 `18-coordination-nodes.md`。

## Run Output 与 Stranded Values

Run output contract 来自固定 GraphRevision：

- `single` key 第一次写入成功，第二次使对应 OutputNode/run failed；
- `append` 按 run-local outputSeq 返回；
- required key 在 settle 时必须至少有一个值；
- optional key 未出现时不创建占位。

Run 静默时可以存在不能再组成 activation 的 queue item，例如 zip 缺另一侧、JoinByKey 缺某个 key 或未走到的 Router path。它们是 `stranded values`，保留 ref、原因和 producer trace，不自动算错误。Required output 缺失、queue/global limit 超限或 graph contract violation 仍然是错误，不能被标记为普通 stranded。

## Settle 与完成判据

`settle_run` 必须在锁定 run 与 scheduling projection 的事务中重新判断，不能依赖一次非一致查询。

```text
如果存在可开始/运行/可重试的 attempt，或 actionable pending scheduler wakeup：保持 running；过期、重复或已被状态覆盖的 wakeup在同一锁定范围内标记 done，不能阻止 settle。
如果没有上述工作但存在 open wait、backoff timer 或 Aggregator window：进入 waiting。
如果 soft interrupt 正在 draining：按 17 转入 interrupted，不做 completion。
如果存在可 firing node：补 durable wakeup，保持 running。
否则 graph 已静默：检查 output contract。
  required output 缺失 -> failed(required_output_missing)
  contract 满足 -> completed，并记录 stranded summary 与 outputCommitId
```

Completed run 可以只有 optional outputs，也可以没有输出；调用方若需要 reply，必须在 revision 中把对应 key 声明为 required。

## 失败传播

阶段一默认策略：NodeInstance 最终失败且 retry 已耗尽时，run 原子转为 failed，提升 control epoch、阻止新 activation、撤销其他 lease 并 best-effort cancel executor。Late result 按 `17` 的 fencing 规则隔离。

已提交的 context commit、effect ledger、run output、queue 和 journal 不回滚；它们留在该 ContextBranch 供审计或显式 merge。上层不能因存在 partial output 就自动把 failed/cancelled run 提升为正式 candidate。

Per-node skip/fallback/continue 只有在未来定义成显式、可审计的 failure edge/control node 后才能加入；阶段一不把 error 隐式转换成普通 output。

## 恢复

Runtime checkpoint 与 durable journal 的权威关系见 `16-domain-consistency.md`。恢复过程：

1. 验证固定 graph revision/hash、run manifest、已有 NodeInstance execution snapshots 和 checkpoint checksum。
2. 从 checkpoint `throughSeq + 1` 重放 runtime journal，重建 queue、node、wait、timer、effect 和 counters projection。
3. 不假定旧 running Future 仍存在；按 lease/fence 规则回收 attempt。
4. 为可能 ready 的 node、到期 timer、可重试 attempt 和 settle_run 补 durable wakeup。
5. 继续使用持久化 runInputRef、ValueRef、ReadSet 和 continuation，不重新搜索或请求 adapter 输入。

恢复不得重新消费已绑定 NodeInstance 的 queue item，也不得切换 graph revision、已有 instance 的 preset/execution snapshot 或 ContextBranch。

## Loop 与 Hard Limits

每次循环 firing 都创建新 NodeInstance。Router 的 visits/timeout 是业务级 guard，见 `14-router-node.md`；它不替代 runtime hard limits。

每次 activation、attempt、queue append、Expand emission、open wait/window 和 finalize 都在事务内检查 GraphRevision 固定的 `RunLimits`。任何路径，包括 Router `onLimitOutputs`、协调节点和 resume，都不能绕过。超限产生结构化 `run_limit_exceeded` 并按失败 fencing 终止 run。

Static SCC guard 规则见 `11-graph-definition.md`。静态分析降低误配置风险，global limits 才是最终可靠性边界。

## Summary

NodeInstance 是一次原子消费输入的 activation，NodeAttempt 是可 lease、retry、resume 的一次 executor 调用。Edge value、node input/output 和 run input 都通过 ValueRef 持久化。`activate_if_ready` 负责原子消费与创建，`finalize_attempt` 负责 commit、node、edge、output 和 journal 的原子可见性。每次终态都重检当前节点，durable wakeup 保证 crash 后不会丢调度。GraphRun 只有一个 execution namespace；ContextBranch 才承载跨 run 的版本分支。
