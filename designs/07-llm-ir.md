# LLMNode 统一 IR

## 定位

`LLMNode` 使用统一 IR 表达一次或多次 model call 的语义输入、输出、tool transcript、streaming、usage 和 trace。

统一 IR 是 runtime 内部语义层，不是 provider 协议层，也不是 provider-to-provider transform：

```text
ContextAssemblyOutput
  -> LLMNode Request Builder
  -> LlmRequestIr
  -> ShapeAdapter
  -> gproxy-protocol wire request
  -> provider response / stream
  -> LlmResponseIr / LlmStreamEventIr
```

`ShapeAdapter` 只处理当前 `OperationKey` 所代表的标准 API shape。上游兼容代理仍然负责真实 provider 映射。

## Request IR

```ts
type LlmRequestIr = {
  model: string
  instructions: InstructionIr[]
  transcript: LlmTurnItemIr[]
  tools?: ToolDescriptorIr[]
  hostedTools?: HostedToolDescriptorIr[]
  toolChoice?: ToolChoiceIr
  responseFormat?: ResponseFormatIr
  generation?: GenerationOptionsIr
  extensions?: ProviderExtensionsIr
  metadata?: LlmMetadataIr
  continuation?: OpaqueContinuationRef
}
```

`ContextAssemblyEngine` 不构造这里的完整对象。它只返回 instructions、初始 messages、provenance 和 budget report。`LLMNode Request Builder` 再加入 model、tools、response format、generation、extensions，并把初始 messages 编译成 transcript items。

工具进入请求前必须已经通过 `ToolGrant` / `HostedToolBinding` 与 registry snapshot 校验。canonical capability 定义见 `19-tools-artifacts.md`；Request Builder 只把 model-facing 子集放入 IR，effect、scope、approval 和 executor 信息不会交给 provider。

```ts
type ToolDescriptorIr = {
  name: string
  description?: string
  inputSchema: JsonSchemaSpec
}

type HostedToolDescriptorIr = {
  bindingId: string
  hostedKind: string
  config: Record<string, string | number | boolean>
}
```

```ts
type ToolChoiceIr =
  | { type: "auto" }
  | { type: "none" }
  | { type: "required" }
  | { type: "named"; name: string }

type ResponseFormatIr =
  | { type: "text" }
  | { type: "json"; schema?: JsonSchemaSpec; strict?: boolean }

type GenerationOptionsIr = {
  temperature?: number
  topP?: number
  maxOutputTokens?: number
  stop?: string[]
  seed?: number
}
```

`JsonSchemaSpec` 来自 `16-domain-consistency.md`；IR 保留完整 canonical spec，ShapeAdapter 只可按当前 provider 的显式 supported subset 转码或拒绝，不能把未知 keyword 静默删除后声称 strict。

`LlmNodeLimits.maxOutputTokens` 是 runtime 硬上限；`generation.maxOutputTokens` 是请求偏好。两者同时存在时取较小值。

## Instructions

```ts
type InstructionIr = {
  id: string
  role: "system" | "developer" | "policy" | "context"
  content: LlmContentPartIr[]
  provenance: ContextProvenanceIr
}
```

`role` 表达编译语义，不表达内容可信度。只有 Context Assembly 校验为允许高权限角色的来源，才能产生 `policy`、`system` 或 `developer` instruction。

`priority` 不放在 Instruction IR 中。预算优先级由 assembly report 记录，不能被误解为安全权限。

## 有序 Transcript

tool loop 需要保留模型原始输出顺序。不能把 assistant text 和 tool calls 分别收集后再随意拼接。

```ts
type LlmTurnItemIr =
  | LlmMessageItemIr
  | LlmAssistantToolCallItemIr
  | LlmToolResultItemIr
  | LlmHostedToolItemIr
  | LlmReasoningItemIr
```

```ts
type LlmMessageItemIr = {
  type: "message"
  id: string
  role: "user" | "assistant"
  content: LlmContentPartIr[]
  provenance?: ContextProvenanceIr
}
```

system、developer 和 runtime policy 不伪装成 conversation message，它们进入 `instructions`。tool result 也不使用普通 message role，而是使用独立 item，避免伪造调用关系。

```ts
type LlmAssistantToolCallItemIr = {
  type: "assistant_tool_call"
  id: string
  call: ToolCallIr
}

type ToolCallIr = {
  id: string
  providerCallId?: string
  name: string
  arguments: unknown
}
```

`id` 是 runtime local id，必须在同一 NodeInstance 中稳定且唯一。如果 provider 没有 call id，adapter 按 call position 和 model-call id 合成。`providerCallId` 只用于 same-shape 回填。

只有完整聚合并通过 JSON/schema 校验的 arguments 才能进入 dispatcher。原始或不完整 argument delta 只属于 stream/trace。

```ts
type LlmToolResultItemIr = {
  type: "tool_result"
  id: string
  toolCallId: string
  toolName: string
  outcome: "success" | "error" | "denied"
  content: LlmContentPartIr[]
}
```

