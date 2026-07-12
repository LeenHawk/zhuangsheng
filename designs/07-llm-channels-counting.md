# LLM Channel、模型引用与 Token Count

## Channel Revision

Channel 是外部标准 API endpoint 的版本化配置，不代表真实模型供应商：

```ts
type LlmChannel = {
  id: string
  name: string
  headRevisionId: string | null
}

type LlmChannelRevision = {
  id: string
  channelId: string
  revisionNo: number
  operationTaxonomyVersion: number
  adapterDecoderVersion: number
  baseUrl: string
  credential:
    | { type: "secret"; apiKeyRef: SecretRef }
    | { type: "none" }
  operationKeys: OperationKey[]
  modelCatalogs?: ChannelModelCatalog[]
  capabilities?: ChannelCapability[]
  createdAt: string
}

type ModelCapabilityName =
  | "streaming"
  | "tool_calling"
  | "structured_output"
  | "vision_input"

type ModelCapabilityOverride = {
  feature: ModelCapabilityName
  assumption: "supported"
  reason: string
  acknowledgementRef: string
  policyVersion: number
}

type ChannelModel = {
  id: string
  name?: string
  contextWindow?: number
  maxOutputTokens?: number
  capabilities?: {
    streaming?: boolean
    toolCalling?: boolean
    structuredOutput?: boolean
    visionInput?: boolean
  }
}

type ChannelModelCatalog = {
  operationKey: OperationKey
  policy: "open" | "allowlist"
  models: ChannelModel[]
}

type ChannelCapability =
  | { type: "hosted_tool"; operationKey: OperationKey; hostedKind: string }
  | { type: "tool_based_compact"; operationKey: OperationKey; hostedKind: string }
```

新建 Channel 是 `headRevisionId=null` 的不可运行容器，必须发布并推进首个 revision 后才能被 Graph Apply/NodeInstance 解析。Revision 不可变。更新 base URL、operation taxonomy/adapter decoder、operation、模型清单或 provider-specific capability 创建新 revision；credential rotation 可以更新 SecretRef 指向的加密值，不把 secret 写入 revision。`credential.type=none` 只允许 channel/origin policy 明确允许的无认证 endpoint，adapter 不得为它注入空或伪造 Authorization header。是否需要 Secret Store 来加密 opaque continuation 与 credential 选择无关。

`baseUrl` 默认必须为 HTTPS，不允许 userinfo 或敏感 query。阶段一本地开发可显式允许 loopback HTTP，不能对任意远程 host 关闭校验。

Model capability boolean 缺失表示 `unknown`，不是 false。Graph Apply 对明确 false 的必需能力报错，即使存在 override 也不可放行；unknown 必须由用户通过 node 的 `ModelCapabilityOverride` 显式确认。Apply 根据 node streaming/tool/output/context 推导 required features，要求 override feature 唯一、reason/acknowledgement 非空、policy version 当前有效，再把规范化列表写入 graph revision/content hash。NodeInstance pin 新 channel revision 时重做同样校验：已变为 false 则拒绝，仍为 unknown 才可使用固定 override。Runtime 仍处理 provider typed error，不能靠模型名猜测。

## Node Model Ref

Applied graph 保存逻辑引用：

```ts
type LlmNodeModelRef = {
  channelId: string
  modelId: string
  modelName?: string
  operationKey: OperationKey
}
```

第一次执行 NodeInstance 前解析 channel head，并把实际 `channelRevisionId` 写入 execution snapshot。Waiting、resume 和 retry 复用该 revision；不能在 tool loop 中途切换 endpoint/shape。新的 NodeInstance 默认解析当时最新 revision，显式 reproducible run 可以在 start manifest 中 pin 允许的 revision。

字段含义：

```text
channelId       使用哪个上游 endpoint 配置
modelId         wire request 中的 model 字符串
modelName       可选展示名，不参与协议
operationKey    当前标准 wire shape/operation
```

一个名为 Claude/Gemini 的实际模型可以由 OpenAI-compatible endpoint 暴露。应用只理解该 channel 的 `OperationKey`，不猜测背后真实 provider。

## 调用校验

发起调用前：

1. 读取/pin channel revision；
2. 确认 revision 声明 node `operationKey`；
3. 找到该 operation 的 catalog；`allowlist` 要求 `modelId` 存在，`open` 允许手工 model ID；
4. 校验 node/provider extension 与 operation shape 匹配；
5. 根据 operationKey 选择 ShapeAdapter；
6. provider client 在发送边界按 `credential` 解析 `apiKeyRef` 并注入认证，或对 `none` 明确不注入；
7. 记录 channel revision/model/operation，永不记录 credential。

用户手工添加的模型可以在 open-list channel 使用；provider `/models` 返回值只是 UI discovery，不足以推断 tool、structured output、context window 等全部能力。

## OperationKey

`OperationKey`、`Operation`、`Provider`、`ContentGenerationKind` 来自 `gproxy-protocol`：

```rust
OperationKey::content_generation(
    Operation::GenerateContent,
    ContentGenerationKind::OpenAiResponses,
)

OperationKey::provider(Operation::CountTokens, Provider::Claude)
OperationKey::provider(Operation::CreateEmbedding, Provider::Gemini)
OperationKey::provider(Operation::CreateImage, Provider::OpenAi)
OperationKey::provider(Operation::CompactContent, Provider::OpenAi)
```

这里的 `Provider` 表示 wire protocol family。Channel 不另存真实 provider 或 auth kind；认证 shape 由 provider client adapter 定义。

版本是 operation 语义的一部分：

```ts
type LlmOperationExecutionPin = {
  channelRevisionId: string
  modelId: string
  operationKey: OperationKey
  operationTaxonomyVersion: number
  adapterDecoderVersion: number
}
```

