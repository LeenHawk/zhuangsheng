# Phase-1 Coordination Nodes

## 定位

普通多输入节点只提供 FIFO `all/zip`：所有 input port 的队首都存在时，各消费一个。本文定义阶段一需要的四个显式协调节点：

```text
Merge(any)       任意一路按 durable 到达顺序通过
JoinByKey        按 scalar key 对齐多路 per-key FIFO
Aggregator       count 或 durable timeout 关闭 tumbling window
Expand           一个数组显式产生多次 finalized emission
```

它们是 built-in deterministic node，不运行 LLM/tool，不创建 branch，不写 WorkingContext，也不执行外部副作用。其 buffer、window、timer 和 cursor 都属于 `ExecutionState`，通过 runtime journal 恢复，不使用 StatePatch。

所有节点仍遵守 `03-async-runtime.md` 的 `ValueRef`、NodeInstance、run-local `enqueueSeq`、scheduling cursor、durable wakeup、control epoch、hard limits 和 atomic finalize。

## 普通 FIFO All/Zip 基线

```text
readiness：每个 input port 的未消费队首都存在。
consume：在一个 activation transaction 中每个 port 各消费一个队首。
inputs：{ portName: selectedValue }
repeat：当前 NodeInstance 终态后重检当前 node；若再次齐全则再 activation。
```

各 port 只保证自身 FIFO。不同 edge 的并发到达不存在隐含 correlation，zip 只按位置配对。需要业务 key 时必须使用 JoinByKey。

普通节点没有 optional input、any、窗口、watermark 或隐式 reducer。

## Durable 到达序

每个 edge queue item 具有 storage 在 GraphRun 内分配的唯一单调 `enqueueSeq`。它是四种节点唯一可用于跨 edge 比较的到达顺序。

同一 completion 产生多个 emission 时，分配顺序固定为：

```text
producer emission index
  -> output port 的决策/声明顺序
  -> applied edge id 顺序
```

数据库 commit wall-clock、worker start time、event publish time 和内存 channel 顺序都不能作为 tie-break。`enqueueSeq` 唯一，因此正常情况下没有相等；恢复时也不能重新编号。

## 共用持久化投影

实现可以使用以下逻辑记录，具体表结构见 storage design：

```ts
type CoordinationBufferItem = {
  runId: RunId
  nodeId: NodeId
  inputPort: PortName
  edgeQueueItemRef: EdgeQueueItemRef
  enqueueSeq: number
  key?: JsonScalar
  status: "indexed" | "reserved" | "consumed" | "stranded" | "cancelled"
}

type AggregationWindow = {
  id: WindowId
  runId: RunId
  nodeId: NodeId
  activationSeq: number
  itemRefs: EdgeQueueItemRef[]
  openedAt: string
  deadlineAt: string
  status: "open" | "ready" | "completed" | "cancelled"
}
```

这些是可重建 projection；edge queue、NodeInstance、NodeAttempt、timer 和 durable journal仍是恢复证据。Projection 更新、queue consumption、NodeInstance/attempt transition、emission、timer 和 journal必须同事务。Merge、JoinByKey和Expand可以在一个 storage transaction内创建并终结 built-in attempt；Aggregator 的 open attempt 直接 completed，NodeInstance 以 `aggregation_window` internal reason waiting，count/timeout关闭时创建 resume attempt终结同一 NodeInstance，不创建外部 WaitRecord。

所有 coordination transition 锁定 `(runId, nodeId)` scheduling cursor，因此同一 node 的多个 worker不能并发选择不同消费集合。

## Merge(any)

### 配置

```ts
type MergeNode = BaseNode & {
  kind: "merge"
  mode: "any"
  inputs: InputPortDefinition[]
  outputs: [OutputPortDefinition]
}
```

阶段一要求至少两个 input ports、恰好一个 output port。所有 input 经过各自 consumer binding 后必须与共同 output schema兼容。

### Readiness 与消费

