# WorkingContext 版本、分支与恢复

## 定位

本文件描述跨 GraphRun 延续的 WorkingContext。ExecutionState、LongTermMemory 和 ArtifactObject 是不同 aggregate，统一边界见 `16-domain-consistency.md`。

```text
WorkingContext
  -> ContextBranch
      -> Commit graph
          -> StatePatch / VersionSnapshot
```

Branch 属于 `contextId`，不属于 `runId`。GraphRun 只绑定一个 context branch 和执行起点；Router fan-out 不创建 branch。

## Run Binding

```ts
type GraphRunContextBinding = {
  contextId: string
  branchId: string
  inputCommitId: string
  outputCommitId?: string
}
```

普通工作流没有 Conversation 时，runtime 创建临时 context/root branch。Core runtime 只理解 opaque context/branch/commit ID，不理解 Turn、candidate 或角色消息。

Run 创建时 CAS/校验 branch head 等于 `inputCommitId`，随后固定执行输入 snapshot。NodeInstance 记录自己的 WorkingContext read set；同一 LLM tool loop 不自动切换到 branch 最新值。

Run 中允许多个节点产生 patch。存储层以 branch head 为 commit sequencer：每个 completion 事务校验 patch base/read set，然后串行推进 head。第二个并发结果必须按下述冲突策略 rebase 或失败，不能覆盖第一个结果。

## StatePatch 与 Commit

WorkingContext 使用 canonical `StatePatch` 的受限视图：

```ts
type WorkingContextPatch = StatePatch & {
  aggregateKind: "working_context"
  aggregateId: ContextId
  lineageKey: BranchId
  baseCommitId: string
  operationId: string
}
```

完整类型还包含 `ops/schemaVersion/policyVersion/author`，见 `16-domain-consistency.md`。本文后续简称 patch；`lineageKey` 即 context branch ID。

JSON Pointer 使用 RFC 6901；阶段一支持 RFC 6902 的 `add/remove/replace/test` 和受控 append。`move/copy` 延后，避免隐藏读集合。Patch 有 schema/policy version和 author；origin run/node 属于最终 Commit metadata。Patch bytes 存入 content-addressed object。

```ts
type ContextCommit = {
  id: string
  contextId: string
  branchId: string
  parentCommitIds: string[]
  patchRef?: string
  snapshotRef?: string
  operationId: string
  mergeResolutionRef?: string
  sequenceNo: number
  schemaVersion: number
  policyVersion: number
  author: ActorRef
  originRunId?: string
  originNodeInstanceId?: string
  createdAt: string
}
```

这是 canonical Commit 的 `aggregateKind=working_context` 领域视图；LongTermMemory 的 `sourceProposalId` 不适用于本类型。

API/CAS 使用 commit ID。`sequenceNo` 仅用于 branch 内展示和稳定排序，不能脱离 branch 定位版本。普通 commit 一个 parent，merge commit 两个 parent。

## 并发提交

提交规则：

```text
current head == base
  -> 校验并直接提交。

current head != base
  -> 收集 base..head 的已提交 paths/operation IDs。
  -> 两边 path 不相交时，在 current head 上确定性 rebase。
  -> append-only op 按 operationId 去重并稳定追加。
  -> ancestor/descendant 或同 path 写冲突时拒绝。
```

Path 冲突按 JSON Pointer 前缀判断，`/a` 与 `/a/b` 重叠。Array index 的并发结构修改默认冲突；业务 append 使用稳定 element ID/operation ID，不能依赖“当前最后一个 index”。

冲突使该 node transition 返回 `state_conflict` 并终止当前 activation。阶段一 RetryPolicy 不刷新 ReadSet，因此不自动重跑昂贵/有副作用节点；需要重新读取时由调用方从新 head 创建新 activation/run。

阶段一不使用 last-write-wins、custom reducer 或 LLM 自动写最终 merge patch。

## Branch

`ContextBranch` 的 canonical 类型见 `16-domain-consistency.md`；storage projection 可以另带 `createdAt/updatedAt/name`，不改变 branch identity/head CAS 语义。

Fork 是 runtime/application API，不是 graph node：

```ts
forkContext({
  contextId,
  sourceBranchId,
  fromCommitId,
  expectedSourceHead?,
  idempotencyKey
})
```

`fromCommitId` 必须在 source branch 可达历史中。Fork 只创建 branch/head 引用，不复制 snapshot 或 artifact bytes。GraphDefinition、RouterNode 和 tool 都不能隐式创建 branch。

常见用途：

- regenerate/sibling candidate；
- 从 interrupt execution point 尝试另一策略；
- 假设性工作流；
- 人工 review 后选择某组 state changes。

