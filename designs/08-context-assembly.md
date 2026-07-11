# Context Assembly 设计

## 定位

Context Assembly 是 runtime 的一等模块，由 `LLMNode` 调用，也可以被 preview/test 独立调用。它负责组合多来源上下文，但不拥有 model、tools、credential、provider extension 或最终 wire request。

```text
authorized node input + resolved bindings + pinned preset snapshot
  -> ContextAssemblyEngine
  -> instructions + messages + provenance + budget report
  -> LLMNode Request Builder
  -> model + tools + response format + generation + extensions
  -> LlmRequestIr
  -> ShapeAdapter
```

这样既保留 SillyTavern 类 prompt manager 的 RP 能力，也不允许 preset 或外部内容改变 runtime capability。

## 与 LLMNode 的关系

Canonical `LLMNode` 配置只在 `10-llm-node.md` 定义。Context Assembly 只消费其中的 `context` 引用和调用方已经解析的 bindings。

`LLMNode Request Builder` 是完整 `LlmRequestIr` 的唯一构造者。Context Assembly 的输出不能覆盖 node model、tool descriptors、tool choice、output schema、limits、extensions 或 channel metadata。

## 输入与输出边界

Context Assembly 不查数据库、Secret Store、任意文件或网络。调用方先执行 scope 校验和 binding resolution，再传入不可变数据。

```ts
type ContextAssemblyInput = {
  nodeInput: unknown
  config: ContextConfigSnapshot
  bindings: Record<string, ResolvedContextBinding>
  budget: ContextBudgetInput
}

type ContextAssemblyOutput = {
  instructions: InstructionIr[]
  messages: AssembledMessageIr[]
  provenance: ContextProvenanceIr[]
  budgetReport: ContextBudgetReport
  snapshot: ContextAssemblySnapshot
}
```

```ts
type AssembledMessageIr = {
  id: string
  role: "user" | "assistant"
  content: LlmContentPartIr[]
  provenanceId: string
}
```

`messages` 只是初始 conversation items。runtime 产生的 tool call/result 和 hosted items 由 tool loop 追加为有序 `LlmTurnItemIr`，ContextItem 不得伪造 tool transcript。

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

阶段一中 `mode` 只影响消息编译方式：`chat` 保留消息；`completion` 生成单个 user message；`structured` 提示该 preset 适合结构化任务。真正的 JSON contract 只由 `LlmOutputSpec` / `ResponseFormatIr` 决定，preset 不能开启或替换 schema。

上述 optional 字段只存在于 draft/import DTO。Preset publish 或 graph apply 必须在计算 content hash 前规范化：缺失 spec budget 补为无 `maxInputTokens` 上限且 `strategy="strict"`，缺失 strategy 补 `strict`，缺失 item `order/priority/insertionDepth` 均补 `0`，缺失 item budget 补为无 per-item max 且 `required=false`。对 optional 非 history item，缺失 overflow 补 `{ type: "drop" }`；对 optional history item，缺失 overflow 补 `{ type: "keep_recent" }`；required item 则必须保持无 overflow。

Source 默认也在同一步显式化：`memory.view=summary`，working/state path 为 root JSON Pointer `""`，summary scope 为该 binding 的 authorized root，artifact selector 为 `{view:"metadata", maxBytes:<workspace-safe-default>}`并把实际整数写入 spec。Event selector 缺失 `afterDurableSeq` 补 `0`，缺失 eventTypes 在 pinned semantic policy 下展开为排序后的已授权类型列表。缺失 postProcess 补 `[]`，缺失 preview 补 `{content:"metadata_only", count:"local"}`。规范化后的值写入 immutable preset/NodeInstance snapshot，preview 和 execution 共用同一份，不允许各 adapter 自行选默认值。

## Preset Snapshot 与恢复

Context 可以引用独立版本化 preset，也可以内联在 immutable GraphRevision：

```ts
type ContextAssemblyConfig =
  | { type: "preset"; presetId: string }
  | { type: "inline"; spec: ContextAssemblySpec }

type ContextConfigSnapshot =
  | {
      type: "preset"
      presetId: string
      version: number
      contentHash: string
      spec: ContextAssemblySpec
    }
  | {
      type: "graph_inline"
      graphRevisionId: string
      nodeId: string
      contentHash: string
      spec: ContextAssemblySpec
    }
```

开始新的 NodeInstance 时读取当前 preset revision（inline 则使用 graph 内容），并原子生成 snapshot。Snapshot 一旦生成，在同一 NodeInstance 的全部 model calls、tool loop、retry 和普通 resume 中固定不变；完整 binding versions 由其引用的 canonical read set 记录。

