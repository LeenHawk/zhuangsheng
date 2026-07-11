# Tools、Effect Ledger 与 Artifacts

## 定位

本文定义 `LLMNode` 内 custom/hosted tools 的能力、授权、执行、恢复和 artifact 生命周期。

领域术语以 `16-domain-consistency.md` 为准：

- tool call/effect/checkpoint 属于 `ExecutionState`；
- 确定性 WorkingContext 变更使用 `StatePatch`；
- 长期记忆语义变更使用 `MemoryChangeProposal`；
- 大内容保存为不可变 `ArtifactObject`，业务对象只持有 `ArtifactRef`；
- Secret 不是 Artifact，阶段一 custom tool 不能访问 Secret Store。

工具不是数据库插件。executor 只能通过有界 capability port 读取已绑定输入、创建 staged artifact 或返回变更提案，不能持有 SeaORM connection、Memory store、SecretResolver 或全局 service locator。

## 四个工具类型

### ToolDescriptor

`ToolDescriptor` 是 registry 中的稳定能力声明；只有其中的 model-facing 子集进入 LLM IR。

```ts
type ToolDescriptor = {
  toolId: string
  version: string
  name: string
  description?: string
  inputSchema: JsonSchemaSpec
  bindingConfigSchema?: JsonSchemaSpec
  effect: ToolEffectSpec
  supportsParallel: boolean
  requiredScopes: ToolScopeRequirement[]
  limits: ToolLimits
}
type ToolEffectSpec = {
  classification: "pure" | "idempotent" | "non_idempotent"
  operationKey: string
  requiresApproval: boolean
}
type ToolLimits = {
  timeoutMs: number
  maxInputBytes: number
  maxLlmResultBytes: number
  maxArtifactBytes: number
}
type ToolScopeRequirement = {
  kind: "memory_read" | "memory_proposal" | "state_patch" | "artifact_read" | "artifact_write" | "network" | "local_network"
  scope: string
}
type ToolScopeGrant = ToolScopeRequirement & {
  paths?: string[]
  origins?: string[]
}

type ArtifactGrant = {
  readScopes: string[]
  writeScopes: string[]
  allowedMediaTypes: string[]
  maxObjects: number
  maxBytes: number
}
```

`name` 在当前 LLM request 内唯一且符合 provider 命名限制；runtime 使用 `toolId + version` 解析，不能只按模型返回的 name 搜索全局 registry。

descriptor 的 effect classification 是实现承诺。graph/node 不能把 `non_idempotent` 降级成 `idempotent`，也不能关闭 descriptor 强制的 approval。

### ToolGrant

`ToolGrant` 是 graph revision 给节点的 capability，不包含 executor：

```ts
type ToolGrant = {
  bindingId: string
  toolId: string
  version: string
  exposedName?: string
  scopes: ToolScopeGrant[]
  artifact: ArtifactGrant
  constraints?: Record<string, string | number | boolean>
  approval?: "descriptor_default" | "always"
  failurePolicy?: ToolFailurePolicy
}
```

Grant 使用固定版本，不使用运行时自动漂移的 `latest`。修改 grant、版本、scope 或 exposed name 都产生新的 graph revision。

### RegisteredTool

```ts
type RegisteredTool = {
  descriptor: ToolDescriptor
  schemaCompilations: JsonSchemaCompilation[]
  implementationDigest: string
  executorKey: string
  enabled: boolean
}

type ToolRegistrySnapshot = {
  revision: string
  entries: Array<{
    toolId: string
    version: string
    descriptorDigest: string
    schemaCompilationDigests: string[]
    implementationDigest: string
  }>
}
```

Tool publish 使用 `16-domain-consistency.md` 的 canonical schema compiler；descriptor digest 覆盖 schema hashes/profile/limits，registry snapshot 还固定 compiled payload digests。NodeInstance 激活时 pin registry snapshot。恢复时 schema/compiler 或 implementation digest 不匹配必须等待显式兼容迁移/人工确认，不能忽略 schema 或用新二进制静默重跑旧 effect。

