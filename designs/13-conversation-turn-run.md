# Conversation、Turn、Candidate 与 GraphRun

## 定位

RP/chat 的默认模型是一次用户输入创建一个 Turn，并为每个回复候选创建一个 GraphRun。

```text
Conversation   长期容器和当前 active context branch 指针
Turn           一个已提交 user message 的索引
Candidate      从该 user commit 执行的一次 sibling GraphRun
GraphRun       core runtime 的一次因果执行边界
```

Conversation domain 位于应用服务层。Core runtime 只理解 `contextId/branchId/commitId` 和 opaque external binding，不解释消息角色、swipe 或 candidate 选择。

## Schema

```ts
type Conversation = {
  id: string
  title?: string
  contextId: string
  activeBranchId: string
  activeHeadCommitId: string
  runProfile?: ConversationRunProfile
  createdAt: string
  updatedAt: string
}

type ConversationContextV1 = {
  schemaVersion: 1
  messages: ConversationContextMessageV1[]
}

type ConversationContextMessageV1 =
  | {
      messageId: string
      turnId: string
      role: "user"
      source: "user_input"
      contentRef: string
      parentMessageId: string | null
      originRunId: null
    }
  | {
      messageId: string
      turnId: string
      role: "assistant"
      source: "run_output" | "saved_partial"
      contentRef: string
      parentMessageId: string
      originRunId: string
    }

type ConversationMessage = {
  id: string
  conversationId: string
  turnId: string
  branchId: string
  commitId: string
  parentMessageId: string | null
  role: "user" | "assistant"
  source: "user_input" | "run_output" | "saved_partial"
  contentRef: string
  originRunId: string | null
  createdAt: string
}

type ConversationTurn = {
  id: string
  conversationId: string
  userMessageId: string
  userCommitId: string
  createdAt: string
}

type AssistantCandidate = {
  turnId: string
  runId: string
  branchId: string
  baseCommitId: string
  replyOutputKey: string
  status:
    | "queued"
    | "running"
    | "ready"
    | "projection_conflicted"
    | "projection_failed"
    | "projection_abandoned"
    | "failed"
    | "cancelled"
  assistantMessageId?: string
  candidateCommitId?: string
  projectionErrorRef?: string
  createdAt: string
}

type ConversationSelection = {
  turnId: string
  selectedRunId: string
  selectedAt: string
}

type ConversationRunSpec = {
  graphRevisionId: string
  replyOutputKey: string
  inputShape: "conversation_message_v1"
}

type ConversationRunProfile = ConversationRunSpec & {
  revisionNo: number
}

type ConversationRunInputV1 = {
  schemaVersion: 1
  conversationId: string
  turnId: string
  userMessageId: string
  userCommitId: string
  content: LlmContentPartIr[]
}

type AssistantReplyPayloadV1 = {
  schemaVersion: 1
  type: "assistant_reply"
  content: LlmContentPartIr[]
}

type CreateConversationCommand = {
  title?: string
  defaultRun?: ConversationRunSpec
  idempotencyKey: string
}

type UpdateConversationRunProfileCommand = {
  conversationId: string
  expectedRevisionNo: number
  run: ConversationRunSpec
  idempotencyKey: string
}

type SubmitConversationTurnCommand = {
  conversationId: string
  expectedHeadCommitId: string
  userContent: LlmContentPartIr[]
  run: ConversationRunSpec
  idempotencyKey: string
}

type RegenerateConversationCandidateCommand = {
  turnId: string
  expectedUserCommitId: string
  run: ConversationRunSpec
  idempotencyKey: string
}

type ResolveCandidateProjectionCommand = {
  turnId: string
  runId: string
  expectedCurrentBranchHead: string
  resolution:
    | { type: "append_after_current"; reason: string }
    | { type: "abandon_projection"; reason: string }
  idempotencyKey: string
}
```

Candidate 以 `(turnId, runId)` 唯一，不需要另一份 candidate ID。Message content 存 content object；message row 是领域索引，commit graph 是 branch/history 权威。Turn 不复制聊天内容或 candidate output。

