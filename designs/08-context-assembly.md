# Context Assembly 设计

## 定位

Context Assembly 是 runtime 的一等模块，而不是 `LLMNode` 里的简单 prompt 字符串。

它负责把一次 LLM 调用需要看到的多来源上下文组合成统一的 `LlmRequestIr`。

它的目标不是只做 `system prompt + user message`，而是成为 SillyTavern prompt manager 的超集，并融合 agentic runtime 的 memory、state、branch、tool trace 和 artifact。

核心职责：

```text
多来源上下文装配
排序和插入规则
token 预算
超限剪裁
预设兼容
最终编译到 LlmRequestIr
```

## 三层模型

Context Assembly 可以拆成三层：

```text
Context Sources
内容从哪里来。

Assembly Rules
怎么排序、插入、启停、预算和剪裁。

Shape Compiler
怎么编译到 LlmRequestIr，再由 shape adapter 编译到标准 provider request。
```

## 与 LLMNode 的关系

`LLMNode` 使用 Context Assembly，但不直接把 prompt 写死成 `instructions + userMessage`。

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

执行时：

```text
LLMNode input + memory scopes
  -> ContextAssemblyEngine
  -> LlmRequestIr
  -> ShapeAdapter
  -> provider request
```

## ContextAssemblySpec

```ts
type ContextAssemblySpec = {
  id?: string
  name?: string
  mode: "chat" | "completion" | "structured"
  items: ContextItem[]
  budget?: ContextBudgetPolicy
  postProcess?: PromptPostProcessRule[]
  preview?: PreviewPolicy
}
```

`mode` 表示上下文最终倾向于编译成聊天消息、单字符串 completion，还是结构化输出场景。

`items` 是上下文片段列表。

`budget` 定义整体 token 预算和预留策略。

`postProcess` 定义消息合并、角色修正、assistant prefill 等后处理规则。

## ContextPreset Version

Context preset 是独立、可版本化的 long-term memory 对象。`LLMNode` 只引用 preset id，不复制完整 preset，也不默认锁定版本。

```ts
type ContextPresetRef = {
  id: string
}
```

默认行为：每次开始新的节点执行时读取 preset 最新版本。用户修改 preset 后，下一次回复应直接使用新版本；已经 running 或 completed 的 NodeInstance 不受影响。

每次调用记录实际使用的版本用于 trace，但记录不代表锁定：

```ts
type ContextAssemblyTrace = {
  presetId: string
  presetVersion: number
}
```

恢复时由用户选择：

```text
latest（默认）
使用 preset 当前最新版本。

restore
本次恢复临时使用执行点当时的 preset 版本，不回滚当前 preset。
```

Preset 内容变化不产生 graph revision。修改节点引用的 preset id 属于节点配置变化，会产生新的 graph revision。

## ContextItem

```ts
type ContextItem = {
  id: string
  name?: string
  enabled: boolean
  role: "system" | "developer" | "user" | "assistant" | "tool" | "context"
  source: ContextSource
  position: ContextPosition
  priority?: number
  insertionDepth?: number
  budget?: TokenBudgetHint
  overflow?: OverflowPolicy
  cache?: CacheHintIr
}
```

`role` 是编译到 IR 时的语义角色。

`source` 表示内容来源。

`position` 表示插入位置。

`priority` 用于预算不足时的保留顺序。

`insertionDepth` 用于类似 author's note / lorebook depth insertion 的能力。

`budget` 表示该 item 的 token 预算提示。

`overflow` 表示超出预算后的处理方式。

## ContextSource

```ts
type ContextSource =
  | { type: "literal"; text: string }
  | { type: "template"; template: string }
  | { type: "input"; path: string }
  | { type: "memory"; bindingId: string }
  | { type: "working_memory"; path: string }
  | { type: "history"; strategy: HistoryStrategy }
  | { type: "world_info"; selector: WorldInfoSelector }
  | { type: "summary"; scope: string }
  | { type: "tool_trace"; selector: ToolTraceSelector }
  | { type: "artifact"; artifactRef: string }
```

`history`、`world_info`、`summary`、`tool_trace` 和 `artifact` 是 memory-backed source。它们是 Context Assembly 为 RP 和 agent 场景提供的领域视图，不代表独立于 Memory 的存储系统。

这些 source 覆盖两类场景。

SillyTavern 类场景：

```text
system prompt
character description
personality
scenario
example dialogue
world info / lorebook
author note
jailbreak
nudge
start reply with
chat history
summary
user persona
```

Agentic runtime 场景：

```text
memory reads
memory patches
branch context
tool results
event trace
artifact memory
structured output constraints
router hints
```

## ContextPosition

```ts
type ContextPosition =
  | { type: "start" }
  | { type: "before_history" }
  | { type: "history" }
  | { type: "after_history" }
  | { type: "before_user_input" }
  | { type: "user_input" }
  | { type: "assistant_prefill" }
  | { type: "end" }
```

这些位置让不同上下文片段可以稳定插入，而不是拼成一个巨大字符串。

## Memory Binding 与 Prompt 装配

Memory 读取本身不应该藏在 ContextItem 里直接查库。

推荐边界：

```text
MemoryBinding
负责读什么 memory。

ContextAssemblySpec
负责把读到的 memory 放到哪里、用什么 role、什么顺序、是否缓存。
```

示例：

```ts
type StaticMemoryRead = {
  id: string
  scope: string
  query?: string
  limit?: number
  mode: "summary" | "items" | "raw"
}
```