`RegisteredTool.enabled=false` 只阻止新的 execution snapshot 选择该 entry；它不改写已有 snapshot。需要紧急阻止已运行/等待的实例时，必须同时发布 deny-only live revocation，不能让一个可变 boolean暗中改变恢复语义。

### HostedToolBinding

```ts
type HostedToolBinding = {
  bindingId: string
  operationKey: OperationKey
  hostedKind: string
  modelFacingConfig: Record<string, string | number | boolean>
  resourceScopes: string[]
  effect: ToolEffectSpec
  maxUsesPerModelCall: number
}
```

Hosted tool 由 provider 在 model call 内执行，不进入本地 executor，但必须在发请求前显式 grant、校验 scope、费用和 approval，并进入 model-call effect 记录。

provider 内部无法逐次暂停审批时，阶段一只允许整个 hosted capability envelope 预先批准。需要逐调用高风险确认的 code execution、购买、发布等能力必须改为 custom tool 或独立 graph node。

包含 hosted tool 的 model call，其 effect classification 取所有 hosted bindings 的最高风险。存在 `non_idempotent` hosted capability 时，model request 进入 started 后不能因断线自动重试。

## 有效权限是交集

一次 custom/hosted call 的有效权限为：

```text
workspace/run policy
∩ graph revision grant
∩ node ToolGrant / HostedToolBinding
∩ ToolDescriptor.requiredScopes
∩ MemoryBinding / ArtifactGrant
∩ 当前 actor permission
∩ 当前 branch/read set
```

任一层未声明即 deny。Grant 只能收窄 descriptor requirement，不能扩大；模型 arguments 也不能选择 grant 外的 aggregate id、path、URL、artifact 或 actor。

Network grant 必须有非空 canonical origin allowlist（scheme/host/effective port），默认拒绝 loopback、private、link-local、multicast 和 cloud metadata ranges。DNS 解析后与每次 redirect 都重新校验目标 IP/origin；redirect 不继承 credential。访问局域网必须使用单独 `local_network` grant并显式列 origin。请求/响应还有 timeout、redirect count、bytes、MIME 和 decompression hard limits，以降低 SSRF/资源耗尽风险。

`ToolGrant.constraints` 必须通过 descriptor `bindingConfigSchema`；HostedToolBinding 的 `modelFacingConfig` 通过 OperationKey adapter allowlist/schema。两者禁止 SecretRef、credential、任意 header、路径和未授权 URL，不能把配置对象当逃生口。

NodeInstance snapshot 固定当时的 semantic policy revision（schema、grant 解释、output/effect 规则），保证 resume 可解释；另有持久化、单调 deny-only 的 live revocation overlay。Dispatcher、approval resume 和 finalized commit 都取“pinned grants ∩ current revocations”：新 overlay 只能立即撤销 tool/scope/origin 或收紧 hard cap，不能给旧 snapshot 新增权限或改变成功结果语义。命中时返回 `permission_revoked` 并按 policy fail/wait for operator；采用新 grant 必须创建新 NodeInstance/run。

拒绝信息只说明当前 call 未授权，不枚举其他 tool/scope 是否存在。权限在首次 dispatch、approval 恢复和最终 commit 前各校验一次，防止等待期间 policy/head 已变化。

## Tool Call 解析与 Approval

adapter 完成有序 tool-call item 后，runtime 按以下顺序处理：

1. 用当前 request 的 exposed name 定位唯一 binding。
2. 按原始 item 顺序分配稳定 local `toolCallId + callIndex`，校验 provider call id、registry snapshot 和 descriptor digest。
3. 对 arguments 做 JSON parse、schema、深度、大小和未知字段校验。
4. 解析 material refs，并用 ArtifactGrant/MemoryBinding 校验；不接受本地路径或任意 URL。
5. 计算不可变 `callDigest = hash(binding + arguments + materials + grants + policyVersion)`。
6. 在权限交集已通过后，收集本 batch 所有需要 approval 的 calls，并按 callIndex 创建一个 durable batch WaitRecord；批准记录逐 `(toolCallId, callDigest)` 绑定 actor、policy version 和 expiry。
7. arguments/material/grant 任一变化都会使旧 approval 失效。