系统 schema registry 发布唯一内建的 `AssistantReplyPayloadV1` canonical `JsonSchemaSpec`。其 document 是 closed object：只允许且必须包含 `schemaVersion/type/content`，前两项分别为 `const: 1` 和 `const: "assistant_reply"`，`content` item 使用本地 `$defs` 完整嵌入 canonical `LlmContentPartIr`/`ArtifactRef` schema，`additionalProperties=false`，不使用 remote ref。V1 canonical cap vector 恰为 `16-domain-consistency.md` 列出的 phase-one baseline hard caps；Release manifest 固定该 document 的 `canonicalDocumentHash`、profile/format version 和 cap vector，任一变化都要发布新 contract version，不能根据当前默认值在启动时临时生成另一份 schema。

创建 candidate 前校验 `replyOutputKey` 在该 GraphRevision output contract 中是 `required + single`，且其 compilation 的 `canonicalDocumentHash` 必须与上述内建 schema 完全相同，owner effective limits 每一项都必须 `<=` canonical limit cap。完整 `schemaHash` 包含 owner-specific limits，因此允许在相同 document 下因进一步收窄 limits 而不同。阶段一不尝试判定任意 JSON Schema 的“可赋值/子类型”关系；未来若接受其他形状，必须发布显式 compatibility ID + canonical-document-hash allowlist。Candidate 持久化该 key，projector 不读取后来修改的 Conversation 配置或猜测“第一个 output”。Projector 仍使用 owner 固定的完整 `schemaHash/compiled payload` 对实例执行 exact validation，只接受该 tagged object 并直接使用其 `content`，不对任意 JSON 做 stringify、字段猜测或隐式转换。普通 GraphRevision 无需声明 conversation-specific 字段；同一图的不同 workflow binding 可选择不同合法 reply key。

Conversation workflow 的 run input 不由 adapter 把用户消息猜成任意 JSON。阶段一只支持 `conversation_message_v1`：ConversationService 预分配 Turn/message/commit IDs，以已持久的 user content 构造 canonical `ConversationRunInputV1`，要求 GraphRevision 显式存在且验证通过对应 `runInputSchema`，再把同一份不可变 RunInputRef 交给 core create-run 事务。Regenerate 必须从原 Turn 重用相同 user message/content/commit 构造同一 input，不接受另一份隐式输入；新 graph revision 不接受该 schema 时创建直接失败。

## Conversation Root 与 Bootstrap

`CreateConversationCommand` 是 fresh workspace 和新对话的唯一 bootstrap 入口。服务预分配 conversation/context/root branch/root commit ID，并以 canonical `ConversationContextV1 { schemaVersion: 1, messages: [] }` 创建 WorkingContext。Root commit 的 `aggregateId=contextId`、`lineageKey=rootBranchId`、`operationId=conversation-root:<conversationId>`，没有 parent，以该完整值的 immutable object 作为 initial snapshot；root branch 的 `creationOperationId=conversation-root-branch:<conversationId>`、`parentBranchId=null`、`forkCommitId=headCommitId=rootCommitId`。

预写 snapshot object 后，下列逻辑记录必须在一个数据库事务中可见：Context、root commit、root branch、`headCommitId=rootCommitId` 的 materialized projection、Conversation 的 `activeBranchId=rootBranchId/activeHeadCommitId=rootCommitId`、durable domain audit/outbox 和 application command receipt。事务失败不能留下可查询的半个 Conversation；同一 idempotency key + digest 返回同一组 ID，不同 digest 冲突。创建结果把 root commit ID 返回为首个 `SubmitConversationTurnCommand.expectedHeadCommitId`，不要求调用方另行创建 Context 或猜测空 head。

Root 不创建 Message 或 Turn。`messages: []` 是可 replay 的历史起点，不以缺行、`null` projection 或 adapter 默认值表达空对话。

