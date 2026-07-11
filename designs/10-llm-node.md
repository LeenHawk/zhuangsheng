# LLMNode 设计

## 定位

`LLMNode` 表示一次 LLM 驱动的语义阶段。一个 NodeInstance 可以包含 Context Assembly、多次 logical model call、custom/hosted tool、approval/wait、流式聚合和最终输出校验。

图只接收 finalized `NodeResult.outputs`。内部 transcript、usage、reasoning、tool/effect 和 delta 进入 checkpoint/event/trace，不自动成为 graph output。

## 正式配置

公共 port、timeout/retry 等 BaseNode 字段见 `11-graph-definition.md`：

```ts
type LLMNode = BaseNode & {
  kind: "llm"
  model: LlmNodeModelRef
  capabilityOverrides?: ModelCapabilityOverride[]
  context: ContextAssemblyConfig
  memory?: MemoryBinding
  tools?: ToolGrant[]
  hostedTools?: HostedToolBinding[]
  request?: LlmRequestOptions
  output?: LlmOutputSpec
  streaming?: LlmNodeStreaming
  limits?: Partial<LlmNodeLimits>
}
```

Graph revision 保存逻辑配置；第一次执行 NodeInstance 时解析并 pin 实际 preset/channel/registry/semantic-policy revision。`capabilityOverrides` 只能确认 catalog 中 unknown 的必需能力，不能把 explicit false 变为 true；canonical 类型和 Apply/pin 规则见 `07-llm-channels-counting.md`。每次 dispatch/approval/commit 还叠加只能收窄的 live revocation overlay，不能用它向旧 snapshot 新增 capability。

## Model Ref

`LlmNodeModelRef` 的 canonical 类型见 `07-llm-channels-counting.md`。其中 `OperationKey` 来自 `gproxy-protocol`，表达当前标准 wire shape，不表达真实 provider。

阶段一 generation 支持：OpenAI Responses、OpenAI Chat Completions、Claude Messages、Gemini GenerateContent。Apply graph 时验证 operation 是 content generation 且与 node features 可兼容；实际 adapter 在调用前再次验证。

## Request Options

```ts
type LlmRequestOptions = {
  generation?: GenerationOptionsIr
  extensions?: ProviderExtensionsIr
  toolChoice?: ToolChoiceIr
}
```

`model`、tools、response format 和 metadata 由 Request Builder 根据 node/config/snapshot生成，不能由 preset template 覆盖。Provider extension 只对当前 wire family生效，敏感 headers/fields 被拒绝。

`generation.maxOutputTokens` 是偏好，`limits.maxOutputTokens` 是硬上限，实际取较小值。

## Context 与 Read Snapshot

```text
激活 NodeInstance
-> 固定 WorkingContext/LongTermMemory read set
-> 解析授权 MemoryBinding/Artifact binding
-> pin ContextConfigSnapshot
-> ContextAssemblyOutput
-> Request Builder
-> LlmRequestIr
```

Context Assembly 只产 instructions、initial messages、provenance、budget report 和 snapshot，不查数据库/secret，不添加 tool/model。详细 trust、role、预算和 preview 规则见 `08-context-assembly.md`。

同一 activation 的全部 model/tool 调用使用固定 static/context read snapshot。工具返回的 StatePatch 暂存到 finalized transition，后续 tool 不从未提交的新 head读取。模型之后发起的内建 `search_memory` 是显式 call-level read；每个 logical call 持久 query/scope token/ordered result 并在恢复时复用，不会反向改写 NodeInstance 的 static snapshot。

## Tools

`ToolGrant` 引用 registry 中固定 `toolId + version`；`HostedToolBinding` 显式声明 provider 托管能力。修改 tool 版本、scope、risk 或 exposed name 产生 graph revision。

节点执行时 pin `ToolRegistrySnapshot`，只向模型暴露有效权限交集内的 descriptor。模型请求未授予 name 时不能从全局 registry 动态命中。

