# 异步图执行器

## 总体模型

图执行器应该是异步、事件驱动、可持久化的系统。它不应该只是一个同步 DAG 拓扑遍历器。

Agentic 执行需要支持：

- 异步节点
- 条件路由
- 循环
- 暂停和恢复
- 外部事件
- 人工确认
- 长耗时 LLM 或工具调用
- 多节点并发
- 失败重试

运行时可以建模为：

```text
GraphRun
  -> Event Queue
  -> Scheduler
  -> Node Executor
  -> State Store
  -> Trace Store
```

上层可以提供简单 API：

```ts
const result = await graph.invoke(input)
```

底层等价于：

```ts
const run = await graph.start(input)
return await graph.wait(run.id)
```

## GraphRun

一次图执行是一个持久化 run。

```ts
type GraphRun = {
  id: string
  graphId: string
  graphRevision: number
  branchId: string
  baseMemoryVersion: number
  conversationId?: string
  turnId?: string
  userMessageRef?: MemoryRef
  status:
    | "created"
    | "running"
    | "waiting"
    | "interrupted"
    | "completed"
    | "failed"
    | "cancelled"
  headMemoryVersion: number
  createdAt: string
  updatedAt: string
}
```

`GraphRun` 可以被观察、暂停、恢复、分支和回放。

RP 默认每次用户输入创建一个新的 GraphRun。Conversation 跨多个 run 存在；一个 Turn 可以通过 regenerate 产生多个 sibling GraphRun。相关规则见 `13-conversation-turn-run.md`。

GraphRun 在创建时固定 `graphRevision`，运行中不切换拓扑版本。Preset 独立版本化，每个 NodeInstance 开始时默认读取最新版。

`waiting` 和 `interrupted` 的边界：

```text
waiting
run 自己声明需要外部条件才能继续。
例如 user_input、approval、webhook、timer。
由节点返回 waiting 触发。

interrupted
用户或系统主动叫停一个本来可以继续跑的 run。
由外部 interrupt 请求触发。
```

两者的恢复路径相同：从 checkpoint、event log、pending 和 waiting node instances 恢复调度。区别只在触发方，不在恢复机制。

## NodeInstance

节点定义和节点执行实例要区分。

同一个节点可能因为循环、重试或分支执行多次。

```ts
type NodeInstance = {
  id: string
  runId: string
  branchId: string
  nodeId: string
  activationSeq: number
  graphRevision: number
  presetId?: string
  presetVersion?: number
  attempt: number
  status:
    | "pending"
    | "running"
    | "waiting"
    | "completed"
    | "failed"
    | "skipped"
    | "cancelled"
  inputs: Record<string, JsonValue>
  inputRefs: Record<string, EdgeQueueValueRef>
  outputs?: Record<string, JsonValue>
  error?: unknown
  inputMemoryVersion: number
  outputMemoryVersion?: number
  createdAt: string
  updatedAt: string
}
```

`nodeInstanceId` 对 trace、恢复和 replay 很重要。一个 `critic` 节点在同一个 run 里可能出现为 `critic#1`、`critic#2`、`critic#3`。

`activationSeq` 是同一 `run + branch + node` 下单调递增的激活序号。同一个节点可以在一个 run 中激活任意多次。

阶段一同一 `node + branch` 最多一个 running NodeInstance。节点可以反复激活，但默认串行执行；不同节点仍可并发。后续有真实需求时再增加 node-level `maxConcurrency`。

## NodeResult

每个节点异步执行，并返回结构化结果。

```ts
type NodeResult<O> =
  | {
      status: "completed"
      output: O
      memoryPatch?: MemoryPatch
    }
  | {
      status: "waiting"
      waitFor: WaitCondition
      memoryPatch?: MemoryPatch
    }
  | {
      status: "failed"
      error: NodeError
      retryable?: boolean
    }
```

`waiting` 是一等结果，用于外部输入、人工审批、webhook、timer 和外部 job。

```ts
type WaitCondition =
  | { type: "user_input"; prompt: string }
  | { type: "approval"; requestId: string }
  | { type: "webhook"; correlationId: string }
  | { type: "time"; resumeAt: string }
  | { type: "external_job"; jobId: string }
```

## Scheduler

节点完成后，scheduler 应该：

- 持久化 finalized 节点结果
- 应用节点返回的 memory patch
- 沿被激活的 output port 向 edge queue 写入 finalized value
- 记录 durable event
- 检查下游节点 readiness
- 创建下游 node instance
- 将可执行节点入队
- 判断 run 是否完成或进入 waiting 状态

节点完成、memory patch、edge emission 和 completion event 必须在同一个存储事务中提交。否则 crash 后可能出现节点已完成但输出未传播，或者输出被重复传播。