同一 model response 可以合法请求多个 tool+arguments 完全相同的 call；它们按 `toolCallId/callIndex` 分别执行和审计，不使用相同 `callDigest` 去重。Provider call id 若缺失或重复只能记为原始 metadata，不能替代 local identity。Approval 精确绑定 `(toolCallId, callDigest)`，因此相同 digest 的多个 call 仍需逐项 decision。

同一 NodeInstance 同时最多一个 open WaitRecord。Batch approval request列出有界的 `{toolCallId, callDigest, riskSummary}`，每个 call 写入 `wait_blockers`；response 必须对每个 `(toolCallId, callDigest)` 恰好给出一个 `approve | reject`。在全部 blockers 收敛前本 batch 一个 call 都不执行。Approved calls继续，rejected calls按 failure policy生成 denied result。Approval 只能确认已获 grant 的高风险调用，永远不能覆盖 missing grant、permission denial 或 live revocation。

未知 tool、malformed arguments或 deny 不启动 executor，按 failure policy 返回有界 `tool_result`，但仍计入 `maxToolCalls`，避免模型无限探测权限。

## ToolExecutionContext

```ts
type ToolExecutionContext = {
  invocation: ToolInvocation
  readBindings: BoundReadPort
  artifacts: ArtifactStagingPort
  memoryProposal?: MemoryProposalPort
  network?: BoundNetworkPort
  cancellation: CancellationToken
}

type BoundNetworkPort = {
  request(input: {
    method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE"
    origin: string
    pathAndQuery: string
    headers?: Record<string, string>
    body?: bytes
  }): Promise<{
    status: number
    headers: Record<string, string>
    body: bytes
  }>
}
```

`BoundReadPort` 只能访问 NodeInstance 已 pin 的 binding/read set。`ArtifactStagingPort.stage({ metadataDraft, declaredMediaType?, body: ByteStream })` 在创建 staging row时校验并不可变绑定 canonical metadata draft/digest，流式写完整 bytes后只返回 opaque staging ID/status，不暴露 storage path；修改 metadata 必须新建 staging。`MemoryProposalPort` 仅供内建 memory capability tool 创建 `MemoryChangeProposal`，不能 apply proposal。

`BoundNetworkPort` 只在有效 `network/local_network` grant 存在时注入。`origin` 必须是 pin grant 中的 canonical origin，`pathAndQuery` 必须是以 `/` 开头的相对目标，不接受 absolute URL/userinfo/fragment。Port 内部统一执行 DNS/IP 检查与 pinning、每次 redirect 重验、禁止 credential/hop-by-hop header、timeout/redirect/decompressed-byte/MIME 限制和 effect audit，response 超限直接返回 typed error。Custom executor 不得自建 socket、HTTP client 或 DNS resolver；需要持久大 response 时转入已授权 `ArtifactStagingPort`。

executor 不自行发布 runtime event、推进 branch、更新 tool ledger 或决定 retry；这些都由 dispatcher/runtime 完成。阶段一 context 中没有 SecretResolver。

## 唯一 Tool Output Union

```ts
type ToolCallOutput = {
  parts: ToolOutputPart[]
}

type ToolOutputPart =
  | { type: "llm_result"; content: LlmContentPartIr[] }
  | { type: "artifact"; stagingId: string }
  | { type: "state_patch"; patch: StatePatch }
  | { type: "memory_change_proposal"; proposal: MemoryChangeProposal }
  | { type: "user_message"; content: LlmContentPartIr[] }
  | { type: "evidence"; refs: string[] }
  | { type: "debug"; summary: string }
```

不再使用模糊的 `memory_patch`、`artifact memory` 或任意 `{kind, payload}`。

