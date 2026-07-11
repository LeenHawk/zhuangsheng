# LLM API 交互总览

## 核心原则

本项目是应用层 agentic runtime，不是 LLM 反代层。

应用层只支持各供应商或上游反代暴露出来的标准 API shape，不重新实现跨供应商协议转换。

```text
应用层负责选择 API shape 和执行 runtime 逻辑。
反代层负责 provider 兼容、协议转换、模型映射和特殊适配。
```

如果用户希望用 OpenAI-compatible endpoint 调 Claude 或 Gemini，应该由 gproxy 或其他上游反代暴露为标准 OpenAI shape。

应用层只看到：

```text
base_url + api_key_ref + operation_key + model_id
```

## gproxy-protocol

本项目复用 `gproxy-protocol` 作为 LLM 标准协议类型来源。

`gproxy-protocol` 是从 gproxy 中拆出的独立 crate，负责维护 provider wire models、operation taxonomy 和 endpoint metadata。

依赖关系：

```text
gproxy
  -> gproxy-protocol

zhuangsheng
  -> gproxy-protocol
```

`gproxy-protocol` 是协议类型的 single source of truth。

本项目不复制 protocol 代码，也不依赖整个 gproxy。

## 复用边界

可以复用 `gproxy-protocol` 中的内容：

- provider taxonomy
- operation taxonomy
- API shape / content generation kind
- OpenAI 标准 wire types
- Claude 标准 wire types
- Gemini 标准 wire types
- endpoint metadata / request target
- serde 请求响应类型

不复用 gproxy 中的反代层能力：

- provider-to-provider transform
- channel routing
- channel auth 特化逻辑
- proxy pipeline
- billing / credential / upstream channel 管理
- 反代专用规则和兼容 hack

## 支持能力分类

第一阶段支持六类能力：

```text
models
模型列表

count
token / usage 计数

generate
内容生成

image
生图

embedding
嵌入

compact
压缩
```

支持矩阵：

```text
models
OpenAI / Claude / Gemini

count
OpenAI / Claude / Gemini

generate
OpenAI Responses / OpenAI Chat Completions / Claude Messages / Gemini GenerateContent

image
OpenAI

embedding
OpenAI / Gemini

compact
OpenAI / Claude tool
```

## API Shape

运行时可以抽象调用入口，但不应该强行把所有 provider 请求体转换成一个自定义大一统结构。

更合适的抽象单位是 API shape。这个概念映射到 `gproxy-protocol` 中的 `OperationKey`、`Provider` 和 `ContentGenerationKind`。

Rust 实现中直接使用：

```rust
use gproxy_protocol::{ContentGenerationKind, Operation, OperationKey, Provider};
```

内容生成 API shape：

```rust
OperationKey::content_generation(
    Operation::GenerateContent,
    ContentGenerationKind::OpenAiResponses,
)
```

provider 级能力：

```rust
OperationKey::provider(Operation::ListModels, Provider::OpenAi)
OperationKey::provider(Operation::CountTokens, Provider::Claude)
OperationKey::provider(Operation::CreateEmbedding, Provider::Gemini)
OperationKey::provider(Operation::CreateImage, Provider::OpenAi)
OperationKey::provider(Operation::CompactContent, Provider::OpenAi)
```

`CompactContent` 是 OpenAI wire operation。Claude 的压缩能力可以通过 Claude tool 形态承载，由 IR/adapter 映射为对应 compact tool，而不是要求 Claude 暴露一个同名 HTTP operation。

六类能力是阶段一 API client boundary；只有 `generate` 直接构成 LLMNode tool loop。Models/count 服务配置和预算，image/embedding/compact 通过显式 service/tool/node 调用，并遵守 grant、effect、ArtifactRef 与 event 规则。

这里的 `Provider` 应理解为 wire protocol family，不代表真实模型供应商。

## Model Capability Metadata

模型 capability 的持久化权威是 `07-llm-channels-counting.md` 的不可变 `LlmChannelRevision.models/capabilities`，不再维护第二套 `ModelSpec`。Discovery 结果只有在用户确认并发布 channel revision 后才参与校验。

Capability metadata 是保守提示；实际请求 shape 始终由节点 `OperationKey` 决定，provider 最终错误仍需 typed handling。

## Client Boundary

LLM API client 是 runtime 的外部依赖边界。

核心 runtime 不应该到处散落 provider SDK 调用。

推荐边界：

```text
LLMNode Executor
  -> ContextAssemblyEngine
  -> LLMNode Request Builder
  -> LlmRequestIr
  -> ShapeAdapter
  -> LlmClient
  -> Provider Client
```

Provider client 的请求/响应类型来自 `gproxy-protocol`。

```text
OpenAI client
  使用 gproxy_protocol::openai::*

Claude client
  使用 gproxy_protocol::claude::*

Gemini client
  使用 gproxy_protocol::gemini::*
```

