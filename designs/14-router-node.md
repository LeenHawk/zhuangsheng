# RouterNode 设计

## 定位

RouterNode 是确定性的图级控制流节点。它集中负责规则判断、路径选择、fan-out、default route 和循环保护，避免把控制逻辑拆成大量零碎节点。

Edge 仍然只负责 output/input 连接，控制条件不放在 edge 上。

Router 不负责 LLM/tool 调用、Memory 写入、payload 转换、join/aggregate 或 branch 创建。

## 类型

```ts
type RouterNode = BaseNode & {
  kind: "router"
  outputPorts: string[]
  rules: RouterRule[]
  matchMode?: "first" | "all"
  defaultOutputs?: string[]
  payloadPort?: string
  memory?: RouterMemoryBinding
  limits?: RouterLimits
}

type RouterRule = {
  id: string
  when: string
  outputs: string[]
}
```

`when` 是无副作用的 Router expression。`outputs` 必须引用 Router 已声明的 output ports。

## DSL Environment

表达式只访问明确暴露的数据：

```text
inputs
经过 input selector 和 schema validation 的输入。

memory
通过 RouterMemoryBinding 显式读取并命名的 Memory。

control
当前 Router 在本次 GraphRun/branch 中的 visits、elapsedMs 等控制信息。
```

示例：

```text
inputs.score < 0.8 && control.visits < 3
inputs.route in ["done", "stop"]
memory.scene.phase == "ending"
```

Router 不能通过 DSL 访问 Secret、网络、文件系统、tool 或未授权 Memory，也不能执行写操作。

## DSL Semantics

DSL 以 CEL 的安全表达式语义为目标：支持布尔逻辑、比较、字符串、数字、列表、对象访问和受限纯函数。

禁止循环、赋值、I/O、任意代码执行和宿主语言回调。实现前再评估 Rust CEL 实现，不在设计阶段锁定具体 crate。

表达式必须可序列化、可静态解析、可设置执行时间和复杂度限制。

JSON Pointer / JSONPath 属于 input binding，用于从 raw edge value 选值；Router DSL 对已解析的 `inputs` 做判断，两者职责不同。

## Match Mode

`first` 是默认模式：按 rules 顺序求值，第一条 true 的规则决定 outputs，后续规则不再执行。

`all` 会执行全部规则，合并并去重所有匹配规则的 outputs，用于 fan-out。

如果没有规则匹配：

```text
有 defaultOutputs -> 激活 default outputs
没有 defaultOutputs -> Router failed
```

Router 不静默吞掉路径。

## Payload

所有被选中的 output ports 默认发送同一个 payload。

```text
声明 payloadPort
payload = inputs[payloadPort]

未声明 payloadPort
payload = 完整 inputs 对象
```

Router 只原样转发 payload，不构造对象、不修改字段。下游通过自己的 input selector 读取需要的部分。

每个被选中 output port 最多产生一个 finalized emission。

## Memory Binding

Router 可以读取显式绑定的 Memory，避免为了简单状态判断额外增加 MemoryReadNode。

```ts
type RouterMemoryBinding = {
  reads: Array<{
    as: string
    scope: string
    path: string
  }>
}
```

读取结果按 `as` 暴露到 `memory`。Trace 应记录使用的 MemoryRef/version，但不能记录 secret。

## Loop Guard

循环由 Router output port 连接回前序节点表达，不引入独立 LoopNode。

```ts
type RouterLimits = {
  maxVisitsPerRun?: number
  timeoutMsPerRun?: number
  onLimitOutputs?: string[]
}
```

计数作用域：

```text
(runId, branchId, routerNodeId)
```

超限时不再执行普通 rules：有 `onLimitOutputs` 时激活这些 ports，否则 Router failed，错误为 `control_limit_exceeded`。

不能用节点生命周期总 activationSeq 作为循环上限，因为 RP 对话中同一个节点可能正常执行很多次。

## Example

```ts
{
  id: "route_after_critic",
  kind: "router",
  outputPorts: ["retry", "done", "needs_human", "limit_reached"],
  matchMode: "first",
  payloadPort: "payload",
  rules: [
    {
      id: "human_review",
      when: "inputs.needsHuman == true",
      outputs: ["needs_human"]
    },
    {
      id: "retry_low_score",
      when: "inputs.score < 0.8 && control.visits < 3",
      outputs: ["retry"]
    },
    {
      id: "finish",
      when: "inputs.score >= 0.8",
      outputs: ["done"]
    }
  ],
  defaultOutputs: ["done"],
  limits: {
    maxVisitsPerRun: 4,
    onLimitOutputs: ["limit_reached"]
  }
}
```

## Validation And Trace

静态校验：rule id 唯一、表达式可解析、引用变量可用、所有 outputs 存在、limit outputs 存在。

运行时记录 `router.decision`：匹配规则、选择 ports、Memory versions、control visits 和未匹配/超限原因。

不要把完整敏感 payload 默认复制到 decision event；保存 input/output refs 或经过过滤的 preview。