- 只有 `llm_result` 回填下一轮模型 transcript；每个成功 custom call 必须恰好一个且 content 非空，内容必须有界、脱敏，并按 untrusted data 处理。
- `artifact` 必须先通过上述 port把 metadata/bytes绑定到 staging；output 只回传 ID，runtime 按 immutable draft commit 后才替换为 `ArtifactRef`，不能在 output/commit 时另传 metadata。
- `state_patch` 只是一项待提交输出；runtime 校验 scope/base commit，并在 node finalized transaction 中应用。
- `memory_change_proposal` 只能处于 canonical proposal 状态，由 MemoryManager 重新校验；tool 不能 apply。
- `user_message` 是带 tool provenance 的 UI event，不得伪装成用户本人或 graph output。
- `debug` 默认不持久化；不得包含 arguments、secret、路径或 raw response。

即使 call 只产生 state patch、artifact 或 memory proposal，tool 也必须返回短 `llm_result` 摘要/引用，不得省略 call/result 配对或由 dispatcher 猜一个 acknowledgement。超出 `maxLlmResultBytes` 时 dispatcher 不截断 JSON；工具应创建 artifact 并返回短摘要/ref。所有 parts 在进入 ledger 前执行 schema、大小、sensitivity 和引用校验。Failed/denied call 不构造 `ToolCallOutput`，由 dispatcher 使用 `07-llm-ir.md` 的有界 canonical error/denied `tool_result`。

`memory_change_proposal` part 只接受 `proposed/awaiting_confirmation/awaiting_review`；approved/applied 等状态只能由 MemoryManager 的后续状态转换产生。

## FailurePolicy

```ts
type ToolFailurePolicy = {
  invalidCall: "model_visible_error" | "fail_node"
  denied: "model_visible_error" | "fail_node"
  approvalRequired: "wait" | "fail_node"
  executionError: "model_visible_error" | "fail_node"
  maxAttempts: number
  retryBackoffMs: number[]
}
```

阶段一默认：invalid/unknown/denied 返回脱敏 model-visible error；已授权但要求 approval 的调用进入 durable wait；schema/runtime invariant、artifact integrity 和 patch validation 失败则 fail node；普通 executor error 可按 descriptor 策略模型可见。

Retry 还受 effect classification 限制：

- `pure`：没有外部可观察 mutation，可按 policy 重试。
- `idempotent`：必须对所有 attempt 使用相同 provider idempotency key，或能按 stable operation 查询结果。
- `non_idempotent`：只有明确 `failed_before_start` 才能自动重试；started 后未知结果必须人工协调。

timeout、连接断开、interrupt 和 cancel 都不证明外部操作失败，不能直接归类为 retryable failure。

## 多工具并发与确定性回填

一个 model call 的 tool calls 按原始 item 顺序分配 `callIndex`。只有同时满足以下条件才并发：

- 每个 descriptor 都声明 `supportsParallel`；
- policy 允许当前 effect classification 并发；
- write scope、artifact staging quota 和互斥 key 不冲突；
- 没有要求先审批/先观察上一 call 结果的依赖。

`non_idempotent` custom tool 阶段一永不并发，即使 descriptor 误声明 supportsParallel；它们按 callIndex 串行，前一个达到 succeeded/failed 才启动下一个。若某个进入 `outcome_unknown`，立即停止启动本 batch 后续 calls并创建唯一 effect-resolution wait。这样一个 NodeInstance 不会同时产生多个 unknown-effect waits。Hosted tools 在单个 provider request 内作为一个 logical model effect整体分类/协调。

执行完成顺序不改变语义顺序。runtime 等同一 batch 的所有 calls 达到 completed/failed/denied，或 durable waiting 后，严格按 `callIndex` 追加 tool result，再开始下一次 model call。

并发返回的 `StatePatch` 使用相同 pinned base 逐项校验，并按 callIndex 组合；path 冲突默认 fail，不采用 completion-order 或 LWW。MemoryChangeProposal 各自独立，以 idempotency key 去重。

