# Graph Definition 与 RouterNode 设计

## 定位

Graph Definition 是 runtime 的静态输入，描述节点和节点之间的 output/input 连接。

Runtime 负责把 graph definition 转成 `GraphRun`、`NodeInstance`、event log、memory patch 和 branch commit。

Graph definition 不应该绑定 Tauri、Axum、数据库模型或 UI 类型。

## Graph

```ts
type GraphDefinition = {
  id: string
  name?: string
  version?: string
  nodes: GraphNode[]
  edges: GraphEdge[]
}
```

Graph definition 不保存 run input，不保存初始 state，也不保存 run-level override。

入口是节点属性，不是 graph 顶层字段。

## Draft And Revision

图编辑分为 draft 和 applied revision：

```text
draft
用户正在编辑，可以暂时不完整，不能被 runtime 执行。

applied revision
通过静态校验的不可变图版本，可以被 runtime 使用。
```

保存并应用 draft 会创建新的 graph revision，不原地覆盖旧 revision。修改节点引用的 preset、模型、channel、port、edge 或节点结构都属于 graph revision。

Preset 本身独立版本化。只修改 preset 内容不会产生 graph revision。

## Run Revision

每个 GraphRun 在创建时固定一个 graph revision，运行中不能切换拓扑版本。

```text
1. Run 中所有 NodeInstance 使用同一个 graph revision。
2. Pending edge queue、node 和 port 都按该 revision 解释。
3. 编辑中的 draft 和新 applied revision 不影响已有 run。
4. 用户要使用新图时，创建新的 GraphRun。
```

RP 中通常从下一次用户输入或 regenerate run 开始使用新 revision。需要立即切换时，取消当前 run，并从同一个 base memory version 创建新 run。

每个 NodeInstance 仍记录 graph revision、node id 和实际使用的 preset version，便于 trace。Preset 独立版本化，默认在 NodeInstance 开始执行时读取最新版本。

## Node

```ts
type GraphNode =
  | InputNode
  | LLMNode
  | RouterNode
  | MemoryNode
  | OutputNode
```

基础字段：

```ts
type BaseNode = {
  id: NodeId
  name?: string
  kind: string
  isEntry?: boolean
  inputSchema?: JsonSchema
  outputSchema?: JsonSchema
  timeoutMs?: number
}
```

节点定义是静态配置。运行时执行记录使用 `NodeInstance`。

同一个 `nodeId` 可能在循环、重试或分支中产生多个 `NodeInstance`。

`isEntry` 表示 run 启动时应调度的入口节点。

阶段一必须支持多个 `isEntry` 节点。多个入口会在同一个 run 中并发调度。

`resume` 不使用 `isEntry`。恢复运行时，runtime 从 checkpoint、event log、pending node instances 和 waiting node instances 恢复调度。

不同启动路径应该由 `InputNode(isEntry)` 后的 `RouterNode` 表达，而不是由 run request 动态指定入口。

## InputNode And OutputNode

阶段一必须支持多输入和多输出。

```ts
type InputNode = BaseNode & {
  kind: "input"
  inputKey?: string
}
```

`inputKey` 用于从 run input 中读取对应输入片段。

例如：

```text
run input = {
  user: {...},
  project: {...},
  files: [...]
}
```

不同 InputNode 可以读取不同 key。

```ts
type OutputNode = BaseNode & {
  kind: "output"
  outputKey?: string
}
```

`outputKey` 用于把多个输出节点的结果写入 run outputs。

例如：

```text
outputs.answer
outputs.memoryProposal
outputs.artifacts
```

如果没有 `outputKey`，默认使用 node id。

## Edge

边只负责静态拓扑连接：把一个节点的 output 端口连到另一个节点的 input 端口。

边不负责：

- 条件判断
- 输入映射
- join 策略
- memory patch

这些能力分别属于 RouterNode、普通节点输出、JoinNode 或 runtime/memory 模块。

```ts
type GraphEdge = {
  id?: string
  from: GraphOutputRef
  to: GraphInputRef
}

type GraphOutputRef = {
  nodeId: NodeId
  output?: string
}

type GraphInputRef = {
  nodeId: NodeId
  input?: string
}
```

`output` 和 `input` 不写时表示默认端口。

## Default Ports

默认端口规则必须统一，因为它同时影响 edge 校验、readiness 判断和 executor 接口：

```text
1. 节点不声明 ports 时，只有一个 input port "default" 和一个 output port "default"。
2. edge 的 output/input 省略时，等价于写 "default"。
3. executor 收到的 inputs 永远是 Record<string, JsonValue>，
   单输入节点收到 { default: value }，不做特殊化。
4. 阶段一所有普通 input port 都是 required，"default" 也是普通 port。
5. 静态校验时，edge 引用的 port 必须存在于节点声明或默认规则中。
```

三处实现（校验、调度、执行）都引用这一份规则，不允许各自猜测。

## Input Contract

输入分成稳定 contract 和图节点 binding：

```ts
type InputPortContract = {
  name: string
  schema?: JsonSchema
}

type InputPortBinding = {
  port: string
  selector?: InputSelector
}
```

`InputPortContract` 描述可复用 executor 最终需要什么。`InputPortBinding` 属于图节点配置，描述如何从该 port 收到的上游完整值中选出 executor input。

默认 selector 是 `whole_value`：

```ts
type InputSelector =
  | { type: "whole_value" }
  | { type: "json_pointer"; pointer: string }
  | {
      type: "json_path"
      path: string
      result: "one" | "many"
    }
```

执行顺序：

```text
raw edge queue value
  -> input selector
  -> cardinality normalization
  -> input port schema validation
  -> NodeInstance.inputs[port]
```

