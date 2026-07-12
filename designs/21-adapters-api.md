# Core API 与 Adapter 契约

## 定位

Core 暴露 runtime 与 application domain 的命令、查询和事件订阅 ports。Axum、Tauri 和未来的其他入口只负责认证、DTO 校验、协议转换与流控，不能直接操作 scheduler 表或复制业务事务。

```text
Web UI -> Axum adapter  -> core service ports
Local UI -> Tauri adapter -> core service ports
Worker / test            -> RuntimeService

core service ports
  RuntimeService
  Graph/Channel/ContextPreset services
  Conversation/Memory/Artifact/SecretStore services
```

Core 不依赖 HTTP 状态码、SSE、WebSocket、Tauri handle 或 SeaORM entity。

## 命令与查询

阶段一的稳定入口：

```ts
type StartRunCommand = {
  graphRevisionId: string
  input: JsonValue
  context:
    | { mode: "temporary" }
    | {
        mode: "existing"
        contextId: string
        branchId: string
        expectedHeadCommitId: string
      }
  deadlineAt?: string
  idempotencyKey: string
}

type RunControlCommand = {
  runId: string
  expectedEpoch: number
  idempotencyKey: string
  reason?: string
}

type SatisfyWaitCommand = {
  waitId: string
  deliveryId: string
  response:
    | { type: "value"; value: JsonValue }
    | {
        type: "blocker_decisions"
        decisions: Array<
          | { kind: "tool_call"; blockerId: string; callDigest: string; decision: "approve" | "reject" }
          | { kind: "memory_proposal"; blockerId: string; decision: "approve" | "reject" }
        >
      }
}

type ResolveEffectUnknownCommand = {
  effectId: string
  expectedEffectAttemptId: string
  expectedStatus: "outcome_unknown"
  expectedRunControlEpoch: number
  resolution:
    | { type: "confirm_succeeded"; resultRef: string; evidenceRefs: string[] }
    | { type: "confirm_failed_retry_safe"; evidenceRefs: string[] }
    | { type: "abort_run"; evidenceRefs?: string[] }
  idempotencyKey: string
}

type RuntimeService = {
  startRun(command: StartRunCommand): Promise<RunView>
  getRun(runId: string): Promise<RunView>
  getRunOutputs(runId: string): Promise<RunOutputsView>
  listOpenWaits(runId: string): Promise<WaitView[]>

  requestInterrupt(command: RunControlCommand): Promise<RunView>
  resumeInterrupted(command: RunControlCommand): Promise<RunView>
  requestCancel(command: RunControlCommand): Promise<RunView>
  satisfyWait(command: SatisfyWaitCommand): Promise<WaitView>
  resolveEffectUnknown(command: ResolveEffectUnknownCommand): Promise<EffectView>

  forkContext(command: ForkContextCommand): Promise<BranchView>
  mergeContext(command: MergeContextCommand): Promise<MergeView>
  subscribeRun(runId: string, afterDurableSeq?: number): EventSubscription
}
```

```ts
type RunOutputValueView =
  | {
      kind: "inline_json"
      valueRef: string
      contentHash: string
      sizeBytes: number
      value: JsonValue
    }
  | {
      kind: "json_value_ref"
      valueRef: string
      contentHash: string
      sizeBytes: number
      downloadPath: string
    }

type RunOutputsView = Record<string, {
  collection: "single" | "append"
  values: RunOutputValueView[]
}>
```

`temporary` 原子创建临时 WorkingContext/root commit/branch；`existing` CAS branch head 等于 `expectedHeadCommitId`，并把它记录为 run `inputCommitId`。从历史 commit 执行时先显式 fork branch。ConversationService 另存 `runId -> conversationId/turnId` 投影；core 不解析 RP 字段。

持久化 `deadlineAt` 取调用方值与 `startedAt + RunLimits.maxRunWallClockMs` 的较早者；调用方省略时仍使用 hard run deadline，不能创建无限 run。

`expectedEpoch` 是运行控制的乐观并发令牌。冲突时返回当前 `RunView`，调用方不能用最后写入覆盖并发控制请求。

