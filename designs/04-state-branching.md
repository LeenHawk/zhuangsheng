# Memory 版本、截止与分支

## 核心模型

Memory 版本管理建议使用四件套：

```text
WorkingMemory + EventLog + Checkpoint + Branch
```

核心原则：

```text
Memory 不是一个可以随便 mutate 的对象。
Working memory 是由初始输入和一串 patch/event 计算出来的版本化快照。
```

## Patch-based Memory

节点不直接修改底层 memory store，而是返回 memory patch。

```ts
type MemoryPatch = {
  scope: string
  baseVersion: number
  ops: MemoryOp[]
}

type MemoryOp =
  | { op: "set"; path: string; value: unknown }
  | { op: "append"; path: string; value: unknown }
  | { op: "delete"; path: string }
  | { op: "merge"; path: string; value: unknown }
```

Working memory 演进：

```text
memory@0 -> patch#1 -> memory@1 -> patch#2 -> memory@2
```

每个节点实例记录自己基于哪个 memory 版本运行，以及产出了哪个 memory 版本。

```ts
type NodeMemoryLink = {
  nodeInstanceId: string
  inputMemoryVersion: number
  outputMemoryVersion?: number
}
```

## 并发写冲突

如果多个并发节点写同一 memory path，runtime 应该使用 path-specific conflict policy。

```ts
type MemoryPathPolicy = {
  path: string
  mergeStrategy:
    | "last_write_wins"
    | "append_only"
    | "reject_conflict"
    | "custom_reducer"
    | "llm_resolve"
}
```

推荐默认值：

```text
/conversation/messages
append_only

/artifacts/*
reject_conflict 或 namespaced

/memory_proposals
append_only

/final_answer
explicit selection 或 last_write_wins
```

## Checkpoint

Event log 可以完整回放 run，但成本可能变高，所以需要 checkpoint。

```ts
type Checkpoint = {
  runId: string
  branchId: string
  memoryVersion: number
  workingMemorySnapshot: WorkingMemory
  completedNodeInstances: string[]
  pendingNodeInstances: string[]
  activeNodeInstances: string[]
  waitingNodeInstances: string[]
  createdAt: string
}
```

恢复流程：

```text
load latest checkpoint
replay events after checkpoint
restore pending and waiting nodes
reconcile active nodes
resume scheduling
```

进程崩溃后，active node 需要特殊处理。节点执行应该有 lease 和 idempotency key。

```ts
type NodeExecutionLease = {
  instanceId: string
  workerId: string
  leaseUntil: string
  idempotencyKey: string
}
```

如果 active node 没有 durable completion event，runtime 可以根据节点语义选择 retry 或 mark failed。

## 中途截止

中途停止不应该被建模成 kill process，而应该是 run 状态转换。

支持模式：

```text
soft interrupt
让 active node 完成，但不再调度新节点。

hard cancel
尽量取消 active work 并停止 run。

pause
停止调度新 work，直到显式 resume。

wait
节点主动等待外部条件。
```

示例 API：

```ts
await runtime.interruptRun(runId, {
  mode: "soft",
  reason: "user_requested"
})
```

Soft interrupt 语义：

- 标记 run 为 `interrupted`
- 停止调度新的 node instance
- 保留已完成节点的 memory patch
- active node 尽可能继续完成
- 记录 active result，但不继续推进图
- 后续可以 resume 或 fork branch

Hard cancel 语义：

- 标记 cancellation requested
- abort 可取消的 LLM/tool call
- 尽可能把 active node 标记为 cancelled
- 停止 run，除非用户从更早 memory version fork

## Branch

分支应该是从某个 memory version fork 出来的提交链，而不是复制整个 memory。

Branch 的创建入口是 runtime API，不是图内节点行为：

```text
用户（或上层应用）从某个 checkpoint / memory version 调用 fork。
RouterNode 和其他图节点不能创建 branch。
GraphDefinition 中没有 branch 概念。
```

这样图定义保持纯粹的结构描述，branch 是运行历史层面的操作，类似版本控制里的分叉，而不是控制流的一部分。

```ts
await runtime.forkBranch(runId, {
  fromMemoryVersion: 42,
  name: "try-different-approach"
})
```

```ts
type RunBranch = {
  id: string
  runId: string
  parentBranchId?: string
  forkedFromMemoryVersion: number
  headMemoryVersion: number
  status: "active" | "merged" | "abandoned"
}
```

Memory 版本形态：

```text
main:    memory@0 -> memory@1 -> memory@2
                          \
branchA:                   -> memory@3a -> memory@4a
branchB:                   -> memory@3b -> memory@4b
```

每个 memory commit 属于一个 branch。

```ts
type MemoryCommit = {
  id: string
  branchId: string
  parentCommitId: string
  patch: MemoryPatch
  authorNodeInstanceId?: string
}
```

Branch 可以支持：

- 多方案并行
- 假设性推理
- memory edit 的人工 review
- 从早期 checkpoint 恢复
- 使用不同模型或 prompt 做 A/B 执行

## Branch Merge

merge 行为应该由 memory path policy 控制。不能对所有路径都使用 last-write-wins。

```ts
type MergeConflict = {
  path: string
  baseValue: unknown
  leftValue: unknown
  rightValue: unknown
  strategy: "requires_user" | "requires_llm" | "reject"
}
```

第一版可以只支持有限 merge：

- append-only path 自动 merge
- final output 显式选择
- 冲突结构化 memory 要求用户或 LLM 解决
- 暂缓任意自动 merge

## 中途截止与分支结合

一个 run 执行到 `memory@10` 后被 interrupt，用户可以选择：

```text
1. 从 memory@10 resume main
2. 从 memory@7 fork，换目标或策略继续
3. 丢弃 active branch
4. merge 部分 artifact memory 后结束
```

因此 interrupt 后不能丢 memory 版本。它应该成为一个可继续的 execution point。