字段投影属于消费者输入侧，不属于生产者 output contract，也不属于 edge。生产者只输出完整值，edge 只传递引用。

### JSON Pointer

JSON Pointer 使用 RFC 6901，适合确定性单值读取：

```text
/decision/route
/items/0/name
```

路径不存在是 `input_contract_violation`。字段存在且值为 `null` 不等于 missing，是否允许由 input schema 判断。

### JSONPath

JSONPath 使用 RFC 9535，提供数组通配、过滤、切片和多结果读取。

```text
$.characters[?(@.active == true)]
$.characters[*].name
```

必须显式声明结果 cardinality：

```text
one
匹配一个值时返回该值；零匹配或多匹配都是 input contract failure。

many
把匹配值按 JSONPath 结果顺序组成数组；零匹配返回 []。
```

`many` 仍然只产生一个 input value，不让节点 firing 多次。需要逐项执行时使用显式 `ExpandNode`。

阶段一禁止任意 JavaScript、eval、自定义脚本函数和非标准宿主语言扩展。实现应限制最大匹配数、递归深度、结果大小和执行时间。

静态校验可以验证 JSON Pointer 与 schema 的明显兼容性，以及 JSONPath 语法。动态过滤的实际结果类型和 cardinality 在 runtime 校验。

Selector 或 schema 校验失败时，runtime 创建 failed NodeInstance，记录 raw queue value ref、selector 和错误，并按默认失败策略使 run failed。重新执行消费者不会改变 finalized 上游值，因此不做消费者重试。

示例：

```ts
{
  from: { nodeId: "draft_answer", output: "answer" },
  to: { nodeId: "final_output", input: "value" }
}
```

含义：`draft_answer.outputs.answer -> final_output.inputs.value`。

如果下游需要特定输入结构，应该由上游节点直接输出，或由中间普通节点整理。

不要在 edge 上加入 mapper 或 condition。这样会让数据变换和控制流散落在边上，难以调试、版本化和复用。

一个 input port 只允许一条入边。多条边指向同一个 input port 是静态校验错误。需要合并多个上游时，显式建 JoinNode / AggregatorNode，不做隐式竞争或隐式合并。

一个 output port 可以有多条出边，语义是广播：每条 edge queue 收到一份独立值。

Port 的运行时语义是持久化 FIFO firing，见 `03-async-runtime.md`。

## Multi Input

多上游汇聚不放在 edge 上。

普通节点的默认 readiness：所有 input edge queue 都有可消费的队首值时 firing。每次 firing 从每个输入原子消费一个值，同一个节点可以反复 firing。

普通多输入按 FIFO 位置配对。需要按业务 key 配对时使用 `JoinByKeyNode`；需要任意一路、窗口或批量语义时使用 `MergeNode` / `AggregatorNode`。

阶段一不支持普通节点的 optional input，因为 scheduler 无法在没有显式窗口策略时判断应该立即执行还是继续等待。

如果需要 `any`、`quorum`、`latest`、窗口聚合或 reducer，使用显式节点表达，例如：

```text
MergeNode / JoinNode / JoinByKeyNode / AggregatorNode
```

这样 join 行为有自己的 node instance、event、trace 和恢复边界，不隐藏在边上。

## RouterNode

RouterNode 集中负责确定性规则 DSL、路径选择、fan-out、default route、payload 转发和 loop guard。条件不放在 edge 上，也不拆成独立 LoopNode。

完整设计见 `14-router-node.md`。

## State Policy

State policy 不属于 GraphDefinition 的顶层结构。

它是 runtime/memory 模块的策略，用于处理 memory patch、并发写入和 branch merge。

Graph definition 只需要声明节点可读写的 memory scope/path。具体冲突如何处理，由 memory runtime 的默认策略或 workspace/run policy 决定。

## Validation

加载 graph definition 时需要做静态校验：

- node id 唯一
- edge from/to 存在
- 至少一个 `isEntry` 节点存在
- edge 引用的 input/output port 存在，或符合默认端口规则
- 每个 input port 至多一条入边
- 普通 input port 都是 required
- Router outputPorts 不为空
- LLMNode 的 operationKey 在 channel 中可用
- 文件大小和 schema 合理

静态分析发现多输入节点的上游路径包含 Router、过滤或不对称循环时，应提示 FIFO 位置配对可能产生残留或错位，建议改用显式 JoinByKeyNode / MergeNode / AggregatorNode。

运行时还需要动态校验：

- input/output schema
- FIFO firing 与原子队首消费
- loop 限制
- memory patch 冲突

## 阶段一实现

阶段一不是 minimal demo，必须覆盖真实图运行需要的基础能力：

- `GraphDefinition`
- `GraphNode`
- `GraphEdge`
- `InputNode`
- `LLMNode`
- `RouterNode`
- `OutputNode`
- 多个 `isEntry` 节点
- 多个 `InputNode`
- 多个 `OutputNode`
- edge 只表达 `output -> input`
- input selector 支持 whole value、RFC 6901 JSON Pointer
- input selector 支持 RFC 9535 JSONPath one/many
- selector 结果执行 input schema validation
- 普通节点按 all-input FIFO firing，可反复激活
- 同一 node + branch 默认串行执行
- RouterNode 通过 output ports 表达分支出口
- Router expression DSL 和 per-run loop guard

延后：

- 任意表达式语言
- LLM judge router
- quorum/latest/window join
- dynamic graph mutation
- graph-level plugin system
- arbitrary reducer

## Summary

Graph definition 应保持静态、可序列化、可校验。

RouterNode 负责图级控制流。LLMNode 负责模型语义执行和内部 tool loop。

边只负责 output/input 连接。控制流由 RouterNode 和显式控制节点表达，不藏在 edge 上。