```ts
type ContextAssemblySnapshot = {
  config:
    | { type: "preset"; presetId: string; version: number; contentHash: string }
    | { type: "graph_inline"; graphRevisionId: string; nodeId: string; contentHash: string }
  readSetRef: string
  readSetDigest: string
  resolvedBindingsDigest: string
  assemblyDigest: string
}
```

完整 `ReadSetEntry[]` 的权威仍是 NodeAttempt/read-set store；assembly snapshot 只引用并校验它，不能用一张 `bindingId -> version` map 建第二份版本真相。一个 binding 可对应多个 records/content hashes。

Preset 模式在新的 NodeInstance 开始时读取当前 revision；inline spec 已由 graph content hash 固定。普通 crash/wait resume 必须使用 checkpoint 中的原 snapshot。用户从历史执行点发起新的执行时可以选择：

```text
restore
新 GraphRun 显式 pin 历史 snapshot，便于复现实验。

latest
新 GraphRun/尚未开始的 NodeInstance 按默认解析策略读取当前 revision。
```

`latest` 不是旧 NodeInstance 的 resume，不能与旧 provider opaque continuation 混用。Preset 内容变化不产生 graph revision，节点改为引用另一个 preset 才产生 graph revision。

## ContextItem

```ts
type ContextItem = {
  id: string
  name?: string
  enabled: boolean
  requestedRole: "policy" | "system" | "developer" | "context" | "user" | "assistant"
  source: ContextSource
  position: ContextPosition
  order?: number
  priority?: number
  insertionDepth?: number
  budget?: TokenBudgetHint
  overflow?: OverflowPolicy
}
```

`requestedRole` 是请求，不是授权。resolver 产生的 `allowedRoles` 和固定 trust policy 决定最终角色。`priority` 只用于 token 预算，不影响 instruction authority。

`order` 控制同一插入锚点的显式顺序；原数组位置是稳定 fallback。

## Binding、来源与 scope

```ts
type ContextSource =
  | { type: "literal"; text: string }
  | {
      type: "template"
      syntax: "zhuangsheng_template_v1"
      template: string
      variables: Record<string, TemplateVariableSource>
      onMissing: "error" | "empty"
    }
  | { type: "input"; path: string }
  | { type: "memory"; bindingId: string; view?: "summary" | "items" | "raw" }
  | { type: "working_memory"; bindingId: string; path?: string }
  | { type: "state"; bindingId: string; path?: string }
  | { type: "history"; bindingId: string; strategy: HistoryStrategy }
  | { type: "world_info"; bindingId: string; selector: WorldInfoSelector }
  | { type: "summary"; bindingId: string; scope?: string }
  | { type: "tool_trace"; bindingId: string; selector: ToolTraceSelector }
  | { type: "event_trace"; bindingId: string; selector: EventTraceSelector }
  | { type: "artifact"; bindingId: string; selector?: ArtifactSelector }
  | { type: "branch_context"; bindingId: string }
```

阶段一辅助类型保持确定、有界：

```ts
type HistoryStrategy =
  | { type: "all" }
  | { type: "recent"; count: number }

type WorldInfoSelector =
  | { type: "all" }
  | { type: "tags"; tags: string[]; match: "any" | "all" }

type ToolTraceSelector = { terminalOnly: boolean; maxCalls: number }
type EventTraceSelector = { eventTypes?: string[]; afterDurableSeq?: number; limit: number }
type ArtifactSelector = { view: "text" | "metadata"; maxBytes: number }

type TemplateVariableSource =
  | { type: "literal"; value: JsonValue }
  | { type: "input"; selector: InputSelector }
  | { type: "binding"; bindingId: string; selector: InputSelector }

type PreviewPolicy = {
  content: "metadata_only" | "authorized"
  count: "local" | "remote_explicit"
}
```

所有 count/limit/array/bytes 字段在 graph/preset validation 时受 workspace hard bounds；selector 只过滤已授权 binding，不扩大 scope。

除 literal、template 和 node input 外，所有来源都必须引用已解析 binding。`artifactRef`、memory path 或 trace selector 不能直接绕过 `MemoryBinding` / `ArtifactGrant`。

```ts
type ResolvedContextBinding = {
  bindingId: string
  scope: string
  version: string
  values: ResolvedContextValue[]
  templateValue?: JsonValue
}

type ResolvedContextValue =
  | {
      kind: "data"
      id: string
      contentHash: string
      content: LlmContentPartIr[]
      provenance: ContextProvenance
      allowedRoles: ContextRole[]
      relevanceScoreMicros?: number
    }
  | {
      kind: "history_message"
      messageId: string
      turnId: string
      stableOrder: number
      role: "user" | "assistant"
      contentHash: string
      content: LlmContentPartIr[]
      provenance: ContextProvenance
    }
```