一个 call waiting 时，已完成 sibling 的结果可以持久化但不提前发起下一 model call。runtime-fatal error 会取消未开始 sibling；已开始外部 effect 仍按 ledger 收敛，不能假定已取消。

多个 MemoryChangeProposal 同时需要 confirmation/review 时也聚合成一个按 proposal ID 排序并写 `wait_blockers` 的 WaitRecord；response 逐 proposal 决策，全部 terminal才 resolve。若新的 blocker 只在 resume 后出现，再创建下一条 wait，不能并行打开第二条。

## Tool Call 与 Effect Ledger

每个 logical tool call 有稳定记录；每次实际执行有独立 effect attempt。状态属于 `ExecutionState`，详细存储 schema 见 `20-storage-schema.md`。

```ts
type ToolCallRecord = {
  id: string
  nodeInstanceId: string
  originatingAttemptId: string
  modelCallId: string
  callIndex: number
  bindingId: string
  callDigest: string
  argumentsRef: string
  status: "requested" | "validated" | "awaiting_approval" | "prepared" | "running"
        | "completed" | "failed" | "denied" | "outcome_unknown" | "retry_ready" | "cancelled_before_start" | "abandoned_unknown"
  outputRef?: string
}

type EffectRecord = {
  id: string
  nodeInstanceId: string
  owner: EffectOwner
  classification: "pure" | "idempotent" | "non_idempotent"
  operationKey: string
  idempotencyKey: string
  status: "pending" | "succeeded" | "failed" | "outcome_unknown" | "cancelled_before_start" | "abandoned_unknown"
  resultRef?: string
}

type EffectAttemptRecord = {
  id: string
  effectId: string
  invokingNodeAttemptId: string
  attemptNo: number
  status: "prepared" | "started" | "succeeded" | "failed" | "outcome_unknown" | "superseded_before_start"
  providerRequestId?: string
  requestRef: string
  resultRef?: string
}
```

每次 retry 建立新的 `EffectAttemptRecord`，但 logical effect/idempotency key 不变。Model/count/tool/effect logical ledger 归 NodeInstance，`originatingAttemptId` 只记录首次解析 call 的 invocation；真正外调用的 lease/control fence 只取 `EffectAttemptRecord.invokingNodeAttemptId`。因此 approval 前 waiting 的旧 attempt 可以终结，resume attempt 仍能对同一 toolCallId 创建 effect attempt，不转移/改写 logical owner。执行协议：

1. 事务性创建 tool call、effect 和 `prepared` attempt，持久化 request ref。
2. 在外部调用前用 invoking fence CAS `prepared -> started`，失败禁止发送；lease recovery 可先赢得互斥 CAS `prepared -> superseded_before_start`，同事务把 owner/checkpoint 置 retry_ready 并由新 NodeAttempt 创建下一 effect attempt。Superseded 证明从未发送，对 non-idempotent effect 也可安全重建；started row 则必须走 reconcile/outcome_unknown。
3. 收到响应后先持久化 immutable result/object，再 CAS succeeded/failed。
4. validated output parts 之后才能进入 node transition。

崩溃恢复：

```text
prepared
  -> 尚未开始，可按 policy 启动。

started
  -> 先用 providerRequestId/idempotency key 查询。
  -> pure/idempotent 可按 policy 重试。
  -> non_idempotent 且不可查询：outcome_unknown + durable wait。

succeeded
  -> 复用 resultRef，绝不重跑 executor。
```

`outcome_unknown` 不是普通 failure。Effect 必须恰好属于一个 model、count 或 tool call。人工协调只适用于需协调的 model/tool owner，按 `17-runtime-control.md` 的冻结状态机执行；success/retry-safe/abort 同事务把 owner row/checkpoint CAS为 `completed/retry_ready/abandoned_unknown`。Pure count attempt unknown 在同 logical effect 下自动新建 attempt 或使用持久化 local fallback，不建人工 wait。原 EffectAttempt始终保留 `outcome_unknown`；resume 看到可重试 owner/checkpoint 都为 `retry_ready` 后，才在创建下一 prepared EffectAttempt的事务中把二者改为 `prepared`。