Context item 引用 memory read 结果：

```ts
{
  id: "project_context",
  role: "context",
  source: { type: "memory", bindingId: "project_memory" },
  position: { type: "before_history" },
  enabled: true
}
```

执行流程：

```text
1. 执行 memory.reads
2. 得到 memoryResults[bindingId]
3. ContextAssemblySpec 组合 literal/template/input/state/memory/history
4. 编译成 LlmRequestIr.instructions + messages
5. shape adapter 转成 provider request
```

## 预算

预算是给不同上下文来源分配 token 空间。

一次 LLM 调用受到 context window 限制。例如：

```text
max context = 128k
reserved output = 8k
可用输入预算 = 120k
```

如果所有上下文加起来超过 120k，就必须决定哪些保留、哪些裁剪。

预算示例：

```text
system prompt        必须保留
当前用户输入        必须保留
结构化输出 schema   必须保留
memory 高相关项     最多 20k
历史对话            最多 40k
tool trace           最多 10k
world info           最多 15k
低优先级示例        超出就丢
```

类型示例：

```ts
type ContextBudgetPolicy = {
  maxInputTokens?: number
  reservedOutputTokens?: number
  strategy?: "strict" | "best_effort"
}
```

```ts
type TokenBudgetHint = {
  maxTokens?: number
  minTokens?: number
  reserve?: boolean
}
```

`reserve` 表示该 item 应该优先保留，例如系统指令、用户当前输入、输出 schema。

## 剪裁

剪裁是当上下文超过预算时采取的动作。

常见策略：

```text
drop
直接丢弃低优先级内容。

truncate_head
从开头截断。

truncate_tail
从尾部截断。

keep_recent
保留最近 N 条。

summarize
压缩成摘要。

top_k
只保留相关性最高的 K 条。

dedupe
去重。

compact_trace
把详细 tool trace 压成简短结果。
```

类型示例：

```ts
type OverflowPolicy =
  | { type: "drop" }
  | { type: "truncate_head" }
  | { type: "truncate_tail" }
  | { type: "keep_recent"; count?: number }
  | { type: "summarize"; targetTokens?: number }
  | { type: "top_k"; k: number }
  | { type: "dedupe" }
  | { type: "compact_trace"; targetTokens?: number }
```

示例：

```ts
{
  id: "chat_history",
  source: { type: "history", strategy: { type: "recent" } },
  priority: 50,
  budget: { maxTokens: 30000 },
  overflow: { type: "keep_recent" }
}
```

```ts
{
  id: "system_prompt",
  source: { type: "literal", text: "..." },
  priority: 100,
  budget: { reserve: true },
  overflow: { type: "drop" }
}
```

实际实现中，`reserve: true` 的 item 不应该被 drop。若预算不足，应返回错误或要求上层降低其他部分预算。

## Token Counting

Context Assembly 需要计数能力来做预算。

计数策略遵循 LLM API 设计：

```text
provider count API
  -> fail / unsupported: gproxy-tokenize::count
```

核心计数类型保持简单：

```ts
type TokenCount = number
```

Context Assembly 可以为 preview/debug 记录每个 item 的 token 数，但这属于观察信息，不应该影响核心 `TokenCount` 类型。

## Post Process

为了兼容不同 API shape 和 SillyTavern 类预设，需要后处理规则。

```ts
type PromptPostProcessRule =
  | { type: "merge_adjacent_messages" }
  | { type: "strict_alternation" }
  | { type: "single_prompt" }
  | { type: "assistant_prefill"; text: string }
  | { type: "strip_empty_messages" }
```

这些规则只处理当前 prompt 的编译形状，不做 provider-to-provider 协议转换。

## Preview

Context Assembly 应支持最终 prompt preview。

这对 UI 和调试很重要。

Preview 可以展示：

- 最终 messages / instructions
- 每个 ContextItem 是否进入最终上下文
- 每个 item 的 token 数
- 哪些 item 被裁剪、丢弃或摘要
- 总 input token
- 预留 output token

示例：

```ts
type ContextAssemblyPreview = {
  items: ContextAssemblyPreviewItem[]
  totalInputTokens: TokenCount
  reservedOutputTokens?: TokenCount
}
```

```ts
type ContextAssemblyPreviewItem = {
  itemId: string
  included: boolean
  tokenCount: TokenCount
  action?: "kept" | "dropped" | "truncated" | "summarized"
}
```

## SillyTavern 兼容

可以做导入层：

```text
SillyTavern preset / instruct / context / sysprompt
  -> ContextAssemblySpec
```

优先兼容核心概念：

- system prompt
- instruct template
- context template
- reasoning template
- start reply with
- world info / lorebook
- character card
- example dialogue
- author note
- prompt processing mode

不要一开始完整复制 SillyTavern 所有历史行为。先把这些预设转换成本项目自己的 `ContextItem[]`。

## 总结

Context Assembly 是本项目的上下文工程引擎。

它应该覆盖 SillyTavern prompt manager 的核心能力，并扩展到 agentic runtime 的 memory、state、branch、trace 和 artifact。

预算是“每类上下文最多/至少能占多少 token”。

剪裁是“超出时怎么丢、截、压缩、排序或摘要”。

最终产物是 `LlmRequestIr`，而不是 provider-specific request。provider-specific request 仍由 shape adapter 基于 `gproxy-protocol` 生成。