`defaultRun` 是用户模式“当前故事后续回复”的持久默认值，可省略以支持专家逐 Turn选择。存在时创建事务先执行与 candidate 相同的 conversation input/reply output compatibility校验，并写 `ConversationRunProfile(revisionNo=1)`。更新 profile 使用 `UpdateConversationRunProfileCommand` 的 revision CAS和application receipt；profile尚不存在时调用方传 `expectedRevisionNo=0`并创建revision 1。它只影响之后新建的 Turn/Candidate，不修改已存在 run、历史 message或NodeInstance snapshot。Submit/Regenerate仍把实际 `ConversationRunSpec` 固定进 Candidate；profile不是历史执行权威，也不是permission边界。

## Canonical Message Append

每条角色消息在 WorkingContext 中只使用一个 canonical patch op：

```ts
type ConversationMessageAppendPatchV1 = StatePatch & {
  aggregateKind: "working_context"
  aggregateId: string       // Conversation.contextId
  lineageKey: string        // append target branch ID
  schemaVersion: 1
  ops: [{
    op: "append"
    path: "/messages"
    elementId: string       // exactly value.messageId
    value: ConversationContextMessageV1
  }]
}
```

User append 固定 `operationId=conversation-user-message:<messageId>`，`baseCommitId=SubmitConversationTurnCommand.expectedHeadCommitId`，`source=user_input`、`originRunId=null`。其 `parentMessageId` 是该 base commit ancestry 中最后一条可见角色消息的 ID；首条消息为 `null`。Assistant append 固定 `operationId=conversation-assistant-message:<messageId>`；正式 candidate 的首次投影使用 `baseCommitId=run.outputCommitId`，`append_after_current` resolution 则在 ancestry 校验后使用命令的 `expectedCurrentBranchHead`。两者都固定 `turnId=Candidate.turnId`、`parentMessageId=Turn.userMessageId`、`originRunId=Candidate.runId`、`source=run_output`。显式保存 partial 时使用 `source=saved_partial`，仍必须引用产生该 partial 的 run。`contentRef` 始终指向已校验、不可变的 canonical `LlmContentPartIr[]` object。

Patch validator 必须同时校验 `elementId == value.messageId`、message ID 未在目标历史中出现、role/source/nullability 组合和关联的 Turn/Candidate/run。相同 operation ID + 相同 canonical patch digest 是幂等重放；相同 operation ID 或 element ID 携带不同 value 必须冲突。

`ConversationContextV1.messages` 是 append-only collection。阶段一拒绝对 `/messages` 或其 descendant 使用 `add`、`replace` 或 `remove`，也不接受用 array index、`/-` 或另一个 path 绕过 canonical append；已提交 message 的 role、content、parent 和 provenance 永不原地改写。需要更正或保留 partial 时创建有明确 source 的新 message/commit，不能重写旧 commit。

`ConversationMessage` row 必须逐字段镜像 append value，且其 `commitId` 指向包含该唯一 append 的 commit。User append 的 message row、patch/commit、active branch head、materialized projection、Turn、`Conversation.activeHeadCommitId` 和 candidate/run 创建属于“提交用户消息”的同一事务。任何写入当前 active branch 的消息 append 都必须在同一事务 CAS 推进 Conversation active head，不能只更新 branch head。

Assistant candidate 通常写入尚未选择的 sibling branch：其 message row、patch/commit、candidate branch head、materialized projection 和 Candidate ready 字段在同一 projector 事务提交，同时明确保持 Conversation active pointer 不变。只有后续初次选择或 Swipe 的独立 CAS 事务才同时更新 `Conversation.activeBranchId/activeHeadCommitId`；不能因“message 与 active head 同事务”的规则误把未选择 candidate 提升为 active。

## 提交用户消息

普通用户输入永远先进入 WorkingContext：

```text
active commit H
-> append user message StatePatch
-> user commit U
-> create Turn
-> fork candidate branch from U
-> create GraphRun bound to candidate branch/U
```

阶段一这些操作通过 ConversationService 的一个高层原子存储命令完成：