1. 查看每个 input port 的未消费队首。
2. 至少存在一个队首即 ready。
3. 选择 `enqueueSeq` 最小的队首。
4. 原子消费且只消费该一个 queue item，创建一个 NodeInstance。
5. 未选 port 和同 port 后续 item 保留，当前 instance终态后重检 Merge。

Selector/schema failure 作用于本应被选中的最小 item，产生 failed NodeInstance；不能跳过坏值选择下一项。

### Output

Merge 原样发射选中 port 经 binding 后的 value，不额外包 envelope。NodeInstance/journal记录 `selectedPort`、input queue ref 和 enqueueSeq，因此 trace 不依赖修改业务 payload。

若不同 ports 的业务类型需要保留来源，生产者应输出带 discriminator 的对象，或由下游读取 trace metadata；阶段一不为 Merge 增加可配置 mapper。

### 恢复与边界

选择只依赖 durable heads 和 enqueueSeq。Crash 前事务未提交则没有消费；已提交则 NodeInstance绑定原 item，恢复不会重新选择。

Merge 不取消“较慢分支”，也不丢弃 loser。它是 union/any stream，不是 Promise.race。需要一次性 race-and-cancel 属于未来显式控制节点。

## JoinByKey

### 配置

```ts
type JoinByKeyNode = BaseNode & {
  kind: "join_by_key"
  inputs: InputPortDefinition[]
  outputs: [OutputPortDefinition]
  keySelectors: Record<PortName, JsonPointer>
  maxOpenKeys: number
  maxBufferedPerKeyPerPort: number
}
```

阶段一至少两个 input ports。每个 port 必须配置 RFC 6901 key selector；它在 consumer binding 和 input schema validation 后执行。

Key 只允许非 null JSON scalar：string、boolean 或可规范化 number。Object、array、null、missing、超出 Router DSL v1 数值域的 number 都是 `join_key_invalid`。Number 使用 canonical decimal表示，因此等值的合法 JSON 数值映射到同一 key。String 按 Unicode code point精确匹配，不做大小写或 locale 归一化。

### Index 与 Per-key FIFO

JoinByKey 允许绕过某 port 队首中尚未齐全的其他 key，否则会产生 head-of-line blocking。Scheduler 按 enqueueSeq 扫描尚未 index 的 queue items：

1. 解析 consumer binding、schema 和 key；
2. 写 `CoordinationBufferItem(run,node,port,key,enqueueSeq)`；
3. 保留原 edge queue item ref，尚不绑定 NodeInstance；
4. index 与 durable journal在同一事务提交。

同一 `(port,key)` 的 buffer 严格按 enqueueSeq FIFO。Index 是 deterministic projection，可从未消费 queue item和 journal重建。Key/schema 失败会原子消费坏 item并创建 failed NodeInstance，不能永久卡在 index cursor。

### Readiness、选择与消费

某 key 的每个 input port 至少各有一个 buffered item时，该 key ready。一个 tuple 使用各 port该 key 的队首。

```text
tupleReadySeq = max(selected heads.enqueueSeq)
```

多个 key 同时 ready 时，选择最小 `(tupleReadySeq, canonicalKeyBytes)`；然后按 applied input port声明顺序从每个 `(port,key)` FIFO各消费一个，原子创建一个 NodeInstance。Canonical key只用于理论上的稳定次级排序，不依赖数据库 collation。

同 key 可以形成多个 tuple；每次 instance终态后重新计算。不同 key 不互相阻塞。

### Output

JoinByKey 发射一个对象：

```json
{
  "key": "canonical scalar value",
  "values": {
    "left": "selected value",
    "right": "selected value"
  }
}
```

`key` 保持原 JSON scalar 类型；`values` 的字段按 applied input port顺序构造，并以 canonical JSON存储。Output schema必须描述该 envelope。

### Limits、静默与恢复