binding resolver 负责 scope、branch、version、artifact lifecycle 和 caller permission；assembly engine 只消费结果。

`history` source 只接受 `history_message` values，按 active branch ancestry 的 `stableOrder` 组装为独立 AssembledMessageIr，并保留已验证 role/message identity；不得使用 ContextItem.requestedRole覆盖整段历史。非 history source 只接受 `data` values。类型不匹配是 assembly contract error，不能从 source ID 或文本猜角色。

`top_k` 只允许用于 `data` values，并且只使用 resolver 提供的整数 `relevanceScoreMicros`（固定范围，降序；tie 按原 binding 顺序再 id），缺失 score 时 validation error。History 只能使用 `HistoryStrategy.all/recent` 选择范围，并且 overflow 只能是 `keep_recent`（或 required item 不配 overflow）；`keep_recent.count` 缺失时按剩余 budget 保留可容纳的最长最新后缀。`dedupe` 按 contentHash 保留首次出现。Assembly 不自行调用 embedding/search，也不使用平台浮点排序。

Template 阶段一只支持 `zhuangsheng_template_v1`：UTF-8 文本中只有 `{{identifier}}` 是 placeholder，identifier 必须匹配 `[A-Za-z_][A-Za-z0-9_]{0,63}`，`\{{` 表示字面量 `{{`，未闭合 delimiter 是 validation error。不支持 section、function、filter、隐式 path 或 Mustache/JS 其他语法。Publish/Apply 把 source 解析为版本化 AST，要求 placeholder 集合与 `variables` 的 canonical key 集合完全相同，并把 syntax version/AST/变量来源加入 content hash；第三方宏只能先显式转换或作 disabled compatibility metadata 保留。

变量按 key 的 Unicode code point 顺序解析一次，placeholder 的出现顺序只决定插入位置。`input` selector 只读 `ContextAssemblyInput.nodeInput`；`binding` selector 只读该已授权 `ResolvedContextBinding.templateValue`，resolver 未产生该值视为 missing，不从 `values[].content` 猜测文本。Selector 使用 `11-graph-definition.md` 的 canonical InputSelector 语义；来源的 binding/version/selector/result digest 进入 assembly snapshot/read set/provenance。不允许引用 node output、SecretRef/SecretValue、未授权 binding 或另一 ContextItem。

已解析的 string 值原样插入；null/boolean/number/array/object 用 `06-persistent-versioning.md` 的 `canonical_json_v1` UTF-8 序列化后插入；不能使用会把 arbitrary-precision number 收窄为 binary64 的 RFC 8785/JCS。binary、SecretValue 和未解析 ArtifactRef 禁止插值。运行时缺失变量严格按 `onMissing` 报错或替换为空，并在 report 中记录。旧 draft/import 缺少 syntax/variables/onMissing 时必须在 publish 前显式迁移；安全默认是 v1、空 variable map 和 `error`，不能留给 runtime 猜测。它不能读取环境变量、文件、网络或调用函数。

Template 的 provenance 使用 taint 合成：trust 取 literal/config 与所有插值来源中最不可信者，sensitivity 取最高者，并记录每个 variable source。任何 user/memory/tool/artifact/external 值插入后，item 都不能保持 `policy/system/developer`；阶段一把整个 item 降级为 `context`（current user template 可降为 `user`）。需要 trusted instruction + untrusted data 时必须拆成两个 ContextItem，用明确 data block引用，不能靠字符串转义假装提升信任。

## Provenance、Trust 与 Sensitivity

```ts
type ContextProvenance = {
  sourceType: string
  sourceId: string
  trust: "runtime_policy" | "trusted_config" | "user_input" | "external_untrusted"
  sensitivity: "public" | "private" | "sensitive"
}

type ContextRole = "policy" | "system" | "developer" | "context" | "user" | "assistant"

type ContextProvenanceIr = ContextProvenance & {
  id: string
  itemId: string
  finalRole: ContextRole
  transformations: string[]
}
```

固定权限规则：

```text
runtime_policy
  -> policy

trusted project/node/preset config
  -> system / developer / context

current user input
  -> user

validated conversation assistant message
  -> assistant

memory / world info / artifact / trace / external import
  -> context data，不得成为 policy/system/developer/tool
```