1. 预先校验 graph revision、input contract 和 content size；
2. CAS `Conversation.activeHeadCommitId == expectedHead`；
3. 写 user content/message、StatePatch/commit U 和 Turn，并把当前 context branch head 与 `Conversation.activeHeadCommitId` 从 H CAS 推进到 U；
4. 创建 candidate context branch、GraphRun、entry instances 和 durable events；
5. commit 后唤醒 scheduler。

如果未来 Conversation 和 runtime 不在同一数据库，用 transactional outbox 替代跨库事务；不能采用“消息写了但 run 永远没创建且没有可恢复命令”的两步裸调用。

用户 message 一旦提交就不因 model failure 丢失。创建命令使用 idempotency key；重复投递返回同一 Turn/run。

## Candidate 隔离

每个 candidate 从同一个 user commit 创建 sibling context branch：

```text
                         -> branch A -> run A state -> assistant A commit
active -> user commit U  -> branch B -> run B state -> assistant B commit
                         -> branch C -> run C state -> assistant C commit
```

Router fan-out 仍在一个 candidate run/branch 内，不创建 candidate。

Run 中间的 planner、critic 或 summary 输出不自动写为 assistant message。只有 Candidate 固定的 `replyOutputKey` 被 ConversationService 读取并验证后，才创建角色回复。

## Run 完成投影

ConversationService 幂等消费 `run.completed`：

```text
读取 finalized reply output/ref
-> 校验 reply contract和安全上限
-> CAS candidate branch head == run.outputCommitId
-> 在该 commit 后追加 assistant message StatePatch
-> 创建 ConversationMessage
-> 更新 Candidate ready/candidateCommitId
```

Assistant commit 与 candidate/message 更新在同一事务，使用 run ID 作为幂等来源。GraphRun 可能已产生其他 WorkingContext commits；assistant projector 必须以 `run.outputCommitId` 为 expected head。若 branch 被其他写入推进则产生 conflict/人工协调，不能把回复静默附到非预期提交之后。

该 CAS 失败时把 Candidate 置为 `projection_conflicted` 并保存脱敏 `projectionErrorRef`；Run 仍是 completed，Candidate 不能伪装成 running/failed/ready。由于 branch head 不回退，协调必须使用 `ResolveCandidateProjectionCommand`，不是重做一次必然失败的旧 CAS。

`append_after_current` 要求 `run.outputCommitId` 在 `expectedCurrentBranchHead` ancestry 中，并在显示 diff/获得授权 reason 后，CAS 当前 head、把 assistant commit 显式追加到当前 head，再置 ready；它不重跑 GraphRun/LLM，并写独立 resolution audit。`abandon_projection` 只把 Candidate 置 `projection_abandoned`，不创建 assistant message。两者都幂等校验当前 head/command digest；head 又前进时返回新 conflict，不静默改挂载点。

Failed/cancelled run 将 candidate 标记为相应 terminal 状态，不创建正式 assistant message，也不移动 Conversation active head。Partial stream 只用于 UI；用户显式“保留部分回复”时，应用服务创建一条来源标记明确的新 message/commit，而不是把 ephemeral token 当 finalized run output。

## 初次选择与 Swipe

Conversation policy 默认自动选择该 Turn 的第一个成功 candidate，但仍需 CAS：

```text
selection 尚不存在
且 active head 仍为该 Turn 的 user commit
-> 写 selection
-> activeBranchId/head = candidate branch/candidate commit
```

`run.completed/failed/cancelled` 的 commit 后 notifier 只是降低延迟的 wake hint，不是 projector 可靠性边界。Conversation projector 在启动及有界周期扫描中查找 `nonterminal turn_candidates JOIN terminal graph_runs`，幂等补全每个 run 的 durable projection job；worker 通过 lease 领取，重放以 runId/terminal event seq 去重。因此 run terminal 事务后、assistant projection 前崩溃不会让 Candidate 永久留在 running。