Effect resolution 只对有权限的协调者开放；服务端重新校验 effect classification、唯一 model/tool owner、expected unknown attempt、evidence/result ref、retry budget 和 run epoch。`confirm_succeeded` 使 logical effect/owner/checkpoint成为 `succeeded/completed/completed`；`confirm_failed_retry_safe` 使其成为 `pending/retry_ready/retry_ready`，下一 EffectAttempt只在 resume把 owner/checkpoint CAS为 prepared时创建；`abort_run` 把 effect/owner/checkpoint置为 `abandoned_unknown`，隔离结果并以 hard-cancel fencing终止 run。原 unknown EffectAttempt不改写；immutable resolution、上述 normalized rows、wait blocker和 journal/outbox同事务提交，任一 CAS失败全部回滚。客户端不能直接改 effect/owner row或自行声明 retryable。

Generic `satisfyWait` 只接受 human/approval/webhook 等允许外部响应的 kind。服务端锁定并按 order逐个校验 `wait_blockers`；decision payload必须恰好覆盖所有 open tool/proposal blockers，整批校验后才同事务更新对应领域状态。存在任何 open effect blocker时整条 generic command返回 `effect_resolution_required`且零写入；只有全部 blockers terminal且无 aborted才 resolve wait并把 NodeInstance置 ready。`secret_store_unlocked`、timer 和内部 coordinator wait同样必须拒绝并由专用 system/application command解决。

所有 mutation command 必须带调用方生成的 idempotency key；wait response 的 `deliveryId` 是该 command 的幂等键。同一作用域、同一 key、同一请求摘要在 result retention 内返回原结果，即使 effect/wait/run随后已推进；同一 key配不同请求返回 `idempotency_conflict`。Run/control/wait/effect 使用各自的 durable command/delivery ledger，其他 application mutation 使用 `20-storage-schema.md` 的 `application_command_receipts`，并与业务变更同事务写入；不得用可覆盖的 current projection 字段代替历史 receipt。Result 到期后同 digest 返回 `idempotency_key_expired` 而不重施，tombstone 不复用。Effect resolution另以 `expectedEffectAttemptId` 保证每次 unknown cycle最多一个结论。

Secret-bearing initialize/create/update/password-change/unlock 是上述 request-digest 规则的安全例外：adapter 不计算/记录对 secret bytes 的普通 hash，而在成功的 Secret Store 边界按 `12-secret-store.md` 计算 data-key-derived HMAC 并使用 `secret_command_receipts`。失败 unlock 不产生可离线验证的 receipt；重放成功命令必须在受控边界重算 HMAC。Initialize/unlock 的原 result 只在绑定的内存 session/generation 仍有效时返回；失效后同 key 返回 `idempotency_key_expired` 且不建立新 session，调用方用新 key unlock。Lock/不含 secret bytes 的纯 metadata 命令仍使用普通 digest。

## Run Input 与结果

`startRun` 的事务边界：

```text
校验 applied graph revision
-> 校验完整 run input schema、大小和 secret 规则
-> 校验 context/branch binding
-> 持久化不可变 RunInputRef
-> 创建 GraphRun、入口 NodeInstance 和 durable events
-> commit 后唤醒 scheduler
```

返回成功只表示 run 已持久化，不表示已经执行完成。同步便捷方法 `invoke` 可以由 SDK 组合 `startRun + subscribe/wait`，不作为另一套执行路径。

`RunView` 是可演进的只读投影：

```ts
type RunView = {
  id: string
  graphRevisionId: string
  status: string
  controlEpoch: number
  contextId: string
  branchId: string
  inputCommitId: string
  inputRef: string
  outputCommitId?: string
  lastDurableSeq: number
  deadlineAt: number
  createdAt: number
  updatedAt: number
}
```

Run output 的领域值始终是 GraphOutputContract 验证过的 JsonValue/ValueRef，adapter 不能因为大就把它替换成 ArtifactRef 而改变 schema。小 JSON 用 `inline_json`；超过 API cap 时用 `json_value_ref` 作为传输 envelope，客户经授权 value endpoint 下载同一 canonical JSON。只有业务 output 本身就是 ArtifactRef 时，其 JSON value 才包含 ArtifactRef，并继续经 artifact permission 读取。`RunOutputsView` 恰好包含 revision output contract 的每个 key；未产生的 optional key 使用空 `values`。`values` 对 single 为 0/1 项，对 append 按 outputSeq 升序；required 缺失会先使 run failed，不伪造 placeholder。

## Axum HTTP

推荐资源接口：