Late completion 先检查该 EffectAttempt 的 invoking NodeAttempt fencing token 和 run cancel/interrupt epoch。过期结果可以作为 audit artifact 保存，但不能推进当前 branch 或覆盖新 attempt。

## Durable LlmLoopCheckpoint

`LlmLoopCheckpoint` 是 NodeInstance runtime checkpoint 的 LLM/tool-loop 部分，不替代 `RuntimeCheckpoint`：

```ts
type LlmLoopCheckpoint = {
  schemaVersion: 1
  nodeInstanceId: string
  lastUpdatedByAttemptId: string
  graphRevisionId: string
  registrySnapshot: ToolRegistrySnapshot
  contextSnapshotRef: string
  readSetDigest: string
  modelCallNo: number
  transcriptRef: string
  continuationRef?: OpaqueContinuationRef
  activeModelEffect?: { modelCallId: string; effectId: string; status: "prepared" | "running" | "completed" | "failed" | "outcome_unknown" | "retry_ready" | "cancelled_before_start" | "abandoned_unknown"; responseRef?: string }
  activeCountEffect?: { countCallId: string; effectId: string; countOrdinal: number; countExecutionPinDigest: string; trimCandidateRef: string; trimCandidateDigest: string; requestDigest: string; status: "prepared" | "running" | "completed" | "failed" | "retry_ready" | "cancelled_before_start" | "abandoned_unknown"; resultSource?: "provider" | "local" | "estimate"; resultRef?: string }
  currentBatch: ToolCallCheckpoint[]
  modelCallsUsed: number
  countCallsUsed: number
  toolCallsUsed: number
  effectWatermark: string
  waitIds: string[]
  checksum: string
}

type ToolCallCheckpoint = {
  toolCallId: string
  callIndex: number
  callDigest: string
  status: ToolCallRecord["status"]
  effectId?: string
  outputRef?: string
  waitId?: string
}
```

CountCall 首次 provider request 进入 prepared 的事务必须同时创建/复用 `(nodeInstanceId,countOrdinal)` logical row、pure Effect 和 prepared EffectAttempt，写入 checkpoint 中的 ID、`08-context-assembly.md` 完整 `CountExecutionPin` digest、canonical trim candidate ref/digest（有序 assembled items、transformation report 与完整 `LlmRequestIr`）及 wire-equivalent request digest，并只在新建 logical row时递增 `countCallsUsed`；超 `maxCountCalls` 则整笔零写入。Provider path 的 operation 必须是 snapshot 中 exact CountTokens `LlmOperationExecutionPin`，local fallback 必须使用同一 pin 中 exact counter/version、fallback policy 和 safety margin，不能读取 current 配置。

发送前把 EffectAttempt `prepared -> started` 与 CountCall/checkpoint `prepared -> running` 同事务 CAS。Provider response/result对象先持久化，再用一个事务终结 attempt/effect，并把 CountCall/checkpoint置为 provider completed、允许 fallback 后的 local/estimate completed 或不可 fallback 的 failed；retry-ready→prepared 则原子更新 Effect/CountCall/checkpoint并创建下一 prepared attempt，所有事务都同时写 journal/outbox。恢复按 countCallId/ordinal读取唯一 row/effect，逐项比对 execution-pin/candidate/request digest；已有 terminal result直接复用，started pure attempt只在同 logical effect下新建有界 attempt，provider失败后的 local/estimate result仍属于原 ordinal。Retry/fallback/crash replay都不增加 `countCallsUsed`、不另占 maxCountCalls，也不重新 trim/recount已固定 candidate；只有基于已持久化 count result产生的新 deterministic trim candidate 才创建 ordinal+1，不一致返回 integrity/compatibility error。

恢复先验证 graph/context/registry/read-set digest，再从 effect ledger 收敛 current batch；不能仅凭旧内存状态把 `running` 当作失败，也不能在 pending batch 未收敛时发起下一 model call。