Projector 结果分类固定：可重试 storage/lease 错误使 job 回 `pending` 并持久化有界 backoff；branch-head CAS 不匹配使 job/Candidate 置 `conflicted/projection_conflicted`；payload exact-schema、content/safety cap、permission/integrity 等永久校验失败使 job/Candidate 置 `failed/projection_failed` 并保存脱敏 `projectionErrorRef`，不自动重试。Run failed/cancelled 的 job 幂等更新 Candidate 对应状态后置 done；只有 completed 且成功创建 assistant commit/message 才置 ready/done。

后续 regenerate 不自动抢占现有选择。Swipe 是显式选择另一个 ready candidate：

```text
校验 candidate 属于同一 Turn并从同一 user commit 分叉
-> 写/替换 ConversationSelection
-> CAS 更新 Conversation active branch/head
```

若该 Turn 后面已有消息，切换旧 Turn 会让 active pointer 回到所选 candidate 的分支，后续 Turn 仍保留在旧 branch 历史中但不再位于 active ancestry。UI 必须提示这是历史分叉；系统不复制或静默重放后续消息。

下一条用户输入总是从当前 active head 追加。因此 branch ancestry，而不是 `createdAt`，决定当前可见对话。

## Regenerate

Regenerate 读取 Turn 的 `userCommitId`，创建新的 sibling branch、Candidate 和 GraphRun，不重复写 user message：

```ts
regenerate({
  turnId,
  run: { graphRevisionId, replyOutputKey, inputShape: "conversation_message_v1" },
  expectedUserCommitId,
  idempotencyKey
})
```

它可以选择新的 applied graph revision、channel/model 或 preset 当前版本。已有 candidate run 固定自己的 graph revision；已有 NodeInstance 恢复时使用已经 pin 的 preset/tool/channel execution snapshot，不能因 regenerate 修改旧 run。

## GraphRun Binding

Core 使用 `04-state-branching.md` 定义的 canonical `GraphRunContextBinding`。

RP 关联存放在 adapter-domain binding/projection：

```ts
type ConversationRunBinding = {
  runId: string
  conversationId: string
  turnId: string
  replyOutputKey: string
}
```

`ConversationRunBinding.replyOutputKey` 映射到 `turn_candidates.reply_output_key`，与 candidate/run 创建在同一事务写入。不要在 core `GraphRun` 结构中加入 `conversationId/userMessageRef` 等业务字段。Trace 通过 run binding、message commit、NodeInstance read set 和 event causation 追踪完整链路。

## Wait Response 与新 Turn

只有某个 NodeInstance 已创建 `human_response` WaitRecord 时，携带 `waitId + deliveryId` 的响应才恢复原 GraphRun。它通过响应 schema 校验并幂等满足 wait。

没有 wait ID 的普通用户输入不能注入旧 run，必须按“提交用户消息”流程创建新 Turn/GraphRun。这样两个独立语义输入不会共享 edge queue 或混淆 causal boundary。

## 版本规则

- GraphRun 创建时固定不可变 `GraphRevisionId + contentHash`。
- Context branch/commit 通过 CAS 变化，不改变 graph revision。
- NodeInstance 首次执行时固定实际 ContextPreset revision、tool registry version、channel config revision 和 semantic policy version；waiting/resume/retry 复用该 execution snapshot，并叠加当前 deny-only revocation overlay。
- 用户要使用新的 graph/preset 配置重新回答时，创建 regenerate run，不改写已执行历史。

## Invariants

- 每个 Turn 恰有一个 user message/commit，可以有零到多个 candidate。
- Candidate run/assistant message 在同一 sibling branch，failed/cancelled candidate 没有正式 assistant message。
- Selection 必须指向同 Turn 的 ready candidate。
- Conversation active head 必须可达于 active branch。
- User commit、Turn、candidate run 创建；assistant commit；selection 更新分别具有清楚的原子边界和幂等 key。
- 当前对话由 active branch ancestry 计算，不按所有 message 的时间戳拼接。