Custom tool executor 不拥有数据库、SecretResolver 或全局 Memory store。唯一 output parts、approval、副作用、并发、Artifact 和 GC 见 `19-tools-artifacts.md`。

## Tool Loop

```text
ModelCall #1 -> ordered assistant/tool items
-> validate/grant/approval
-> execute tool batch + persist effects/results
-> append tool results by callIndex
-> ModelCall #2
-> final assistant items
```

每个 model terminal、tool state、wait 和 batch 边界更新 `LlmLoopCheckpoint`。Waiting/crash 恢复复用 transcript、opaque continuation 和已完成 effect result，不从头重跑工具。

完整状态机、JSON repair 和 model/tool retry 边界见 `07-llm-tool-loop.md`。

## Output Contract

```ts
type LlmOutputSpec =
  | {
      mode: "text"
      finalText?: "last_assistant_turn" | "all_assistant_text"
      allowEmpty?: boolean
    }
  | {
      mode: "json"
      schema: JsonSchemaSpec
      strict?: boolean
    }
```

`JsonSchemaSpec` 的唯一 dialect、keyword、format、number、resource limits 和 compiled payload 契约见 `16-domain-consistency.md`；LLM provider 的 structured-output subset 只能进一步收窄，最终本地校验不能换成 provider 自称的成功。

默认：

```text
mode = text
finalText = last_assistant_turn
allowEmpty = false
```

Text output 是 `outputs.default` 的裸字符串，不包装为 `{text}`。`last_assistant_turn` 只选最后收敛 logical ModelCall 的 assistant message items，`all_assistant_text` 选当前 durable transcript 内全部 logical ModelCall 的 assistant message items；两者均按 modelCall/item/content 顺序把 text part 的 UTF-8 字符串原样直接连接，不插入分隔符/换行/修剪空白。Tool call/result、reasoning、hosted item 和 image/file part 不贡献 text；没有 text 时按 `allowEmpty` 处理。

JSON output 只从最后收敛 logical ModelCall 的 assistant message items 提取：按 item/content 顺序原样连接全部 text part（无分隔符），要求至少一个 text part，且所选 message 不得含 image/file 等非 text semantic part。然后使用 `canonical_json_v1` 的 exact parser 要求恰好一个完整 JSON value：只允许值前后 JSON whitespace，拒绝 BOM、trailing non-whitespace、duplicate object key、非法 Unicode 和超出 number/resource limit 的值，再用 compiled `JsonSchemaSpec` 验证。产物是完整 JsonValue，LLMNode 不为 JSON 字段派生 output port；下游使用自己的 consumer selector。

JSON 流式 delta 不作为 graph output。最后收敛轮完成后按上述唯一 extraction/parser/schema 流水线处理；失败时 repair 记录该 extracted bytes digest 和结构化错误，并在同一 durable transcript 上做有限 repair ModelCall，不能重跑已完成工具。耗尽后 node failed。

空字符串需 `allowEmpty=true`；`null/false/0/[]/{}` 是存在的 JSON value，是否合法由 schema 决定，runtime 不能用 truthiness 判断 emission。

阶段一 LLMNode 只有 `default` finalized output port。Usage、finish reason、model/tool call 数和 raw response ref 属于 execution metadata。

## Streaming

```ts
type LlmNodeStreaming = {
  enabled: boolean
  audience: "user" | "trace" | "both" | "internal"
  persistChunks?: boolean
}
```

Streaming 只影响观察层和 finalizer，edge/Router 等待 finalized output。默认 token/reasoning/tool-argument delta ephemeral；`persistChunks` 显式启用时按有界 chunk 持久化，不逐 token 写数据库。

每个 model call 恰有一个 durable terminal。Client 断线可能丢 delta，但可通过 terminal/result ref 恢复最终输出。