本项目内部 runtime event、tool abstraction、StatePatch 和 MemoryChangeProposal 不应该反向进入 `gproxy-protocol`。

## 内容生成

内容生成支持四种标准形状：

```text
OpenAI Responses
OpenAI Chat Completions
Claude Messages
Gemini GenerateContent
```

请求体尽量贴近对应标准 API，不做统一大对象。

统一 IR 只作为 LLMNode 语义层，最终仍由 shape adapter 编译成 `gproxy-protocol` wire request。

## 生图与嵌入

生图第一阶段只支持 OpenAI shape：

```text
openai.images
```

嵌入支持：

```text
openai.embeddings
gemini.embeddings
```

Embedding 注意事项：

- 不同 embedding 模型维度不同
- 不同 embedding 模型不能默认混用到同一个 vector index
- vector index 需要记录 embedding model 和 operation key
- embedding 可以由 source content hash 重算，不一定要在所有版本中重复保存

非 generation operation 不进入 `LlmRequestIr`，使用独立 plan/result：

```ts
type ImageOperationPlan = {
  model: LlmNodeModelRef
  promptRef: string
  options: Record<string, SafeExtensionValue>
  maxImages: number
  maxTotalBytes: number
}

type ImageOperationResult = {
  operation: LlmOperationExecutionPin
  artifacts: ArtifactRef[]
  usage?: LlmUsageIr
}

type EmbeddingOperationPlan = {
  model: LlmNodeModelRef
  inputs: Array<{ sourceRef: string; contentHash: string }>
  dimensions?: number
}

type EmbeddingOperationResult = {
  operation: LlmOperationExecutionPin
  vectors: Array<{
    sourceContentHash: string
    dimensions: number
    vectorRef: string
  }>
}
```

Plan 由显式 service/tool/node 构造；创建 provider effect 前必须把逻辑 model ref 解析为 `07-llm-channels-counting.md` 的 exact `LlmOperationExecutionPin`，ShapeAdapter 再使用 `gproxy-protocol` 原生 wire type。Image bytes 先走 artifact staging/validation；embedding vector 是可重算的 private derived object，不进入 prompt/event inline，也不自动写 vector index。Options 仍由对应 adapter schema/allowlist 校验。

## 压缩

压缩是一个逻辑能力，不只等同于某个 HTTP endpoint。

OpenAI 可以使用显式 compact operation：

```text
openai.compact
```

对应 `gproxy-protocol`：

```rust
OperationKey::provider(Operation::CompactContent, Provider::OpenAi)
```

Claude 可以通过工具形态支持压缩：

```text
claude compact tool
```

在这种情况下，显式 compact service/tool/node 发起逻辑请求，Claude shape adapter 把它映射成已授权的 HostedToolBinding；Context Assembly 不会在 overflow 时隐藏调用 compact。

压缩能力可用于：

- 上下文压缩
- 长历史摘要
- trace / tool output compact
- memory 写入前整理

它是显式能力，不应伪装成普通内容生成，也不隐式修改 WorkingContext/LongTermMemory；结果作为有 provenance 的 content/ArtifactRef 供后续 binding 使用。

如果某个 provider 没有 compact endpoint，但有标准 tool 形态的压缩能力，可以在 IR 到 shape adapter 的边界映射为对应 tool。这个映射不是 provider-to-provider 协议转换，而是逻辑 compact 能力到当前标准 API shape 的表达。

```ts
type CompactOperationPlan = {
  model: LlmNodeModelRef
  inputRefs: string[]
  targetTokens: number
  purpose: "context" | "history" | "trace" | "memory_proposal"
}

type CompactOperationResult = {
  contentRef: string
  sourceRefs: string[]
  operation: LlmOperationExecutionPin
  tokenCount: number
}
```

Compact output 带 source refs/provenance，由显式后续 StatePatch/proposal 决定是否使用。失败不覆盖原内容，递归 compact 次数和费用受 hard limit。

## 错误处理

Provider client 应该把底层 HTTP / provider 错误映射为本项目的错误边界。

```ts
type LlmApiError = {
  operationKey: OperationKey
  operationTaxonomyVersion: number
  adapterDecoderVersion: number
  statusCode?: number
  code?: string
  message: string
  retryable: boolean
}
```

错误映射只负责 runtime 决策需要的信息：

- 是否可重试
- 是否限流
- 是否鉴权失败
- 是否请求格式错误
- 是否上下文超限
- 是否 provider 暂时不可用

不要把 provider 原始错误结构泄漏到 core runtime 深处。

## 总结

LLM API 设计遵守：

```text
1. 应用层支持标准 API shape
2. 不做 provider 之间的协议转换
3. 转换、模型映射和兼容交给上游反代
4. OperationKey 表达 wire shape，不表达真实 provider
5. LLMNode 使用统一 IR，provider client 使用 gproxy-protocol wire types
6. streaming 只转换为 runtime event，不转换成其他 provider 协议
```
