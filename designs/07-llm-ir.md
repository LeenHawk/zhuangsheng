# LLMNode 统一 IR

## 定位

`LLMNode` 需要一个统一 IR 来处理请求、响应、tool loop、streaming、usage 和 trace。

统一 IR 是 runtime 内部语义层，不是 provider 协议层。

```text
LLMNode Semantic IR
runtime 内部语义格式。

Provider Shape Adapter
把 IR 映射到某一个标准 API shape，或把标准响应映射回 IR。

gproxy-protocol Wire Types
OpenAI / Claude / Gemini 的标准请求响应类型。
```

调用链：

```text
LLMNode
  -> LlmRequestIr
  -> ShapeAdapter(openai.responses / claude.messages / gemini.generate_content)
  -> gproxy-protocol wire request
  -> HTTP
  -> gproxy-protocol wire response / stream
  -> LlmResponseIr / LlmStreamEventIr
  -> Runtime events / NodeResult
```

IR 的目的不是实现 provider-to-provider 转换。

它只负责让 `LLMNode` 能用统一方式理解模型交互结果。

## Request IR

```ts
type LlmRequestIr = {
  model: string
  instructions?: InstructionIr[]
  messages: LlmMessageIr[]
  tools?: ToolDefinitionIr[]
  toolChoice?: ToolChoiceIr
  responseFormat?: ResponseFormatIr
  generation?: GenerationOptionsIr
  extensions?: ProviderExtensionsIr
  metadata?: Record<string, unknown>
}
```

指令 IR：

```ts
type InstructionIr = {
  role: "system" | "developer" | "policy" | "context"
  content: LlmContentPartIr[]
  source?: InstructionSourceIr
  priority?: number
  cache?: CacheHintIr
}
```

`instructions` 使用数组，因为系统指令、开发者指令、runtime policy、memory 注入、项目上下文和节点 prompt 可能来自不同来源。

消息 IR：

```ts
type LlmMessageIr = {
  role: "system" | "developer" | "user" | "assistant" | "tool"
  content: LlmContentPartIr[]
  toolCallId?: string
}
```

内容片段：

```ts
type LlmContentPartIr =
  | { type: "text"; text: string }
  | { type: "image"; imageRef: string; mimeType?: string }
  | { type: "file"; fileRef: string; mimeType?: string }
```

工具定义：

```ts
type ToolDefinitionIr = {
  name: string
  description?: string
  inputSchema: unknown
}
```

通用生成参数：

```ts
type GenerationOptionsIr = {
  temperature?: number
  topP?: number
  maxOutputTokens?: number
  stop?: string[]
  seed?: number
}
```

## Provider Extensions

Provider 私有扩展按 provider-scoped 分组：

```ts
type ProviderExtensionsIr = {
  openai?: OpenAiExtensions
  claude?: ClaudeExtensions
  gemini?: GeminiExtensions
}
```

每个 provider extension 都可以带高级透传字段：

```ts
type ProviderExtraIr = {
  extraBody?: Record<string, unknown>
  extraHeaders?: Record<string, string>
}
```

示例：

```ts
type OpenAiExtensions = ProviderExtraIr & {
  reasoning?: unknown
  parallelToolCalls?: boolean
  serviceTier?: string
  store?: boolean
  modalities?: string[]
}
```

```ts
type ClaudeExtensions = ProviderExtraIr & {
  thinking?: unknown
  cacheControl?: unknown
  contextManagement?: unknown
  anthropicBeta?: string[]
}
```

```ts
type GeminiExtensions = ProviderExtraIr & {
  safetySettings?: unknown
  generationConfig?: unknown
  cachedContent?: string
  toolConfig?: unknown
}
```

`extraBody` 合并到当前 provider 标准请求 body。

`extraHeaders` 合并到当前 provider 请求 headers。

这些字段是高级逃生口，不做跨 provider 转换，也不保证可移植。

当前 `apiShape` 只读取对应 provider 的 extension。不匹配的 extension 开发期应该 reject。

## Response IR

```ts
type LlmResponseIr = {
  message: LlmAssistantMessageIr
  usage?: LlmUsageIr
  finishReason?: LlmFinishReason
  rawResponseRef?: string
}
```

```ts
type LlmAssistantMessageIr = {
  content: LlmContentPartIr[]
  toolCalls?: ToolCallIr[]
  structuredOutput?: unknown
}
```

```ts
type ToolCallIr = {
  id: string
  name: string
  arguments: unknown
}
```

```ts
type LlmUsageIr = {
  inputTokens?: number
  outputTokens?: number
  totalTokens?: number
  providerRaw?: unknown
}
```

`rawResponseRef` 可以指向原始 provider 响应，用于 debug、审计或回放。

## Streaming IR

```ts
type LlmStreamEventIr =
  | { type: "started"; callId: string }
  | { type: "text_delta"; callId: string; text: string }
  | {
      type: "tool_call_delta"
      callId: string
      toolCallId: string
      name?: string
      argumentsDelta?: string
    }
  | { type: "tool_call_completed"; callId: string; toolCall: ToolCallIr }
  | { type: "usage"; callId: string; usage: LlmUsageIr }
  | { type: "completed"; callId: string; response: LlmResponseIr }
  | { type: "failed"; callId: string; error: LlmApiError }
```

runtime 再把 `LlmStreamEventIr` 转成全局事件：

```text
llm.started
llm.token
llm.completed
tool.started
tool.completed
llm.failed
```

Token delta 不直接改变 working memory。只有节点完成、tool loop 收敛或显式 memory patch 才改变语义状态。

## Shape Adapter

Shape adapter 只负责当前选定 API shape 和 IR 之间的映射。

例如当前节点配置：

```text
apiShape = openai.responses
```

adapter 只做：

```text
LlmRequestIr -> OpenAI Responses request
OpenAI Responses response -> LlmResponseIr
OpenAI Responses stream -> LlmStreamEventIr
```

它不做：

```text
Claude request -> OpenAI request
Gemini request -> OpenAI request
OpenAI response -> Claude response
```

跨 provider 协议转换仍然属于上游反代。

## IR 约束

IR 应该保守，只覆盖 runtime 必需语义。

应该覆盖：

- instructions
- messages
- multimodal content refs
- tool definitions
- tool calls
- structured output
- usage
- finish reason
- stream delta
- provider-scoped extensions
- raw response reference

不应该覆盖：

- provider-to-provider 转换规则
- channel routing
- 反代兼容 hack
- billing 或 credential 管理

当某个 provider 标准能力无法自然放进 IR 时，优先考虑：

```text
1. 是否 runtime 真的需要理解这个能力
2. 是否可以作为 provider-scoped extraBody / extraHeaders 透传到当前 shape adapter
3. 是否应该只保存在 rawResponseRef 中用于审计
4. 是否应该扩展 IR 的语义字段
```