Abandon 只是状态转换，不立即删除提交；retention、audit root 和 GC 见 `06-persistent-versioning.md`。

## Conversation Candidate 隔离

Conversation service 在用户消息 commit 后，为每个 candidate run 创建 sibling branch：

```text
active head -> user commit U
                    |-> candidate branch A -> run commits -> assistant A
                    |-> candidate branch B -> run commits -> assistant B
```

Failed/cancelled run 可以保留 branch、event 和 artifact 供审计，但不会推进 Conversation active branch/head。选择 candidate 时使用 expected active head CAS；新的用户消息只追加到已选择 branch。

完整 schema 与旧 Turn 切换规则见 `13-conversation-turn-run.md`。

## Merge MVP

Merge 输入：

```ts
type ExplicitMergeSelection = {
  conflictId: string
  path: string
  resolution:
    | { type: "value"; value: JsonValue }
    | { type: "artifact_ref"; artifactRef: ArtifactRef }
}

type MergeContextCommand = {
  contextId: string
  sourceBranchId: string
  targetBranchId: string
  expectedSourceHead: string
  expectedTargetHead: string
  sourceDisposition: "mark_merged" | "keep_active"
  selections?: ExplicitMergeSelection[]
  idempotencyKey: string
}
```

Runtime 计算唯一最近共同 ancestor 作为 merge base，并做三方比较。“最近”候选是不再为其他共同 ancestor 的 ancestor 的极大共同节点；阶段一若存在多个候选（criss-cross history）则返回 `ambiguous_merge_base`，不按时间或 ID 猜测，也不做 recursive merge。阶段一只自动处理：

- operation ID 可去重的 append-only 集合；
- 两侧不相交 path；
- 两侧最终值完全相同；
- 调用方显式选择的 final value/artifact ref。

其他重叠生成持久化 `MergeConflict`，target head 不变。解决使用新的 `mergeContext` 命令和 idempotency key；每个 selection 必须引用同一 context/source/target/base/expected-head 集合中尚未解决的 conflict ID，`path` 必须与记录相同，resolution 必须通过当前 state schema、artifact grant 和引用可达性校验。选择未覆盖全部重叠时只返回仍未解决的 conflicts，不创建部分 merge commit。

成功时在一个事务中创建双 parent commit、CAS target head、更新 projection、把已使用的 conflict 标记 resolved、按 `sourceDisposition` 把 source 标记为 `merged` 或保持 `active`，并写 durable event。`sourceDisposition` 不改变 source head；无论选哪个值，source branch 的 commit 都仍作为可达审计历史保留。

Target 在分析后被其他事务推进时 CAS 失败，重新计算 merge；不能把旧 resolution 强行提交。LLM 可以生成 resolution proposal，但 Memory/State manager 仍需验证，replay 不重新调用 LLM。

## Interrupt、Resume 与 Fork

Soft interrupt 只停止 ExecutionState 继续调度；已经提交的 WorkingContext commit 保留。Run 到达 `interrupted` 后：

```text
resume 原 run
  -> 继续同一个 context branch，从已提交 head 调度 pending queue。

fork 新方案
  -> 从可达历史 commit 创建新 context branch和新的 GraphRun。

cancel
  -> 原 run terminal；只能创建新 run/branch，不能 resume。
```

Interrupt/control epoch、late completion 和 WaitRecord 的精确规则见 `17-runtime-control.md`。不能把 kill process 当作状态转换。

## 两种快照

```text
RuntimeCheckpoint
GraphRun 的 scheduler 一致切面，含 through durable seq、queue/wait/attempt/control/effect 状态。

VersionSnapshot
某个 context commit 的物化 JSON，只缩短 StatePatch replay。
```

恢复 GraphRun 时加载 RuntimeCheckpoint/运行表并重放 durable journal，同时校验其引用的 graph revision、context commit 和 VersionSnapshot。只加载 WorkingContext snapshot 无法恢复 edge queue、wait、lease 或副作用结果。

进程崩溃前的 running attempt 必须根据 lease/effect ledger 协调；不能因为存在输入就直接再次调用外部工具。

## Invariants

- 一个 context branch 恰有一个可 CAS 的 head commit。
- 每个非 root commit 的 parents 必须存在且属于同一 context aggregate。
- Run 的 `inputCommitId` 在创建后不可变；`outputCommitId` 只指向已提交 commit。
- Graph data fan-out、context branch 和 Conversation selection 是三个不同概念。
- Branch 状态不会决定 object 可回收性；只有完整 reachability/retention 扫描可以删除物理对象。
- Merge、candidate selection、node state commit 都同时写 projection 与 durable audit/event。
