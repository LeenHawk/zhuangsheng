# LLM 渠道、模型引用与计数

## Channel 配置

Channel 负责保存上游连接信息、支持的操作类型和可选模型列表。

Node model reference 负责声明某个节点具体使用哪个 channel、哪个 model，以及哪个标准 operation shape。

不要为模型单独设计复杂配置表。模型在第一版里就是 `modelId`，最多加一个展示名 `modelName`。

```ts
type LlmChannel = {
  id: string
  name: string
  baseUrl: string
  apiKeyRef: string
  operationKeys: OperationKey[]
  generationModels?: ChannelModel[]
  embeddingModels?: ChannelModel[]
  imageModels?: ChannelModel[]
  compactModels?: ChannelModel[]
}
```

```ts
type ChannelModel = {
  id: string
  name?: string
}
```

## 节点模型引用

```ts
type LlmNodeModelRef = {
  channelId: string
  modelId: string
  modelName?: string
  operationKey: OperationKey
}
```

字段含义：

```text
channelId
走哪个上游渠道。

modelId
实际请求里的 model 字符串。

modelName
展示名，可选，不参与协议语义。

operationKey
本次调用使用哪个标准 API shape / operation。
```

Channel 不保存真实 provider。

Claude、Gemini 或其他模型完全可以通过 OpenAI-compatible endpoint 暴露。应用层不关心背后真实 provider，只关心当前 channel 支持的标准 `OperationKey`。

Channel 也不保存 auth kind。认证方式由 `operationKey` / API shape 的 adapter 决定。Channel 只保存 `apiKeyRef`。

## 调用校验

调用时校验流程：

```text
1. 用 channelId 找到 channel
2. 确认 channel.operationKeys 包含 node.operationKey
3. 如果 channel 配置了对应模型列表，确认 modelId 存在
4. 用 operationKey 选择 shape adapter
5. 用 baseUrl + apiKeyRef + modelId 发起标准请求
```

示例 channel：

```json
{
  "id": "gproxy-main",
  "name": "GProxy Main",
  "baseUrl": "https://gproxy.example",
  "apiKeyRef": "secret:gproxy-main",
  "operationKeys": [
    {
      "operation": "generate_content",
      "kind": {
        "content_generation": "openai_responses"
      }
    }
  ],
  "generationModels": [
    {
      "id": "claude-sonnet-4",
      "name": "Claude Sonnet 4"
    }
  ]
}
```

节点引用：

```json
{
  "channelId": "gproxy-main",
  "modelId": "claude-sonnet-4",
  "modelName": "Claude Sonnet 4",
  "operationKey": {
    "operation": "generate_content",
    "kind": {
      "content_generation": "openai_responses"
    }
  }
}
```

这里表达的是：

```text
使用 gproxy-main 这个 channel。
请求里的 model 是 claude-sonnet-4。
本次调用使用 OpenAI Responses 标准 shape。
应用层不关心背后真实 provider。
```

## 模型列表

模型列表支持：

```text
OpenAI models
Claude models
Gemini models
```

模型列表主要用于填充 UI 中的 `generationModels`、`embeddingModels`、`imageModels`、`compactModels`。

应用层不应该只靠 provider 返回的裸模型列表推断所有能力。用户可以手动维护模型列表。

## 计数

计数支持：

```text
OpenAI count
Claude count
Gemini count
```

计数用于：

- prompt 预算检查
- context window 防溢出
- cost 估算
- router decision
- memory/context 裁剪

第一版计数结果保持最小形状：

```ts
type TokenCount = number
```

语义：

```text
TokenCount 表示 input token 数。
```

计数策略：

```text
provider count API
  -> success: 使用 provider 返回的 token 数
  -> fail / unsupported: 使用 gproxy-tokenize::count
```

`gproxy-tokenize::count` 已经内部兜底：

```text
tiktoken / tokenizer
  -> bundled fallback vocab
  -> chars/2 estimate
```

因此本项目的核心计数返回类型不需要带 `source` 或 `exactness`。

如果未来需要 debug，可以在 trace event 中记录计数来源，但不要污染核心 `TokenCount` 类型。

Output token、cache token、reasoning token 等运行后统计应该进入 usage 结构，不应该塞进简单计数结果。

不同 API shape 的计数规则可能不同。`TokenCount` 只作为当前请求 shape 下的预算参考，不作为跨 provider 严格等价值。

## 压缩

压缩是逻辑能力，可以由不同 shape 用不同方式表达。

OpenAI channel 如果支持压缩，可以在 `operationKeys` 中包含：

```rust
OperationKey::provider(Operation::CompactContent, Provider::OpenAi)
```

Claude channel 如果通过标准 tool 形态支持 compact，可以不要求存在同名 compact operation，而是由 Claude shape adapter 把逻辑 compact 请求映射为 compact tool。

这类 tool-based compact 应在 channel 或节点配置中显式启用，避免把普通内容生成误当成压缩能力。

如果 channel 配置了 `compactModels`，调用压缩时可以校验 `modelId` 是否在列表中。

压缩模型列表是可选的。没有配置时，只校验 `operationKeys`。

对于 tool-based compact，没有独立 operation key 时，应校验该 channel 支持对应 generation operation，并且 compact tool 已配置。

## 维护方式

`gproxy-protocol` 应该由 gproxy 和本项目共同依赖，避免复制代码导致漂移。

本地开发阶段可以使用 path dependency：

```toml
gproxy-protocol = { path = "samples/gproxy/crates/gproxy-protocol" }
gproxy-tokenize = { path = "samples/gproxy/crates/gproxy-tokenize", optional = true }
```

跨仓库或发布后可以使用 git/tag dependency。

维护规则：

- 协议类型变更先进入 `gproxy-protocol`
- 本地计数逻辑复用 `gproxy-tokenize`
- gproxy 和本项目都只消费共享 crate
- 不在本项目内复制 protocol 或 tokenize 目录
- 不在本项目内实现 gproxy transform/channel/pipeline

## gproxy-transform

可以把 `gproxy-transform` 作为独立 crate 存在，但它属于反代能力集合。

本项目默认不使用跨 provider transform。

如果未来使用 `gproxy-transform`，只允许使用 provider-native stream parsing、usage extraction、token/text aggregation 等辅助能力。

不得在应用层启用 provider-to-provider request/response transform，除非未来明确改变设计目标。

## 依赖边界

共享 crate 应保持轻量。

理想依赖：

```text
serde
serde_json
http，可选
```

如果 endpoint metadata 需要 `http::Method`，可以考虑后续把 HTTP 相关能力放到 optional feature，避免 core runtime 被 HTTP 类型污染。

本项目 core runtime 可以依赖 protocol taxonomy 和 wire model，但不应该依赖 Axum、Tauri 或 gproxy 主 crate。