模型可见的错误必须是有界、脱敏的内容。详细错误只进入受控 trace，不得把底层数据库、路径、credential 或 provider response 原样回填。

Effect ledger 的 `outcome_unknown` 不会生成 tool result 或继续 model loop；它先进入 durable waiting，直到人工/协调器把结果收敛为 success、error 或终止节点。

```ts
type LlmHostedToolItemIr = {
  type: "hosted_tool"
  id: string
  bindingId: string
  kind: string
  phase: "requested" | "running" | "completed" | "failed"
  displayContent?: LlmContentPartIr[]
  opaqueItemRef?: OpaqueContinuationRef
}
```

hosted tool 不进入本地 dispatcher，但必须来自显式 `HostedToolBinding`。它仍然受 grant、资源范围、审计和成本策略约束。

```ts
type LlmReasoningItemIr = {
  type: "reasoning"
  id: string
  summary?: string
  opaqueItemRef?: OpaqueContinuationRef
}
```

IR 不要求暴露 provider 的私有 reasoning。可展示的 summary 与 adapter-owned opaque continuation 分离；raw reasoning 默认不持久化。

## Content Parts

```ts
type LlmContentPartIr =
  | { type: "text"; text: string }
  | { type: "image"; artifactRef: ArtifactRef }
  | { type: "file"; artifactRef: ArtifactRef }
```

`artifactRef` 必须已经通过 artifact binding/grant 校验，其 immutable `mediaType` 是 ShapeAdapter 的唯一 MIME 权威；ContentPart 不允许携带可冲突的 override。`image` 还要求已验证 media type 属于 adapter 支持的 image allowlist，其他类型使用 `file` 或在构建 IR 前失败。IR 不允许任意本地路径或任意 URL 绕过 artifact/binding 校验。

## Opaque Continuation

Claude thinking signature、Gemini thought signature、OpenAI continuation item 等内容可能是继续同一 shape tool loop 的必要材料，但 runtime 不应解释其内部结构。

```ts
type InternalSensitiveEntryRef = {
  objectId: string
  entryKey: string
}

type OpaqueContinuationRef = {
  adapterKey: string
  operationKey: OperationKey
  operationTaxonomyVersion: number
  adapterDecoderVersion: number
  modelCallId: string
  ref: InternalSensitiveEntryRef
  digest: string
  expiresAt?: string
}
```

`digest` 是随机 nonce 加密后整个 ciphertext container 的 SHA-256，用于 object/ref 完整性；不能保存可用于明文等值关联的 plaintext hash。内容真实性由 AEAD tag/AAD 校验。同一 model EffectAttempt 的 top-level continuation、hosted item 和 reasoning sidecar 收进一个 versioned encrypted bundle；多个 ref 可共享 objectId，但使用不同 authenticated `entryKey`。

约束：

- 只有创建它的 adapter 可以解析。
- 只能在相同 `OperationKey + operationTaxonomyVersion + adapterDecoderVersion` 下回填；任一 version 未知或切换 shape 时必须拒绝或重新构造显式 transcript。
- `ref.objectId` 必须指向 `12-secret-store.md` 定义的 internal-sensitive encrypted bundle，`entryKey` 必须在 AEAD 验证后的 bundle index 中唯一命中；它不是可枚举 ArtifactRef，不进入普通 event、preview 或日志。
- 阶段一每 entry 默认上限 256 KiB，每 bundle 最多 64 entries/总计 1 MiB（workspace policy 可收窄）；超限时节点失败并给出可诊断但不含内容的错误。
- checkpoint 必须保存 ref 和 digest，不能只保存在进程内。

## Response IR

```ts
type LlmResponseIr = {
  modelCallId: string
  items: LlmTurnItemIr[]
  usage?: LlmUsageIr
  finishReason?: LlmFinishReason
  continuation?: OpaqueContinuationRef
  rawResponseRef?: ArtifactRef
}
```

```ts
type LlmFinishReason =
  | "completed"
  | "tool_calls"
  | "length"
  | "content_filter"
  | "cancelled"
  | "unknown"

type LlmUsageIr = {
  inputTokens?: number
  outputTokens?: number
  totalTokens?: number
  cachedInputTokens?: number
  reasoningTokens?: number
}
```

最终 `LlmNodeOutput` 从最后收敛轮的 assistant message items 派生。中间 preamble、tool calls、hosted items 和 reasoning 仍保留在 trace/transcript，但不自动拼进最终 output。

## Same-shape Round-trip Invariant

每个 shape adapter 必须满足：

```text
wire response
  -> ordered response items + opaque continuation
  -> append validated tool result items
  -> same-shape wire request
```

回填后的请求必须保留 provider 要求的调用关联、item 顺序和 opaque signature。允许丢弃只用于 UI 的 delta，但不能丢弃继续 tool loop 所需的语义或 sidecar。

该 invariant 只保证同一 shape 的 round-trip，不保证 OpenAI、Claude、Gemini 之间可移植。若 adapter 无法满足，应在发起首次调用前拒绝 tool loop 能力，而不是运行到第二轮才静默降级。