```text
POST   /v1/graphs/{graphRevisionId}/runs
GET    /v1/runs?limit={1..100}
GET    /v1/runs/{runId}
GET    /v1/runs/{runId}/outputs
GET    /v1/runs/{runId}/waits
GET    /v1/runs/{runId}/events
POST   /v1/runs/{runId}/interrupt
POST   /v1/runs/{runId}/resume
POST   /v1/runs/{runId}/cancel
POST   /v1/waits/{waitId}/responses
POST   /v1/effects/{effectId}/resolution
POST   /v1/contexts/{contextId}/branches
POST   /v1/contexts/{contextId}/merges
GET    /v1/artifacts/{artifactId}/content
GET    /v1/values/{valueRef}
```

所有 mutation（run/control/wait/effect/fork/merge/Conversation/proposal/artifact/config）都使用 `Idempotency-Key` 或 body 中等价字段。`If-Match` 可以承载 run control epoch、draft token 或 branch/head commit；adapter 将其转换成 core command 字段。

成功创建 run 返回 `202 Accepted`。领域错误统一 envelope：

Effect resolution 的 HTTP body 固定为
`{ expectedEffectAttemptId, expectedRunControlEpoch, kind, decision, resultObjectId?, evidenceObjectId? }`；
`resolutionId` 由服务端预分配，不属于 request digest。同一个 `Idempotency-Key` 重放时，即使 adapter
重新预分配了候选 ID，也必须返回原 resolution，不能产生 digest conflict。HTTP principal 在 adapter
边界映射为 actor，客户端不能伪造 `actorKind/actorId`。

```ts
type ApiError = {
  error: {
    code: string
    message: string
    retryable: boolean
    details?: JsonValue
    traceId: string
  }
}
```

映射原则：请求格式/contract 错误为 `400`，认证为 `401/403`，不存在为 `404`，幂等或 head/epoch 冲突为 `409`，等待条件或 idempotency result 过期为 `410`，限流为 `429`，暂时性存储/上游故障为 `503`。Provider 原始错误、SQL 错误和 secret 不进入 `details`。

## Application Service API

Graph editor、Conversation 和 MemoryManager 位于 core runtime 之上的应用服务。它们可以组合多个 core/storage invariant，但 handler 仍不能直接拼数据库事务。

这些是独立的窄 service ports，不并入巨型 `RuntimeService`：GraphService 管 draft/apply，ChannelService 和 ContextPresetService 管版本化配置，ConversationService 管 root/turn/candidate，MemoryManager、ArtifactService、SecretStoreService 管各自领域。Adapter composition root 注入所需 port；Axum 与 Tauri 的同一命令必须调用同一 service method/repository transaction。Application service 可以复用 RuntimeService 的纯 validation/plan API或调用无需跨域原子的高层 command，但不能访问 scheduler table；`submit_turn + create_run` 必须调用专用 application storage transaction，禁止先提交消息再单独 `startRun`。

Fresh install 必须能只经 application service 创建顶层资源：

```ts
type CreateGraphCommand = { name: string; idempotencyKey: string }
type CreateChannelCommand = { name: string; idempotencyKey: string }
type CreateContextPresetCommand = { name: string; idempotencyKey: string }

type InitializeSecretStoreCommand = {
  masterPassword: SecretValue
  idempotencyKey: string
}

type SecretStoreStatusView =
  | { initialized: false }
  | { initialized: true; storeId: string; formatVersion: 1; locked: boolean }
```

这里的 `SecretValue` 是 `12-secret-store.md` 的 non-serializable application-port 明文包装；adapter 把 write-only request 字段解析后立即转为 Zeroizing buffer。`CreateConversationCommand` 由 `13-conversation-turn-run.md` 唯一定义。CreateGraph 返回 graph ID + 初始 draft token，draft 固定为空 nodes/edges/outputContract；CreateChannel/CreateContextPreset 返回 head 为空的资源 ID；后续分别通过 draft update、channel revision、preset version 接口发布内容。CreateConversation 返回 conversation/context/root branch/root commit ID。InitializeSecretStore 返回 `{ storeId, formatVersion: 1, sessionId }`，且只在 header 不存在时原子创建；正常返回时当前进程保持 unlocked。

Graph/draft、Channel、Preset、Conversation/context root 的创建分别与 `application_command_receipts` 同事务提交；Secret 初始化与 header/audit/data-key-derived HMAC receipt 同事务提交。所有 ID 在事务前预分配，同 key 重放返回原资源，不能用查询后再插入的两步流程模拟幂等。

阶段一还需要：