## ArtifactObject 与 ArtifactRef

`ArtifactObject` 是不可变 bytes，按内容寻址；Artifact metadata 是可版本化领域对象。

```ts
type ArtifactObject = {
  contentHash: string
  hashAlgorithm: "sha256"
  byteSize: number
  createdAt: string
}

type ArtifactRef = {
  artifactId: string
  contentHash: string
  byteSize: number
  mediaType: string
}
type ArtifactMetadata = {
  artifactId: string
  content: ArtifactRef
  name?: string
  classification: "public" | "private" | "sensitive"
  status: "active" | "deleted"
  originRunId?: string
  originNodeInstanceId?: string
  originToolCallId?: string
  retention: ArtifactRetention
  createdAt: string
}

type ArtifactMetadataDraft = Omit<
  ArtifactMetadata,
  "artifactId" | "content" | "status" | "originRunId" | "originNodeInstanceId" | "originToolCallId" | "createdAt"
>
```

object-store adapter 在自己的存储记录中把 content hash 映射到 `storageKey`；该字段不属于领域 `ArtifactObject`，也不能出现在 ArtifactRef、LLM context、tool output 或 API。阶段一 `artifactId` 永久绑定创建时的 `content` object/hash/size/media type；修改 bytes 必须创建新 object、新 artifactId 和新 ArtifactRef，不在旧 artifact metadata commit 中换 content ref。`ArtifactMetadataDraft` 故意不接收 media type：caller declaration只作 validation hint，scanner/policy 产生的 canonical `validatedMediaType` 才能进入 ArtifactRef/artifact row。Staging commit 把含 immutable content、`status=active` 的完整 ArtifactMetadata 写为 artifact_metadata root snapshot/commit/projection；后续 StatePatch 只允许 name、classification、retention 和单向 `active -> deleted` status，对 artifactId/content/origin/createdAt 的 patch 一律拒绝，deleted 不可隐式 revive。Branch-local 可见性只由 WorkingContext 的 ArtifactRef 控制；ExecutionState 不使用 StatePatch。

`ArtifactRef` 只是定位符，不是权限 token。读取仍需当前 actor permission、binding、branch reachability 和 ArtifactGrant；Context Assembly 只能使用已解析 artifact binding。

## Staging 与 Commit

Tool/API 上传统一走 staging：

```ts
type ArtifactStagingStatus = "uploading" | "staged" | "validated" | "quarantined" | "deleting" | "deleted" | "committed"
```

唯一合法转换是 `uploading -> staged -> validated -> committed`，`uploading | staged | validated -> quarantined -> deleting -> deleted`；`committed/deleted` 是 terminal，quarantined/deleting 不可恢复为可 commit 状态。Uploading 持 temp key/writer lease并执行增量限制；staged 已 fsync且固定 hash/size，并只把 caller MIME 保存为非权威 declared hint；validated 已通过 magic/policy/scan、固定 canonical `validatedMediaType`，并在同一事务发布/锁定 live content object、写 `validatedContentObjectId` 和 staging owner ref。无法识别时只有 grant/policy 显式允许才使用 `application/octet-stream`，不能照抄 hint；commit 只接受 validated，并原子把该 media type写入 ArtifactRef/metadata root、receipt 与反向关联。

约束：

- staging id 是随机 opaque id，不是路径，也不是 ArtifactRef。
- hash 基于原始 bytes；语义 JSON 去重前使用版本固定的 canonical JSON。
- object 写入采用 temp + fsync + atomic rename；数据库引用只能在 object 可读后提交。
- content hash 去重不跳过 caller quota、classification、scanner 或 permission 校验。
- blob publish 成功但数据库事务失败时对象保持 unreferenced，进入宽限期 GC。
- node completion 只能引用已 committed artifact；staging object 不进入 graph output、memory 或 context。

阶段一先支持整对象 hash；chunk manifest、远程 multipart 和跨 tenant dedup 延后。