用户显式信任导入的 preset 后，它可以成为 `trusted_config`，但仍不能覆盖 runtime safety policy、扩大 tool/memory grant 或解析 secret。外部 character card、lorebook、网页和 tool output 默认是 `external_untrusted`。

`sensitive` 内容只有在 node grant 明确允许且目标 channel policy 允许外发时才能进入上下文。Secret Store 的解析值在任何情况下都没有 Context role，必须 fail closed。

## 插入位置与确定性顺序

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

确定性排序步骤：

1. 按上述 enum 顺序得到 position rank。
2. `insertionDepth` 相对已解析 history 尾部计算锚点；越界按 spec validation error 处理。
3. 同锚点按 `order` 升序，未配置视为 `0`。
4. 再按 preset 原数组 index，最后按 item id 打破异常重复顺序。

`priority` 不参与展示排序。`assistant_prefill` 只允许 trusted config，并在 shape 不支持时明确 reject 或按用户选择降级为普通 assistant message，不能静默改变角色。

## 预算输入

```ts
type ContextBudgetInput = {
  contextWindowTokens: number
  reservedOutputTokens: number
  fixedRequestTokens: number
  safetyMarginTokens: number
}

type ContextBudgetPolicy = {
  maxInputTokens?: number
  strategy?: "strict" | "best_effort"
}

type TokenBudgetHint = {
  maxTokens?: number
  required?: boolean
}
```

`fixedRequestTokens` 由 Request Builder 预估，覆盖 tool descriptors、response schema 和 wire envelope。`safetyMarginTokens` 在 fallback tokenizer 或 shape 开销不精确时保留。

`required` 用于 runtime policy、当前用户输入和必要 schema hint。Required item 不允许配置任何会改变/丢弃内容的 overflow（drop、truncate、top_k、dedupe、keep_recent），并以完整内容计数；required 总量超限时直接返回 `ContextBudgetExceeded`。Runtime policy 还禁止引用不可信 template variables。

## 阶段一 OverflowPolicy

```ts
type OverflowPolicy =
  | { type: "drop" }
  | { type: "truncate_head" }
  | { type: "truncate_tail" }
  | { type: "keep_recent"; count?: number }
  | { type: "top_k"; k: number }
  | { type: "dedupe" }
```

阶段一不在 Context Assembly 内隐藏调用 LLM、compact endpoint 或 tool 做 `summarize/compact_trace`。需要语义压缩时使用显式 graph node/tool，生成有版本和 provenance 的 summary binding，再供 assembly 读取。

`truncate_head/tail` 只裁剪 text part，并在 tokenizer/Unicode scalar 安全边界重新计数；image/file part 不做字节截断，只能由 optional item整体 drop或返回 strict error。所有 transformation 写入 provenance/report。

导入 preset 中的 summarize/compact 规则可以保留为 disabled compatibility metadata，但 preview 必须显示“阶段一未执行”，不能静默替换成普通截断。

## 确定性预算算法

1. 校验 preset、binding scope、role、sensitivity 和 required/overflow 组合。
2. 解析 source，生成有 provenance 的 item；执行确定性 dedupe/top-k/keep-recent。
3. 按插入规则构造 candidate instructions/messages。
4. 先为 required items 和 fixed request overhead 计数；超限立即失败。
5. optional items 按 `priority` 降序分配，tie 按最终展示顺序；执行各自 maxTokens/overflow。
6. optional item 处理后仍不适配时，`best_effort` 丢弃并记录，`strict` 返回错误。
7. Request Builder 加入 tools/schema 后对完整 request 再计数。若仍超限，只能按同一 report 继续删除 optional item；不能截断 required item。
8. provider context-too-large 可触发一次重新计数/裁剪，不得形成无界重试。

Context Assembly 自身不调用 count endpoint。外层 `LlmCountingService` 先用本地 counter生成 candidate；Request Builder 完整组装后，它可经 ProviderClient执行当前 shape 的 count API（作为 pure external effect/audit，处理 credential/wait），再把 count feedback 交回纯 budget/trim 函数。Provider count 失败时使用 `gproxy-tokenize::count` 并增加 safety margin。核心 `TokenCount` 仍是 number；来源和估算状态只进入 budget report。