```text
POST   /v1/graphs
GET    /v1/graphs
GET    /v1/graphs/{graphId}/draft
PUT    /v1/graphs/{graphId}/draft
POST   /v1/graphs/{graphId}/apply
GET    /v1/graphs/{graphId}/revisions/{revisionId}
GET    /v1/graph-revisions/{revisionId}
GET    /v1/roleplay/graph-options
GET    /v1/graph-revisions/{revisionId}/roleplay-compatibility
GET    /v1/graph-revisions/{revisionId}/roleplay-settings

GET    /v1/channels
POST   /v1/channels
POST   /v1/channels/{channelId}/revisions
POST   /v1/channels/{channelId}/model-discovery
POST   /v1/context-presets
GET    /v1/context-presets
GET    /v1/context-presets/{presetId}
POST   /v1/context-presets/{presetId}/revisions
POST   /v1/context-presets/{presetId}/preview
GET    /v1/tools/descriptors

POST   /v1/conversations
GET    /v1/conversations
POST   /v1/conversations/{conversationId}/turns
GET    /v1/conversations/{conversationId}
GET    /v1/conversations/{conversationId}/turns
PUT    /v1/conversations/{conversationId}/run-profile
GET    /v1/turns/{turnId}/candidates
POST   /v1/turns/{turnId}/regenerations
PUT    /v1/turns/{turnId}/selection
POST   /v1/turns/{turnId}/candidates/{runId}/projection-resolution

GET    /v1/contexts/{contextId}/branches
GET    /v1/contexts/{contextId}/commits
GET    /v1/contexts/{contextId}/diff?from={commitId}&to={commitId}

GET    /v1/memory-proposals
POST   /v1/memory-proposals
POST   /v1/memory-proposals/{proposalId}/decision
POST   /v1/memory-proposals/{proposalId}/apply
GET    /v1/memories/{memoryId}
POST   /v1/memory-search

POST   /v1/artifacts/staging
GET    /v1/artifacts/staging/{stagingId}
POST   /v1/artifacts/staging/{stagingId}/commit
GET    /v1/artifacts
GET    /v1/artifacts/{artifactId}
GET    /v1/artifacts/{artifactId}/content
GET    /v1/secrets
PUT    /v1/secrets/{secretId}
GET    /v1/secret-store/status
POST   /v1/secret-store/initialize
POST   /v1/secret-store/unlock
POST   /v1/secret-store/lock
```

Draft update 使用 revision token，apply 返回 immutable revision ID/content hash/diagnostics。`/v1/roleplay/graph-options` 只投影每个 Graph 最新 applied revision，返回 graph/revision identity、合同兼容的 reply output keys、可识别的主 LLM node和 compatibility view；单 revision endpoint返回同一判定。Role Play compatibility/settings query由application service按 `24-agentic-role-play-ui.md` 的versioned profile生成，浏览器不能遍历任意Graph猜主节点或ContextItem；settings view只返回可无损映射字段和locked reasons，实际保存仍写canonical GraphDraft/ContextPreset等资源。提交 Turn 原子写 user commit、Turn、candidate branch 和 run；regenerate 不重复 user message；selection 带 expected conversation head。Conversation create可带default run profile，更新profile使用revision CAS和application receipt，只影响后续Turn。Proposal command 带 expected memory head/policy version。

Turn 与 regeneration body 分别使用 `13-conversation-turn-run.md` 的 `SubmitConversationTurnCommand` 和 `RegenerateConversationCandidateCommand`；两者都必须携带 `{ graphRevisionId, replyOutputKey, inputShape: "conversation_message_v1" }` 的 `ConversationRunSpec`。Adapter 不选第一个/default output，ConversationService 在事务前校验该 key 的 `required + single`、compilation `canonicalDocumentHash` 与内建 `AssistantReplyPayloadV1` 完全相同且每项 effective limit `<=` canonical cap；不执行任意 JSON Schema subsumption。实例仍按 owner 的完整 `schemaHash/compiled payload` exact validate，并把 key 持久化到 candidate run binding。Run input 由 ConversationService 从已提交 user message 构造 canonical `ConversationRunInputV1`；regenerate 重用原 Turn input，不从第二个自由 JSON 字段猜测映射。

Channel/model discovery 只返回临时结果；发布 immutable channel revision 后才成为运行权威。Preset preview 显式选择 local 或 remote count，后者显示目标 channel；若 store locked，run 外 application request 返回 typed `secret_store_locked`，UI解锁后用同 request digest/idempotency key重试，不伪造缺少 run/node 的 WaitRecord。Tool descriptor API 只返回当前 principal 可发现的 model-facing metadata，不返回 executor key、其他 scope 或 secret。`GET /graph-revisions/{id}` 让 RunView 可直接解析固定 revision。