## Artifact 安全

- 每个 grant 限制总 bytes、单对象 bytes、对象数和允许 media types。
- 不信任扩展名或调用方 MIME；对 magic bytes、压缩炸弹、递归 archive 和 image dimensions 设置上限。
- HTML/SVG/PDF 等 active content 在 UI 中 sandbox 展示；文件名做路径和控制字符清理。
- 外部/tool 生成内容默认 untrusted/private，不自动进入 prompt；需要显式 binding。
- SecretValue、credential、master key 和 auth header 检测到时 fail closed，不能把 Secret Store 当 artifact store。
- `sensitive` artifact 的 preview、download、raw capture 和 context egress 使用更严格 permission；普通 event 只记录 ref/digest/大小。
- opaque provider continuation 使用 `12-secret-store.md` 的 one-EffectAttempt/one-encrypted-bundle：top-level/hosted/reasoning entries 共享发送前预留的 object ID 和最终 purpose-bound SensitiveWriteLease，各 ref 携带 authenticated entryKey。它不进入普通 content dedup/index、不能作为 ArtifactRef 枚举；checkpoint/event 只保存 opaque ref/ciphertext digest。Store locked 时恢复通过 `secret_store_unlocked` wait。

## Retention 与 GC

```ts
type ArtifactRetention =
  | { type: "ephemeral"; expiresAt: string }
  | { type: "run" }
  | { type: "context" }
  | { type: "pinned" }
  | { type: "audit_until"; timestamp: string }
```

GC roots 与 `16-domain-consistency.md` 一致：retained branch/commit、RuntimeCheckpoint、durable event ref、Conversation candidate、pending proposal/evidence、effect request/result、active staging lease 和 user pin。

GC 使用 mark-and-sweep 加宽限期：先标记候选，再用 `06-persistent-versioning.md` 的 lifecycle/delete-fence 事务线性化最终复核与删除。删除 artifact metadata/ref 不立即删共享 bytes；abandoned branch、failed run 和 cancelled staging 只有在 retention/audit 允许后回收。Owner repository 只能给 live object 新增 ref，遇到 deleting/deleted 必须重试上传/引用事务，不得直接“取消”已开始的物理删除。

Staging 有独立短 retention 和 heartbeat/lease。每次转换以 `(id,status,lifecycleGeneration)` CAS并原子递增 generation；commit、cancel、scanner、expiry/GC 竞争只能一个获胜。进入 deleting 的事务必须复核 quarantine 宽限已过、writer lease失效、未 committed，移除 validated staging owner ref，并写唯一 durable `deleteFence`。删除 worker只在 `(id,deleting,generation,deleteFence)` 仍匹配时幂等删除 staging temp/quarantine bytes；成功或目标已不存在都以相同 expected fence/generation CAS 为 deleted并递增 generation，live content bytes 另由普通 object GC处理。崩溃后 repair 重用该 fence继续，fence/generation不匹配只停止，不能猜文件名或倒退状态。Uploading 过期恢复到 quarantined；staged 可重跑 scanner；object publish 后事务失败只留下普通 unreferenced object；validated/commit response crash分别从 owner ref或同 request digest receipt恢复。Committed 清空 temp key/移除 staging ref，并按 artifact source、幂等与 audit retention 保留 row，不进入 staging GC。

## 阶段一验收边界

阶段一必须验证：

- ungranted、版本漂移、malformed 和越权 material call 被拒绝；
- approval 绑定 call digest，恢复后不能换参复用；
- 并发完成顺序不同仍按 callIndex 得到相同 transcript；
- crash 发生在 effect started 后不会重复 non-idempotent 副作用；
- `outcome_unknown` 可 durable wait 和人工解决；
- tool 只能返回 canonical output parts，不能直接写 DB/memory；
- staging crash 不产生可见 ArtifactRef，commit crash 可由 ref/object repair 恢复；
- unreachable blob 在保留期后回收，仍被 branch/checkpoint/effect 引用的对象不回收。
