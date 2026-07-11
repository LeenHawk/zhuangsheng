# Graph Definition 与 Applied Revision

## 定位

Graph Definition 是 runtime 的静态输入，只描述可执行拓扑、节点配置、端口契约和运行限制。`GraphRun`、edge queue、wait、event、context commit 和 branch head 都不是 Graph Definition 的一部分。

Core graph 类型不依赖 Tauri、Axum、数据库模型或 UI。状态与分支术语遵循 `16-domain-consistency.md`：branch 属于 `WorkingContext`，不属于 run；runtime 的调度事实属于 `ExecutionState`。

## Draft 与 Applied Revision

编辑中的 draft 可以暂时不完整，不能执行。Apply 会完成规范化和静态校验，并创建不可变 revision：

```ts
type GraphDraft = {
  graphId: GraphId
  name?: string
  nodes: DraftGraphNode[]
  edges: DraftGraphEdge[]
  runInputSchema?: JsonSchemaSpec
  outputContract: GraphOutputContractEntry[]
  limits?: Partial<RunLimits>
}

type GraphRevision = {
  id: GraphRevisionId
  graphId: GraphId
  revisionNo: number
  schemaVersion: number
  operationTaxonomyVersion: number
  adapterDecoderVersion: number
  contentHash: ContentHash
  nodes: GraphNode[]
  edges: GraphEdge[]
  runInputSchema?: JsonSchemaSpec
  outputContract: GraphOutputContractEntry[]
  schemaCompilations: JsonSchemaCompilation[]
  limits: RunLimits
  createdAt: string
}
```

`GraphRevisionId` 是不可复用的 opaque id。`revisionNo` 只用于同一 graph 内展示，runtime 和 API 一律引用 `GraphRevisionId`。

Apply 必须先补齐默认端口、edge id、默认 selector 和有效的全局 limits，使用 `16-domain-consistency.md` 的唯一 `JsonSchemaSpec` 流水线编译 revision 内全部 schema，再对规范化后的可执行内容做 canonical serialization。`contentHash` 覆盖所有会影响执行的字段，包括 schema version、operation taxonomy/adapter decoder version、schema compilation semantic tuple、node/port/config、edge id、output contract 和 limits；它不覆盖 storage object ID、name、createdAt 等定位/展示 metadata。runtime 加载时同时校验 revision id、hash 和所有 compilation digest，不能按 graphId 读取“最新版”替代固定 revision。

修改 topology、port、selector、node config、model/channel/preset 引用或 limits 都创建新 revision。Preset 内容自身独立版本化；运行时 pin 规则见 `17-runtime-control.md`。

`operationTaxonomyVersion` 固定 revision 中所有 `OperationKey` 的序列化与语义，`adapterDecoderVersion` 固定 Apply 已验证的 request/stream/terminal decoder contract；两者不是 crate semver 或数据库 schema version。Apply 只能从显式 support matrix 选择正整数版本，未知版本或 operation 不存在时拒绝。即使 graph 暂时没有 LLMNode 也记录当前版本对，避免未来 node/config reader 猜默认值；具体 channel 与 NodeInstance pin 规则见 `07-llm-channels-counting.md`。

## Node 与显式端口

阶段一的 applied node 是闭合集合：

```ts
type GraphNode =
  | InputNode
  | LLMNode
  | RouterNode
  | OutputNode
  | MergeNode
  | JoinByKeyNode
  | AggregatorNode
  | ExpandNode

type BaseNode = {
  id: NodeId
  name?: string
  kind: string
  isEntry: boolean
  inputs: InputPortDefinition[]
  outputs: OutputPortDefinition[]
  timeoutMs?: number
  retryPolicy?: RetryPolicy
}
```

`LLMNode` 的模型、tool loop 和 context assembly 配置见 `10-llm-node.md`。Router 见 `14-router-node.md`。四种协调节点见 `18-coordination-nodes.md`。

阶段一不提供未定义语义的 `MemoryNode`。需要持久上下文的具体 node kind 显式声明自己的 memory 字段：LLMNode 使用 `MemoryBinding`，Router 使用只读 `RouterMemoryBinding`；Input/Output/协调节点不带 memory capability。确定性 WorkingContext 写入最终编译为 `StatePatch`，长期记忆变更经 `MemoryManager` proposal。所有类型遵循 `02-memory.md` 与 `16-domain-consistency.md`，节点不能直接操作底层 store。

## Port Definition 与 Consumer Binding

Applied revision 中每个端口都显式存在，runtime 不猜测端口：