伪代码：

```ts
async function onNodeCompleted(instance, result) {
  const affectedNodes = await storage.transaction(async tx => {
    await tx.completeNodeInstance(instance, result.outputs)
    await tx.applyMemoryPatch(instance, result.memoryPatch)
    const nodes = await tx.emitToEdgeQueues(instance, result.outputs)
    await tx.appendNodeCompletedEvent(instance, result)
    return nodes
  })

  for (const nodeId of affectedNodes) {
    await fireWhileReady(instance.runId, instance.branchId, nodeId)
  }

  await maybeCompleteRun(instance.runId)
}
```

## Edge 语义

边只表达 `output -> input` 连接，见 `11-graph-definition.md`。

```ts
type GraphEdge = {
  id?: string
  from: { nodeId: NodeId; output?: string }
  to: { nodeId: NodeId; input?: string }
}
```

边不承载 condition、mapper、join 和 memory patch。

普通节点完成时发射 finalized outputs。RouterNode 只向它选择的 output ports 发射。LLM token delta 等流式事件不进入 graph edge queue。

## FIFO Firing

每条 edge 是持久化、有序的 finalized-output queue。它在逻辑上是临时数据流，但未消费值必须持久化，才能支持 crash recovery。

```text
1. 上游每次 finalized emission 向对应 edge queue 追加一个值。
2. 阶段一普通节点的所有 input port 都是 required。
3. 节点 firing 条件：所有 input edge queue 非空，且该 node + branch 当前没有 running instance。
4. 节点激活时，从每个 input queue 各原子消费一个队首值。
5. 对 raw value 执行 input selector 和 schema validation，组成 NodeInstance.inputs。
6. NodeInstance.inputRefs 保留 queue value 引用，用于 trace，不重复保存上游大对象。
7. 每次激活创建新的 NodeInstance 并分配 activationSeq。
8. 一个 output port 连多条出边时，每条 edge queue 追加一份独立引用（广播）。
9. RouterNode 只向被激活的 output port 发射。
```

只要输入再次齐全，同一个节点就可以再次 firing。循环、fan-out 和节点重入不需要 frame 或“节点是否历史完成”的全局判断。

同一 node + branch 默认串行，因此 edge emission 天然保持 activationSeq 顺序，不需要阶段一实现并发实例的乱序提交 slot。

Pending queue value 绑定 GraphRun 固定 revision 中的 edge 和目标 input port。新的 graph revision 只用于新的 GraphRun，因此 runtime 不需要在运行中 remap queue value。

Run 结束时可能有残留 queue value（某些节点永远凑不齐输入）。它们作为 stranded outputs 保留在 trace 中，不算错误。

普通多输入按各 edge queue 的 FIFO 位置配对。如果业务必须按 requestId 等 key 配对，使用显式 JoinByKeyNode。

非默认汇聚（any、quorum、latest、窗口聚合）不放在 edge 或隐式规则里，用显式 MergeNode / JoinNode / AggregatorNode 表达。

Optional input 会引入“现在执行还是继续等待”的时间歧义，阶段一不支持。需要 optional/window 语义时使用 AggregatorNode。

## Run 完成与失败传播

Run 完成判据：

```text
没有 pending / ready / running / waiting 的 node instance，
且没有任何节点满足 firing 条件时，run completed。
```

允许部分输出：Router 没走到的 OutputNode 不会执行，`outputs` 里有什么就是什么。

失败传播的默认策略：

```text
节点失败且无重试 -> run failed。
向其他 running 节点发出 cancel。
已完成节点的 memory patch 和 event 保留。
```

per-node 的 onError 策略（skip、fallback、continue）延后，等真实场景出现再设计。

## 循环

循环必须有 runtime 强制限制。

不能只靠 LLM 自觉停止。每次循环都应该创建新的 node instance。

不能直接用节点生命周期总 `activationSeq` 作为循环上限。RP 中同一个 Actor 节点可能在长对话里正常执行上千次。

Loop budget 限定在当前 run、branch 和负责回边选择的 Router 内：

```text
(runId, branchId, routerNodeId) -> visits
```

Router 通过 `maxVisitsPerRun`、`timeoutMsPerRun` 和 `onLimitOutputs` 管理 loop guard，不引入独立 LoopNode。完整设计见 `14-router-node.md`。

GraphRun 本身是 causal boundary。Waiting resume、approval、webhook 和 tool callback 继续原 run，不创建额外 cause；新的语义用户输入创建新的 GraphRun。

```text
planner#1 -> executor#1 -> critic#1
planner#2 -> executor#2 -> critic#2
planner#3 -> output
```