Preset preview 的阶段一 request body 为 `{ versionId?, nodeInput, sampleBindings, budget }`；`sampleBindings` 是调用方主动提供的测试材料，不是权限 token，也不触发 storage resolver。缺失 binding 只作为 unresolved empty sample 展示。响应固定 metadata-only，并明确 count source；adapter 不返回 assembled content，也不能把 sample digest 写成 GraphRun/NodeInstance read set。真实执行预览必须读取该 run 已 pin 的 snapshot/read set，不能把此 endpoint 的 sample 结果当作恢复事实。

Secret 列表只返回 SecretRef/metadata。Secret initialize/create/update/unlock request 被 adapter 标记为 sensitive：禁止 body logging、重试缓存、analytics 和错误回显；远程 Web 部署必须使用 TLS。Tauri 可以通过平台安全输入调用等价 application command。

## SSE 事件流

`GET /events` 默认返回 SSE。客户端通过 `Last-Event-ID` 或 `after` 传 durable cursor。

```text
1. 以请求 cursor 建立该连接唯一的 dbCursor，并注册 wake-hint channel。
2. 单一 drain loop 查询 durableSeq > dbCursor，按 durableSeq ASC 分页发送并推进 cursor。
3. drain 为空后等待 hint 或有界 poll deadline；被唤醒后仍回到步骤 2 查数据库。
4. ephemeral live channel 独立转发，不参与 dbCursor。
```

Commit 后 notifier 只发送可重复、可合并、可丢失、可乱序的“可能有新行”提示；adapter 禁止直接转发 notifier 携带的 durable event/payload/sequence。每个连接只有上述 drain loop 能写 durable frame，因此即使并发 transaction 的 commit callback 乱序，客户端仍只看到数据库中的严格递增 `durableSeq`。Sequence 允许空洞，loop 不等待 `dbCursor + 1`；hint 丢失由有界 poll 补偿。

SSE `id` 只设置为实际从 durable event row 读取的 sequence。Ephemeral delta 没有 `id` 或 durable cursor，允许相对 durable frame 任意交错且断线后不补发；它不能推进 dbCursor。最终 `llm.call.completed` / `node.completed` 可恢复最终内容。

每个连接使用有界队列。慢消费者可以丢 ephemeral event；durable event 队列将满时断开连接，让客户端凭最后实际收到的 SSE `id` 重连，不能阻塞 runtime 或无限缓存。重连同样只从数据库按 cursor drain。

服务端定期发送无 `id` 的 heartbeat。已完成 run 仍允许读取保留期内的 durable log。

## WebSocket 控制

阶段一的双向入口可以复用同一组 core commands：

```ts
type ClientFrame = {
  requestId: string
  command: "interrupt" | "resume" | "cancel" | "satisfy_wait" | "resolve_effect"
  body: JsonValue
}

type ServerFrame =
  | { requestId: string; result: JsonValue }
  | { requestId: string; error: ApiError["error"] }
  | { event: StreamEvent }
```

WebSocket 不是独立状态机。重连、授权、幂等和并发规则与 HTTP/SSE 相同；缺少 durable cursor 的 live delta 同样不保证补发。若第一阶段 UI 不需要单连接双向通信，可以只实现 HTTP commands + SSE，不影响 core。

## Tauri Adapter

Tauri commands 使用与 HTTP adapter 等价的 serde DTO：

```text
start_run
get_run
interrupt_run
resume_run
cancel_run
satisfy_wait
resolve_effect_unknown
fork_context
merge_context
create_graph
create_channel
create_context_preset
create_conversation
get_roleplay_settings
update_conversation_run_profile
initialize_secret_store
get_secret_store_status
```

事件通过 Tauri event channel 发出，但订阅仍从 durable cursor 开始。Tauri adapter 不内嵌 Axum server，也不直接持有数据库 transaction；runtime control 调用同一个 `RuntimeService`，bootstrap/Conversation/config 等调用与 Axum 相同的对应 application service port。

Graph、Channel、Preset、Conversation、Memory、Artifact 和 Secret 的 application commands 同样提供 serde 等价 Tauri wrappers；上方只枚举 bootstrap 与 runtime control 的最小入口，不是 Tauri API 白名单。

Secret 初始化/解锁、选择本地文件等平台能力属于 adapter port。Initialize/unlock request 使用 write-only sensitive DTO：只实现受控 `Deserialize` 并立即转为 Zeroizing buffer，不实现 `Serialize/Debug/Clone`，response 永不返回明文；主密码不得进入 tracing span、IPC日志或重试缓存。