```ts
type InputPortDefinition = {
  name: PortName
  schema?: JsonSchemaSpec
  binding: ConsumerInputBinding
}

type OutputPortDefinition = {
  name: PortName
  schema?: JsonSchemaSpec
}

type ConsumerInputBinding = {
  selector: InputSelector
}

type InputSelector =
  | { type: "whole_value" }
  | { type: "json_pointer"; pointer: string }
  | { type: "json_path"; path: string; result: "one" | "many" }
```

Draft 中普通节点可以省略 ports；Apply 将其规范化成一个名为 `default` 的 input 和 output，并填入 `whole_value`。特殊节点使用各自固定的端口约束，不能套用普通默认值。

消费顺序固定为：

```text
raw edge ValueRef
  -> consumer selector
  -> cardinality normalization
  -> input port schema validation
  -> selected ValueRef
  -> NodeInstance.inputs[port]
```

JSON Pointer 使用 RFC 6901。路径 missing 是 `input_contract_violation`，显式 `null` 交给 schema 判断。

JSONPath 使用 RFC 9535。`one` 必须恰好匹配一个值；`many` 返回按标准结果顺序组成的一个数组，零匹配为 `[]`。它不会隐式产生多次 firing；逐项发射必须使用 `ExpandNode`。Apply 校验语法，runtime 还限制匹配数、递归深度、结果大小和执行 fuel。

selector 或 schema 失败时，activation 仍以 failed NodeInstance 的形式持久化，记录 raw queue item ref、selector id 和结构化错误；上游 finalized value 不会被重新计算。

`NodeMemoryBinding/StaticMemoryRead` 的 canonical 类型见 `02-memory.md`。逻辑 scope 由 run binding 解析；一次 attempt 的全部 reads 从同一存储快照解析，并记录完整 `ReadSetEntry[]`，不能用裸整数 memory version 代替。

## InputNode 与 Run Input

阶段一只有 InputNode 可以是 entry，并且每个 InputNode 都是 entry source：

```ts
type InputNode = BaseNode & {
  kind: "input"
  isEntry: true
  inputs: []
  outputs: [OutputPortDefinition]
  runInputSelector: InputSelector
}
```

每个 GraphRun 持久化不可变 `runInputRef`，见 `03-async-runtime.md`。启动事务从该引用解析每个 InputNode 的 selector，验证 output schema，并各创建一次 source activation。进程恢复读取同一引用，不重新向 adapter 请求输入。

InputNode 必须恰好一个 output port且没有入边。普通用户输入不会被注入既有 run；Conversation/Turn 与 wait response 的边界见 `13-conversation-turn-run.md` 和 `17-runtime-control.md`。

## OutputNode 与 Run Output Contract

OutputNode 是 terminal sink：

```ts
type OutputNode = BaseNode & {
  kind: "output"
  isEntry: false
  inputs: [InputPortDefinition]
  outputs: []
  outputKey: string
}

type GraphOutputContractEntry = {
  key: string
  schema?: JsonSchemaSpec
  collection: "single" | "append"
  required: boolean
}
```

每个 contract key 恰好绑定一个 OutputNode，outputKey 全 revision 唯一。OutputNode 消费一个 input value并在 completion 事务中写 durable run output：

- `single`：最多提交一个值；第二次 activation 失败为 `output_cardinality_exceeded`。
- `append`：每次 activation 追加一个值，顺序使用存储层分配的 run-local `outputSeq`。
- `required`：run 静默后仍没有值时，run 失败为 `required_output_missing`。
- 非 required output 未被 Router 选中是合法的，不会生成占位值。

OutputNode 没有出边。Run output 保存 `ValueRef`，大对象不复制到 GraphRun 行或 event。

## Edge 与稳定身份

Draft edge 可以暂时没有 id；Apply 必须生成在该 revision 内唯一且稳定的 id：

```ts
type DraftGraphEdge = {
  id?: EdgeId
  from: GraphOutputRef
  to: GraphInputRef
}

type GraphEdge = {
  id: EdgeId
  from: { nodeId: NodeId; output: PortName }
  to: { nodeId: NodeId; input: PortName }
}
```

Edge 只连接 output 到 input，不承载 condition、mapper、join 或 patch。每个 input port 恰好零或一条入边；普通非 source required input 必须恰好一条。协调节点也通过多个显式 input ports 接收多路数据，不允许多条 edge 竞争同一 port。

一个 output port 可以连接多条 edge，语义是广播。completion 按 output emission 顺序、再按稳定 edge id 顺序写 queue，并给每个 queue item 分配唯一 run-local `enqueueSeq`。Applied edge id 是持久化 queue、trace 和 recovery 的身份，不能在恢复时重算。

## 普通 FIFO All/Zip

