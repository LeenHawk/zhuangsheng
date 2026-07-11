# LLMNode 设计

## 定位

`LLMNode` 表示一次 LLM 驱动的语义执行阶段。

它不是单次 provider HTTP request。一次 `LLMNodeRun` 可以包含多次 model call、多次 custom tool call、流式输出、工具结果回填和最终输出聚合。

图调度层只看到一个节点执行。

trace 层可以看到节点内部的 model calls、tool calls、stream deltas、usage 和错误。

## 基础结构

```ts
type LLMNode = {
  id: string
  name?: string
  kind: "llm"
  model: LlmNodeModelRef
  context: ContextAssemblyRef | ContextAssemblySpec
  tools?: ToolBinding[]
  memory?: MemoryBinding
  output?: LlmOutputSpec
  streaming?: LlmNodeStreaming
  limits?: LlmNodeLimits
  retry?: RetryPolicy
}
```

`LLMNode` 不直接保存固定的 `system prompt + user message`。

它通过 Context Assembly 组合 input 和 memory scopes，最终编译成 `LlmRequestIr`。

## Model Ref

```ts
type LlmNodeModelRef = {
  channelId: string
  modelId: string
  modelName?: string
  operationKey: OperationKey
}
```

含义：

```text
channelId
走哪个上游渠道。

modelId
实际请求里的 model 字符串。

modelName
展示名，可选。

operationKey
本次调用使用哪个标准 API shape / operation。
```

`OperationKey` 来自 `gproxy-protocol`。

它表达 wire shape，不表达真实 provider。

## Context Assembly

`LLMNode.context` 引用上下文装配规则。

执行流程：

```text
node input / memory reads
  -> ContextAssemblyEngine
  -> LlmRequestIr.instructions + messages
  -> ShapeAdapter
  -> provider request
```

Context Assembly 负责：

- 多来源上下文组合
- 排序和插入
- token 预算
- 超限剪裁
- SillyTavern 类预设兼容
- prompt preview

`LLMNode` 只声明自己使用哪套 context assembly，不在节点内手写所有 prompt 拼接逻辑。

## Memory

```ts
type MemoryBinding = {
  reads?: StaticMemoryRead[]
  writes?: StaticMemoryWrite[]
  tools?: MemoryToolGrant[]
}
```

职责边界：

```text
MemoryBinding
负责读写哪些 memory scope。

ContextAssemblySpec
负责把读到的 memory 放到上下文哪个位置。

MemoryToolGrant
负责哪些 memory 操作可以暴露给模型作为工具。
```

LLM 不直接修改底层 memory store。复杂 memory edit 应通过 proposal / patch / validation。

## Tools

工具是 `LLMNode` 的 capability，不默认提升为图节点。

```ts
type ToolBinding = {
  name: string
  description?: string
  inputSchema: JsonSchema
  materialPolicy?: ToolMaterialPolicy
  outputPolicy?: ToolOutputPolicy
  timeoutMs?: number
}
```

工具输入可以包含结构化参数和材料：

```ts
type ToolCallInput = {
  args: unknown
  materials?: ToolMaterialRef[]
}
```

工具输出可以包含多份 typed parts。

只有 `llm_result` 回填给模型，其他部分交给 runtime 分发。

```text
artifact -> artifact memory
memory_patch -> memory manager
memory_proposal -> memory manager
user_message -> UI event
debug -> trace
```

## Tool Loop

`LLMNodeRun` 内部是多轮状态机。

```text
Start
  -> BuildRequestIr
  -> ModelCallStreaming
  -> AccumulateResponse
  -> HasToolCalls?
      -> DispatchTools
      -> AppendToolResults
      -> BuildNextRequestIr
      -> ModelCallStreaming
  -> FinalizeOutput
  -> NodeCompleted
```

custom tool 执行后的继续生成通常需要下一次 provider request。

hosted / built-in tools 由 provider 或上游平台执行，不进入本地 tool dispatcher。

一次 model call 可能返回多个 tool call。第一版可以支持多个 tool call，并顺序执行或小并发执行。

从 Codex 的实现可以借鉴几条规则：

- tool delta 只做聚合、展示或 trace，不立即执行工具
- 完整 tool call 出现后才 dispatch tool
- tool result 写回下一轮模型输入
- tool failure 可以转换成模型可见 tool result，而不是总是让节点失败
- 工具并发最好是 per-tool capability，而不是单一全局开关

## Streaming

```ts
type LlmNodeStreaming = {
  enabled: boolean
  target: "user" | "trace" | "both" | "none"
}
```

语义：

```text
enabled
是否请求 provider streaming。

target
流式事件给用户、trace、二者，或只内部聚合。
```

streaming 只影响观察层，不改变图调度层。

下游节点、Router 和 memory patch 默认等待 finalized node output。