## Artifact 传输

Artifact 下载先做 metadata 权限校验，再由 object store 以流式 reader 返回。HTTP adapter设置安全的 `Content-Type`、`Content-Length` 与下载文件名；不信任上传时提供的路径或 MIME。

Value endpoint 仅解析 `json_value_ref`，必须校验该 value 仍由调用者可读的 run output/retained owner 引用，固定返回 `application/json` 的 canonical bytes，并校验 contentHash/size。ValueRef 不是 permission token，不能用该 endpoint 枚举普通 content object 或 internal-sensitive object。

阶段一 HTTP `POST /artifacts/staging` 是一次有界 streaming multipart 上传（canonical metadata draft、可选 declared media-type hint + 单个 object bytes），不是把大文件读成 JSON/内存 buffer。服务端先校验 classification/retention/name policy并在创建 row时不可变绑定 metadata object/digest，再推进 uploading→staged并同步完成 phase-one policy/scan，成功返回 validated staging view；更换 metadata 必须新建 staging。未来异步 scanner 可返回 staged并由同一 GET view 观察，但不能让客户端直接写 status。创建/查询响应只暴露 `{ stagingId, status, lifecycleGeneration, byteSize?, contentHash?, validatedMediaType? }`，其中 media type 仅在 scanner/policy 固定后出现；不暴露 storage key、content object ID 或 ArtifactRef。失败/取消进入 quarantine，由延迟 fenced GC 清理。

Commit body 只有 `expectedLifecycleGeneration`，不再接收 metadata/retention/media override；服务端重验创建时绑定的 metadata digest与当前 policy，只接受 validated 状态和 `Idempotency-Key`，以 staging ID 为 scope对该 body计算 request digest。相同 digest 重放从 receipt 返回同一 committed ArtifactRef/metadata head，不同 digest返回 `idempotency_conflict`；只有 committed response 可以包含 ArtifactRef。CAS 失败时客户端读取最新 staging view，不能把 staged/quarantined/deleting 状态猜成已提交。

## 版本与兼容

- 外部路由以 `/v1` 版本化。
- Event 和持久化 payload 各自带 `schemaVersion`；增加可选字段不升 API major。
- `operationTaxonomyVersion/adapterDecoderVersion` 是 LLM shape compatibility ID，不是 HTTP API 版本。Graph Apply 与 channel publish response 暴露最终版本对；ShapeAdapter 只按 exact `(taxonomy, decoder, OperationKey)` dispatch。
- 未知、缺失或 graph/channel/snapshot 不匹配的 operation 版本返回 typed `unsupported_operation_taxonomy`、`unsupported_adapter_decoder` 或 `operation_version_mismatch`，并在创建 provider effect/发网络请求前 fail closed；adapter 不按名字猜 provider或 current decoder。
- Graph/channel import 与读取先以 bounded envelope 提取 version，再调用对应 `OperationKey` decoder；不能让 Axum/Tauri DTO 直接用当前 Rust enum吞掉未知历史 payload。
- Graph、tool、wait 和 run input 使用 `16-domain-consistency.md` 的唯一 `JsonSchemaSpec`。DTO 层的大小/深度预检不能替代 canonical compiled validator，也不能 coercion、应用 default 或丢弃 unknown properties。
- JSON body/IPC decoder 必须在进入 `JsonValue` 时保留 bounded exact-decimal number，不能先经 f64/JavaScript `Number` round-trip 再 hash/validate；超 digit/exponent limit 直接报 contract error。业务 ID 若要求无损跨 JS 客户端传输应由 schema 声明为 string。
- Graph、preset、tool 和 policy 使用不可变 revision，不接受调用方上传运行时内部状态。
- Adapter DTO 与领域类型显式转换，数据库 entity 不作为 API response。
- 列表接口使用稳定 cursor，不用 offset 承载 event 或大型历史查询。

## 安全边界

- Run input、wait response、tool 参数和 graph definition 都有 schema、深度、字节数与集合长度限制。
- Artifact、event、prompt preview 和 raw provider response 按调用者 scope 授权并默认脱敏。
- API key、主密码、Authorization header 和 provider credential 永不进入请求回显、event 或错误。
- 本地模式也保留 workspace/resource ownership 检查；阶段一可以只有一个 principal，但不能用全局可变对象绕过接口。