Run 内 provider count 是 NodeInstance-owned 的独立 logical `CountCallRecord`，不占 modelCallNo/maxModelCalls；它按 countOrdinal 持久 exact count execution pin/digest、canonical trim candidate ref/digest、wire-equivalent request digest、result/token count 和 pure Effect/EffectAttempt，并由 invoking NodeAttempt fence。同一 countCallId 的 crash/retry 复用 durable result 或在同 logical pure effect 下新建 EffectAttempt，不重新取“当前计数”后静默改 prompt。只有新的 trim candidate 才新建下一 ordinal，并受 LLMNode `maxCountCalls` 限制。Run 外 preview/discovery 没有 NodeInstance，使用 application receipt/effect boundary，不伪造 runtime CountCall。

```ts
type CountExecutionPin = {
  operation: LlmOperationExecutionPin
  localCounterId: string
  localCounterVersion: number
  fallbackPolicyVersion: number
  safetyMarginTokens: number
}

type CountCallRecord = {
  id: string
  nodeInstanceId: string
  originatingAttemptId: string
  countOrdinal: number
  execution: CountExecutionPin
  executionPinDigest: string
  trimCandidateRef: string
  trimCandidateDigest: string
  requestDigest: string
  status: "prepared" | "running" | "completed" | "failed" | "retry_ready" | "cancelled_before_start" | "abandoned_unknown"
  resultSource?: "provider" | "local" | "estimate"
  resultRef?: string
}
```

NodeInstance 首次构建 counting service 时把 `CountExecutionPin` 写入 execution snapshot；provider 失败后的 local result 必须使用其 exact counter/version、fallback policy 和已规范化 safety margin，并把 source/result 持久到 CountCall/checkpoint。Logical row/effect/prepared attempt/checkpoint/首次预算扣减原子创建，后续状态转换也原子更新 ledger/checkpoint/journal；恢复时缺少或不匹配 counter/policy/pin/candidate/request digest 则 fail closed 为 compatibility error，不用升级后的当前 tokenizer 重算旧 candidate或重复扣预算。

```ts
type ContextBudgetReport = {
  availableInputTokens: number
  fixedRequestTokens: number
  assembledTokens: number
  countSource: "provider" | "local" | "estimate"
  items: ContextBudgetItemReport[]
}

type ContextBudgetItemReport = {
  itemId: string
  included: boolean
  tokenCount: number
  action: "kept" | "dropped" | "truncated" | "deduped" | "unsupported"
  reason?: string
}
```

## Post Process 安全边界

```ts
type PromptPostProcessRule =
  | { type: "merge_adjacent_messages" }
  | { type: "strict_alternation" }
  | { type: "single_prompt" }
  | { type: "strip_empty_messages" }
```

Post process 只能改变初始 prompt 的可表示形状，不能提升 role、跨 provenance 合并 sensitive 与 public 内容、制造 tool result、改变 tool order，或把 runtime policy 并入不可信 user text。

`merge_adjacent_messages` 仅合并 role、trust、sensitivity 完全相同的相邻 message。`strict_alternation` 只能插入标记为空的 adapter placeholder，不能伪造用户/assistant 语义。

## Preview

Preview 与真实执行使用同一 validation、scope 和预算算法，但必须保持只读：

- 不执行工具、memory write、summary、compact 或 provider generation。
- 不解析 SecretRef，不读取未授权 binding。
- 默认只返回 item 元数据、token 数、action 和 digest，不返回完整内容。
- 调用者有 `context.preview_content` 权限时才返回内容；sensitive part 仍按 policy 遮罩。
- preview 默认不进入 event log；trace 只记录 snapshot digest 和 budget totals。
- Preview 调用方若选择 provider count，由 provider client 明确显示目标 channel并处理 credential；assembly engine 仍只接收 count feedback。独立 preview 没有 GraphRun/NodeInstance，locked 时返回 typed application error，解锁后重试，不创建 runtime WaitRecord。用户可以选择 local-only preview。

## SillyTavern / RP 兼容边界

导入层可以映射：

- system prompt、instruct/context/reasoning template；
- character description、personality、scenario、user persona；
- example dialogue、chat history、summary；
- world info/lorebook、author note；
- start reply with / assistant prefill；
- jailbreak、nudge 和 prompt processing mode。

这些内容转换为本项目的 `ContextItem[]`，不会直接保留第三方执行逻辑。导入时：

- 未知宏、脚本、网络加载和任意代码保持 disabled 并报告。
- character card、lorebook 等外部数据默认 `external_untrusted`。
- jailbreak 只是用户 prompt 内容，不能覆盖 runtime policy 或 capability grant。
- 历史中的伪造 system/tool role 降级为 context data。
- 不承诺逐字节复现 SillyTavern 的 provider-specific 历史 hack。

阶段一保留 RP 装配表达力，但优先保证相同 snapshot、binding 和 tokenizer 输入得到确定结果。