## Streaming IR

所有 stream event 带 model-call 内从 0 开始、每个 normalized event 加 1 的连续 `seq`。若 provider 没有序号，adapter 按验证后的 wire arrival order分配；有序号时先完成 provider-native 去重/顺序校验再归一化。

```ts
type LlmStreamEventIr =
  | { type: "started"; callId: string; seq: number }
  | { type: "text_delta"; callId: string; seq: number; itemId: string; text: string }
  | { type: "reasoning_delta"; callId: string; seq: number; itemId: string; text: string }
  | {
      type: "tool_call_delta"
      callId: string
      seq: number
      itemId: string
      toolCallId: string
      name?: string
      argumentsDelta?: string
    }
  | { type: "tool_call_completed"; callId: string; seq: number; item: LlmAssistantToolCallItemIr }
  | { type: "hosted_tool_event"; callId: string; seq: number; item: LlmHostedToolItemIr }
  | { type: "usage"; callId: string; seq: number; usage: LlmUsageIr }
  | { type: "completed"; callId: string; seq: number; response: LlmResponseIr }
  | { type: "failed"; callId: string; seq: number; error: LlmApiError }
```

Stream finalizer 必须保证：

- 每个 model call 只有一个 terminal event：`completed` 或 `failed`。
- item 顺序由首次出现顺序确定，不能按并发完成时间重排。
- tool argument 未完成前不能 dispatch。
- 缺失、重复或倒退的 seq 产生明确 stream protocol error。
- 中断流不会伪造 completed response；是否允许 partial output由 LLMNode 策略决定。
- token/reasoning delta 默认只用于短生命周期观察，不逐 token 永久持久化。

## Provider Extensions

Provider 私有扩展按 wire family 分组：

```ts
type ProviderExtensionsIr = {
  openai?: OpenAiExtensions
  claude?: ClaudeExtensions
  gemini?: GeminiExtensions
}

type SafeExtensionValue =
  | null | boolean | number | string
  | SafeExtensionValue[]
  | { [key: string]: SafeExtensionValue }

type OpenAiExtensions = ProviderExtraIr & {
  options?: Record<string, SafeExtensionValue>
}
type ClaudeExtensions = ProviderExtraIr & {
  options?: Record<string, SafeExtensionValue>
}
type GeminiExtensions = ProviderExtraIr & {
  options?: Record<string, SafeExtensionValue>
}

type ProviderExtraIr = {
  extraBody?: Record<string, SafeExtensionValue>
  extraHeaders?: Record<string, string>
}
```

`extraHeaders` 只允许 adapter 明确 allowlist 的非敏感功能 header。以下类别必须拒绝，不能覆盖 adapter 注入值：

```text
authorization / proxy-authorization / cookie / set-cookie
x-api-key / x-goog-api-key
任何名称包含 token、secret、credential、signature 的 header
host / content-length / transfer-encoding
```

credential 只能由 provider client 根据 `SecretRef` just-in-time 注入。`extraBody` 也不能包含 credential、SecretRef 的解析值或任意二进制；adapter 应做字段 allowlist、深度和序列化大小校验。

各 provider `options` 的 key 同样由对应 OperationKey adapter allowlist；`SafeExtensionValue` 只限定可序列化形状，不代表任意 key 都被允许。

当前 `OperationKey` 只读取对应 wire family 的 extension。不匹配 extension、未知保留字段或敏感 header 在请求发送前 reject。

## Metadata、Raw 与脱敏

```ts
type LlmMetadataIr = Record<string, string | number | boolean | null>
```

阶段一限制：

- 最多 32 项，key 最长 128 bytes，整体 JSON 最多 16 KiB。
- metadata 只用于 runtime correlation，默认不转发 provider。
- 禁止 secret、credential、完整 prompt、tool arguments 和任意嵌套对象。
- correlation id 使用随机 opaque id，不编码用户输入。

`rawResponseRef` 只指向受访问控制的 artifact，不内联 raw response。默认不保存；debug 明确启用时先脱敏，单个默认上限 1 MiB，并受 retention/GC 管理。超限时省略 raw capture，不影响语义 response。

日志和普通 event 只记录 ref、digest、大小和安全摘要。请求 header、opaque continuation、raw prompt、provider 原始错误正文不得进入 tracing 字段。`LlmApiError.message` 必须经过 adapter 脱敏和长度限制。

## Shape Adapter 约束

Shape adapter 负责：

```text
LlmRequestIr <-> 当前标准 wire shape
provider stream -> ordered LlmStreamEventIr
provider continuation <-> OpaqueContinuationRef
```

它不负责：

```text
provider-to-provider transform
channel routing
credential 管理
tool authorization / dispatch
memory 或 state mutation
```

当 provider 标准能力不能自然进入 IR 时，依次判断：runtime 是否必须理解、是否只需 adapter-owned sidecar、是否可作为安全 extension、最后才扩展 IR。不能用无界 `unknown` 或 raw response 偷渡核心语义。