`maxOpenKeys` 和 `maxBufferedPerKeyPerPort` 必须为正且不超过 `RunLimits.maxCoordinatorBufferedValues`。超过限制使 node/run failed；不能静默丢最旧值。

图静默且某 key 缺少其他 port时，其 buffered items成为 stranded values，记录 key、port 和 queue refs，不阻止 run completion。阶段一没有 key TTL、outer join 或自动 timeout；需要时间关闭使用显式 Aggregator 或后续专用 keyed-window node。

恢复根据 durable index/cursor继续选择；已绑定 NodeInstance的 tuple不重新入 buffer。

## Aggregator

### 配置

```ts
type AggregatorNode = BaseNode & {
  kind: "aggregator"
  inputs: [InputPortDefinition]
  outputs: [OutputPortDefinition]
  count: number
  timeoutMs: number
}
```

阶段一是单 input、非 keyed、非重叠 tumbling window。`count >= 1`，`timeoutMs > 0`，且二者受 workspace/global policy限制。没有 reducer；窗口保留完整 selected values。

### 打开与推进窗口

没有 open window时，input queue存在队首即：

1. 锁 scheduling cursor并消费最小队首；
2. 分配 activationSeq，创建一个 Aggregator NodeInstance、completed built-in open attempt 和 open window；NodeInstance 因 internal aggregation window 置 waiting；
3. `openedAt` 使用数据库时间，`deadlineAt = openedAt + timeoutMs`；
4. 同事务创建 durable timer、保存第一个 item ref和 journal。

Open window 是该 node 的唯一非终态 NodeInstance。后续 wakeup不创建新 instance，而通过 `advance_window` 事务按 enqueueSeq继续消费 input queue，直到达到 count、当前没有值或达到单事务批量上限。批量上限只让 worker再次 wake，不改变窗口语义。

达到 count时，同一事务创建/终结 resume attempt，把 window和 NodeInstance finalize。`count == 1` 可在打开事务直接完成 NodeInstance，无 waiting 中间态。

### Durable Timeout

Timer 以数据库绝对时间为权威。达到 deadline且 window仍 open时，timer transaction创建 coordinator resume attempt并关闭非空窗口。若“第 count 个 item append”和 timeout并发，先成功锁定 scheduling cursor并提交的事务决定 close reason；另一个读取 terminal window 后幂等退出。

Soft interrupt期间不消费新 item、不关闭窗口输出；到期 timer只标记 due。Resume 后先处理 due timer。Hard cancel取消 window/timer但保留 refs供审计。Crash 恢复发现 deadline已过时补 timer wakeup。

Open window/timer 是 durable blocker，run 进入 waiting而不是把 items判作 stranded。Run hard deadline仍可在 Aggregator timeout之前终止整个 run。

### Output

无论 count 或 timeout关闭，Aggregator 发射恰好一个 finalized envelope：

```json
{
  "items": ["values ordered by enqueueSeq"],
  "closeReason": "count | timeout"
}
```

窗口只在至少一个 item后打开，因此不发射空数组。Window id、openedAt/deadlineAt和 input refs记录在 trace，不默认复制到业务 payload。

Finalize window、NodeInstance、timer、output ValueRef、edge emissions和 journal必须同事务。之后重检当前 node，剩余 queue values打开下一窗口。

### 阶段一限制

不支持 keyed、sliding、session、watermark、late-data修正、允许空窗口或自定义 reducer。Timeout 是 processing-time durable timer，不声称 event-time语义。

## Expand

### 配置

```ts
type ExpandNode = BaseNode & {
  kind: "expand"
  inputs: [InputPortDefinition]
  outputs: [OutputPortDefinition]
  maxItems: number
}
```

Consumer binding 的结果必须是 JSON array。`maxItems` 必须为正且不超过 workspace policy；runtime还检查本次 emission不会超过 queue和run total limits。

### 消费与 Output

Expand 每次 activation消费一个 input queue item。它按 array index产生零个或多个 finalized output values：