普通节点的所有 input port 都是 required。readiness 是所有 input queue 均非空，每次 activation 从每个 port 原子消费一个队首值。这是普通多输入的 FIFO `all/zip`，同一节点可以反复 activation。

它不按业务 key 关联数据。Router、非对称循环或并发上游可能造成位置错配；需要任意一路、按 key、窗口或逐项展开时，必须分别使用 Merge、JoinByKey、Aggregator 或 Expand，不能加入隐式 optional input。

同一 run 内同一 node 最多一个非终态 NodeInstance，因而默认串行；不同 node 可以并发。完整 activation 原子语义见 `03-async-runtime.md`。

## Router 与 Fan-out

Router 只选择 output ports，所有被选择的 ports 在同一个 GraphRun execution path 内发射。fan-out 创建并行数据流，不创建 branch。

Branch 只指 `16-domain-consistency.md` 定义的 ContextBranch。一个 GraphRun 固定绑定一个 `contextId + branchId + inputCommitId`；GraphRun 内不创建 scheduler-owned branch。

## Applied Revision 静态校验

Apply 至少执行以下错误级校验：

### 身份与结构

- node、port、edge、output key 唯一，引用全部存在；
- 至少一个 InputNode，且只有 InputNode 的 `isEntry` 为 true；
- InputNode 零入边、单输出；OutputNode 单输入、零出边；
- 普通 required input 恰好一条入边，每个 input port 至多一条；
- edge 两端 schema 明显不兼容时拒绝；动态 selector 结果留给 runtime；
- required output 从至少一个 entry 静态可达；不可达 node 和永不消费的 output port 至少产生 warning；
- Router、coordination node 及 LLMNode 的专属配置通过各自校验。

### Output 与资源

- 每个 output contract key 恰好对应一个 OutputNode；
- `single/append/required` 组合合法，schema 与 sink input 兼容；
- read binding 的 scope、path、别名和权限声明合法；
- `RunLimits` 每个 hard limit 都存在、为正且不超过 workspace policy 上限。

### SCC 与循环保护

对 entry 可达子图计算 strongly connected components：

1. 自环或多节点 cyclic SCC 必须包含至少一个配置了有限 `maxVisitsPerRun` 或 `timeoutMsPerRun` 的 Router。
2. 从子图中移除这些 guarded Router 后不得仍存在 cycle；这保证每条静态 cycle 至少经过一个业务 guard。
3. Router 的 `onLimitOutputs` 必须离开原 cyclic SCC，避免 limit route 自身成为同一业务循环。
4. 即使通过上述校验，revision 仍必须携带不可关闭的全局 activation、attempt、queue 和 run deadline limits；Router guard 不能替代 hard limits。

动态 selector、Router fan-out、Expand 和循环组合可能放大队列。Apply 应估算明显的无界生产路径并 warning；runtime 始终以 `17-runtime-control.md` 的 hard limits 为最终边界。

## GraphRun Revision Pinning

GraphRun 创建时固定 `graphRevisionId + contentHash`，所有 NodeInstance、edge queue 和 checkpoint 都使用该 revision。Draft 或新 revision 不影响既有 run。恢复也不得切换 revision或 remap queue。

若用户必须立即使用新图，应取消当前 run，并从相同 WorkingContext commit 创建新 GraphRun。Run manifest 记录配置解析策略和可选显式 pins；默认在 NodeInstance 首次执行时把 preset/channel/registry 的实际 revision 写入 instance execution snapshot。恢复已有 activation 不重新解析“最新版”。

## 阶段一边界

阶段一实现：

- immutable applied GraphRevision 与 content hash；
- 显式 ports、consumer selector、schema validation；
- durable run input、InputNode sources、OutputNode contract；
- stable edge id、FIFO queue 与普通 all/zip；
- LLMNode、RouterNode、Merge、JoinByKey、Aggregator、Expand；
- deterministic read binding、ReadSet 和 StatePatch commit 边界；
- SCC guard validation 与不可绕过的 global limits。

阶段一延后：optional 普通 input、动态改图、arbitrary reducer、quorum/latest/sliding window、LLM judge router、graph plugin system 和动态 execution-path cloning。

## Summary

Draft 可以宽松，Applied GraphRevision 必须闭合、不可变、带 hash 且可完全静态解释。端口和 binding 属于消费者节点，edge 只负责稳定连接。InputNode 从 durable run input 启动，OutputNode按明确 contract 收集结果。普通多输入是 FIFO all/zip，其余协调语义由一等节点承担。GraphRun 只有一个 execution namespace；可分支的是 WorkingContext，而不是 scheduler。