`audience=user` 仍受 content/policy filter；reasoning 默认不面向用户，provider private reasoning 只保存必要 opaque continuation。

## Limits

```ts
type LlmNodeLimits = {
  maxModelCalls: number
  maxCountCalls: number
  maxToolCalls: number
  maxOutputRepairs: number
  maxConcurrentTools: number
  maxInputTokens: number
  maxOutputTokens: number
}
```

阶段一默认建议（workspace policy 可收紧，不能超过服务硬上限）：

```text
maxModelCalls       8
maxCountCalls       2
maxToolCalls        32
maxOutputRepairs    1
maxConcurrentTools  4
```

Input/output token 默认值来自 model/channel spec；缺失时 graph apply 要求用户显式配置或采用服务安全上限。

Logical model/count call 分别进入 prepared 时计入各自上限，transport retry 不重复；count 不占 model 额度。完整 tool call 出现即计数，包括 invalid/denied。达到 limit 默认 fail，或由明确 policy 创建人工 wait，不能依赖模型自行停止。

继承的 `BaseNode.timeoutMs` 限制一次 NodeAttempt 的活跃执行；waiting 会结束当前 attempt，resume attempt 有新的 execution deadline，而 run wall-clock deadline 始终包含 waiting。Tool/provider 自身 timeout 可以更短；完整规则见 `17-runtime-control.md`。

## Retry 分层

```text
provider transport retry
同一个 logical ModelCall/effect attempt policy；不重建 NodeInstance。

tool retry
由 ToolEffect classification/idempotency 决定。

JSON output repair
同一 transcript 上的新 logical ModelCall。

NodeAttempt retry
通用 runtime RetryPolicy；只有整个 executor 声明可安全恢复/重执行时使用。
```

LLMNode 已开始 non-idempotent effect 后，不能通过 node retry 从 Context Assembly 开头执行。`outcome_unknown` 必须等待人工/协调器。

## Waiting

可能等待：

- Secret Store 解锁（model request 尚未发送）；
- tool/hosted capability approval；
- MemoryChangeProposal confirmation/review；
- non-idempotent effect outcome reconciliation；
- executor 明确声明的外部 callback。

返回 waiting 前必须持久化 `LlmLoopCheckpoint`、WaitRecord、response schema、effect watermark、deadline 和 current counters。Resume 继续同一 logical activation；普通用户输入没有 wait ID 时创建新 run。

## Finalized Transition

LLM executor 返回 `03-async-runtime.md` 的 canonical `NodeResult`：completed 带 `outputs/transition`，waiting 带 `wait/continuation/transition`，failed 带结构化 `NodeError`。Storage 会先把 continuation 和 transition values 预写为 ValueRef。

`NodeTransition` 可以包含待重新校验的 StatePatch、MemoryChangeProposal refs 和 ArtifactRefs。Storage 在检查 run epoch、attempt fencing、context head/read set 后，与 attempt completion、edge emission、run output和 durable events 同事务提交。

如果 CAS/conflict 失败，不能把已经 streaming 给 UI 的内容当作 committed output。UI 必须等待 node/run terminal 判断最终状态。

## Execution Flow

```text
1. claim NodeAttempt + fencing token
2. load/pin execution snapshot
3. resolve deterministic bindings/read set
4. assemble context + full request budget
5. prepare/send/finalize model call
6. execute/recover tool batches until converged
7. derive text/JSON output
8. validate/repair within limits
9. stage transition parts
10. atomic commit or return durable waiting/failed
```

## 阶段一与延后

阶段一实现上述四种 generation shape、text/JSON output、custom/hosted grants、有限并发 tool loop、checkpoint、streaming、approval 和 artifact refs。

延后：跨 provider transcript 转换、动态 tool registry、arbitrary multimodal output port、完整 provider private reasoning 展示、隐藏式 Context summarize、任意 JSON auto-repair agent 和 node-level 多 activation 并发。