## Stream Finalizer

需要把流式事件聚合成最终非流式结果。

```text
provider stream
  -> LlmStreamEventIr
  -> runtime stream events for UI
  -> StreamFinalizer
  -> final LlmResponseIr
  -> LlmNodeOutput
  -> NodeResult.completed
```

`StreamFinalizer` 负责：

- 拼接 text delta
- 聚合 tool call delta
- 拼接 tool arguments
- 收集 usage
- 记录 finish reason
- 生成最终 `LlmResponseIr`

工具参数未完整前不能执行工具。

流式中间事件和最终节点结果必须分离。UI 可以实时看到 token、reasoning delta、tool argument delta，但 Router、下游节点和 memory patch 仍然等待 finalized `LlmNodeOutput`。

## Output Contract

```ts
type LlmOutputSpec =
  | { mode: "text" }
  | { mode: "json"; schema?: JsonSchema }
```

默认 mode 是 `text`。

```text
text
outputs.default 是裸字符串，不包装成 { text }。

json
outputs.default 是完整 JsonValue。
```

LLMNode 不为 JSON 字段派生 output port。下游节点通过自己的 input selector 使用 JSON Pointer 或 JSONPath 读取需要的字段，见 `11-graph-definition.md`。

`output.mode = json` 时：

```text
stream / response text
  -> final text
  -> parse JSON
  -> validate schema
  -> structured output
```

JSON parse/schema validation 失败时可以在 LLMNode 内按 output retry policy 重试，耗尽后 NodeInstance failed。Text 模式不做结构化校验。

空字符串、`null`、`false`、`0`、空数组和空对象都是明确存在的 finalized value，不能按 truthiness 判断是否产生 output。是否允许由对应 schema 或节点策略决定。

阶段一每个 NodeInstance 对每个 output port 最多产生一个 finalized value。Streaming delta 不是 graph emission。

Usage、finish reason、model call 数量等属于执行 metadata/event/trace，不进入 graph output：

```ts
type LlmExecutionMetadata = {
  usage?: LlmUsageIr
  finishReason?: string
  modelCalls: number
}
```

Router 和下游节点只基于 finalized output。

工具调用 trace 不应该默认塞进 output，应进入 event/trace。

## Limits

```ts
type LlmNodeLimits = {
  maxModelCalls?: number
  maxToolCalls?: number
  timeoutMs?: number
  maxInputTokens?: number
  maxOutputTokens?: number
}
```

runtime 不能依赖模型自觉停止。

达到限制后，节点应该失败、等待人工介入，或按配置返回 partial result。

## Count And Budget

`LLMNode` 在发起 model call 前应执行 token 预算检查。

计数策略：

```text
provider count API
  -> fail / unsupported: gproxy-tokenize::count
```

核心计数类型保持：

```ts
type TokenCount = number
```

Context Assembly 负责预算和剪裁，LLMNode 负责在执行前应用结果和限制。

## Waiting

如果工具、memory edit 或外部操作需要人工确认，LLMNode 可以返回 waiting。

```ts
type NodeResult<O> =
  | { status: "completed"; output: O; memoryPatch?: MemoryPatch }
  | { status: "waiting"; waitFor: WaitCondition; memoryPatch?: MemoryPatch }
  | { status: "failed"; error: NodeError; retryable?: boolean }
```

恢复后继续同一个 `NodeInstance`。

## Execution Flow

```text
1. 接收 node input
2. 执行 deterministic memory reads
3. Context Assembly 编译 LlmRequestIr
4. token count / budget 检查
5. 根据 operationKey 选择 shape adapter
6. 发起 model call，可 streaming
7. stream event 进入 UI/trace，同时进入 StreamFinalizer
8. 如果 response 包含 tool calls，dispatch tools
9. tool results 追加到下一轮 request
10. 继续 model call，直到 final answer 或达到 limits
11. 生成 LlmNodeOutput
12. 解析和校验 output schema
13. 生成 memory patch
14. 执行 deterministic memory writes
15. 返回 NodeResult.completed / waiting / failed
```

## 第一版范围

第一版 LLMNode 建议支持：

- `model`
- `context`
- `tools`
- `memory.reads`
- `memory.writes`
- `streaming`
- `output.mode`
- `output.schema`
- `limits.maxModelCalls`
- `limits.maxToolCalls`
- `limits.timeoutMs`

可以延后：

- 复杂 prompt post process
- 完整 SillyTavern 行为兼容
- LLM 自动修复 JSON
- 复杂 provider extensions UI
- 高级 tool 并发策略

## Summary

`LLMNode` 是一次可流式观察、内部可 tool loop、最终产出非流式 `NodeResult` 的语义执行节点。

流式事件服务 UI/trace，图调度和下游数据流仍然基于最终聚合输出。