`operationTaxonomyVersion` 固定 `OperationKey` 的 discriminant、canonical serialization 和 endpoint metadata 语义；`adapterDecoderVersion` 固定该 shape 的 request encoder、stream ordering/dedup 和 terminal decoder contract。它们是正整数 compatibility ID，不等于 gproxy crate semver、API `/v1` 或数据库 migration version。

Graph/channel/snapshot reader 必须两阶段解码：先用有界、version-agnostic envelope parser 只读取两个整数 version，再从 support matrix 选择 version-specific `OperationKey` decoder；未知/缺失 version 时不得先把 JSON 反序列化成当前 enum。Wire response/stream 同理，先从 trusted execution pin 选择 exact adapter decoder，再解析 provider bytes。

Channel revision 发布和 Graph Apply 都只能使用进程显式 support matrix 中的版本对。NodeInstance 首次执行要求 graph revision 与选中 channel revision 的两个版本完全一致，并要求 adapter registry 存在 exact `(operationTaxonomyVersion, adapterDecoderVersion, operationKey)`，然后把完整 `LlmOperationExecutionPin` 写入 execution snapshot。缺失、未知或不匹配在创建 provider Effect 之前 fail closed 为 `unsupported_operation_taxonomy`、`unsupported_adapter_decoder` 或 `operation_version_mismatch`；禁止猜当前版本、按 enum 名字符串兜底或静默 transform。

Waiting、retry、stream recovery、model-bound count 和 opaque continuation 解码都复用完整 pin；不绑定 model 的 discovery/list operation 仍必须使用选中 channel revision 的 exact version pair 与 OperationKey，不能猜 current。部署升级必须保留被活跃 snapshot/retention 数据引用的 compatibility reader/decoder，或先运行显式迁移并创建新的 immutable graph/channel revision；不能让旧 run 自动采用新 decoder。

## 模型列表

阶段一可以从 OpenAI/Claude/Gemini 标准 model-list shape 获取候选并让用户确认后保存到新 channel revision。

```text
discovered model      临时 UI 结果
configured model      revision 中的可选 allow/metadata
node modelId          实际执行选择
```

不要因为 model list 暂时不可用而破坏已有 pinned revision。模型 capability metadata 是保守提示；最终请求仍需处理 provider 的 typed error。

## Token Count

核心结果保持简单：

```ts
type TokenCount = number // 当前完整 input request 的 token 数
```

计数顺序：

```text
当前 operation/model 的 provider count API
  -> unsupported/temporary failure: gproxy-tokenize::count
  -> tokenizer fallback/estimate
```

预算对象必须尽量 wire-equivalent，包括 instructions/messages、tool schemas、response schema、multimodal metadata 和 shape 固有开销。Provider count 如果需要 credential，仍由 provider client 注入。

Fallback estimate 不假装精确。Context Assembly 在 fallback 时使用配置的安全余量（阶段一默认可用 input budget 的 5%，至少 256 tokens），并在 trace/preview 中记录 count source；`TokenCount` 类型本身不携带 source。

Output/cache/reasoning token 是调用后的 usage，不混入 input count。不同 shape 的数字不是跨 provider 的严格等价计费单位。

## Count 错误

以下错误不应全部 fallback：

```text
unsupported / endpoint_not_found / transient provider failure
  -> 可以本地 fallback。

invalid assembled request / invalid tool schema / forbidden content
  -> 修正请求，不能用本地计数掩盖。

secret locked / permission denied
  -> 返回 typed error 或显式 wait，不绕过认证。
```

最终 provider 仍可能因自己的 tokenizer/隐藏开销返回 context overflow；LLMNode 按 output/runtime retry policy处理，但不能重复已完成的 side-effect tool。

## Compact

Compact 是显式逻辑能力：

- OpenAI 标准 compact operation 使用对应 OperationKey；
- Claude 若通过标准 hosted/custom tool 表达，必须有显式 `HostedToolBinding/ToolGrant`；
- 配置需要 model/channel、预算、deadline、费用/递归上限和失败 fallback；
- compact result 是新 content/ref，不隐式修改 WorkingContext 或 LongTermMemory。

Context Assembly 阶段一不会在 overflow 时偷偷发起 model compact。调用方应预先提供 summary binding，或通过明确 node/tool 产生 compact result。

## gproxy 依赖边界

```text
zhuangsheng -> gproxy-protocol
zhuangsheng -> gproxy-tokenize（本地 fallback，可选 feature）

zhuangsheng -X-> gproxy proxy/channel/pipeline/transform
```

当前仓库是 `MIT OR Apache-2.0`，样例树中的 `gproxy-tokenize 2.0.8` 是
`AGPL-3.0-or-later`，因此默认构建暂不直接链接它。Runtime 已实现 provider count、
durable CountCall/recovery 和明确标记为 `estimate` 的最后一级 fallback；在引入许可证兼容
且版本固定的 tokenizer（或项目明确采用兼容的 AGPL 分发策略）前，不得把该 estimate
标记为 `local`，也不得声称已经执行 `gproxy-tokenize`。这一限制不改变上面的目标计数顺序。

本项目复用 taxonomy、wire types 和 endpoint metadata，不复制 protocol 目录，不启用 provider-to-provider transform。若未来复用 `gproxy-transform` 的 stream parser/usage aggregation，只能作为当前 native shape 的辅助，不能把应用层变成反代层。

共享 crate 变更先进入独立版本/tag；taxonomy 或 decoder 语义变化提升对应 compatibility ID，升级时通过显式 support matrix、兼容 reader 和 conformance validation，不静默改变历史 run。