```json
{
  "index": 0,
  "item": "original array element"
}
```

空数组合法，NodeInstance completed但不发射。每个 element都使用上述固定 envelope，output schema必须兼容。需要原样 element时，下游用 `/item` consumer selector。

Emission index 等于 array index。向多个 edges广播时，enqueueSeq 按 `(index, applied edge id)` 顺序分配。因此 downstream activation顺序可恢复且不依赖 JSON iteration或 worker timing。

### 原子性、恢复与限制

Expand 是普通 `NodeResult.outputs`“每 port最多一个值”的唯一阶段一例外，但只存在于 built-in finalize路径。一次 activation 的全部 output ValueRefs、全部 queue items、NodeInstance completion和 journal在一个事务可见。

如果 array 超过 maxItems、任一 element/schema不合法、对象持久化失败或全批次会超过 hard queue limit，则整个 activation失败且零 emission可见；不允许 partial expand。对象 bytes可以预写，未引用对象由 GC回收。

Crash 后，未提交事务重新执行选择；已提交事务根据 NodeInstance和producerEmissionIndex识别为完成，不重复 emission。

Expand 不是 streaming map，不并发调用下游，也不提供 per-item retry。它只显式把一个 finalized array转成有序 finalized stream。

## Control 与 Settle 交互

四种节点不能绕过 run control：

- interrupting/interrupted 时不创建 Merge/Join/Expand activation，也不推进 Aggregator window；
- hard cancel/failure 提升 epoch后，旧 coordination wakeup幂等失效；
- resolved timer或已 index values保留到 resume；
- 每次 transition检查 activation、queue、buffer和run deadline limits。

Settle 规则：

- Merge 尚有任意 input item就是 ready；没有则不阻塞；
- JoinByKey 有 ready tuple就是 ready；只有不完整 key时可作为 stranded完成；
- Aggregator open window是 wait blocker，必须等 count、timeout、cancel或run deadline；
- Expand 尚有 input item就是 ready；完成批次后必须重检自身。

## Static Validation

Apply revision时至少验证：

- Merge 至少两个 inputs、一个 output、mode只能 any、schema兼容；
- JoinByKey 至少两个 inputs、每个 port有 key selector、limits合法、output envelope schema兼容；
- Aggregator恰好一入一出，count/timeout合法，output envelope schema兼容；
- Expand恰好一入一出、maxItems合法，input selector/schema保证或允许运行时验证 array；
- 每个 input port恰好一条入边，output port存在；
- 节点位于 cycle时满足 `11-graph-definition.md` 的 SCC guard，且所有路径仍受 global limits。

## Durable Trace

每次决策记录 refs而非默认复制 payload：

```text
coordination.merge_selected
coordination.join_tuple_selected
coordination.window_opened / window_item_added / window_closed
coordination.expand_completed
```

Trace 至少包含 node instance、配置 revision、input queue refs、enqueueSeq、key/window/index metadata、选择或关闭原因和 output refs。事件与对应状态转换同事务追加，publish顺序不改变 durable run-local seq。

## 阶段一边界

阶段一明确不支持：quorum、latest、priority merge、race-and-cancel、outer join、key TTL、keyed/sliding/session window、event-time watermark、arbitrary reducer、partial Expand、跨 GraphRun join和隐式 optional input。

出现真实需求时应增加新的显式 coordination node或版本化现有 config；不能把这些语义偷偷放进 edge、selector 或 scheduler全局分支。

## Summary

普通多输入是 FIFO all/zip。Merge 每次选择全局最早的一个 queue head；JoinByKey按 scalar key维护各 port独立 FIFO并稳定选择 ready tuple；Aggregator把 count/timeout window作为 durable NodeInstance和timer；Expand以有界原子批次产生显式多 emission。四者都使用同一 scheduling cursor、ValueRef、journal、epoch和hard limits，因而 crash、interrupt和重复 wakeup不会改变消费结果。
