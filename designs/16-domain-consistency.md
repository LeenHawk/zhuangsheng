# 领域一致性与恢复边界

## 定位

本文统一 `State`、`Memory`、版本、分支和恢复术语，是 `02`、`04`、`05`、`06` 的一致性约束。底层可以复用 object store、version log 和 projection，但领域权限不能因复用存储而合并。

阶段一优先保证单机 SQLite 下可恢复、可审计；多租户、分布式调度和任意自动 merge 延后。

## 四个领域边界

### ExecutionState

`ExecutionState` 是一次 GraphRun 的调度事实：run 状态、NodeInstance/attempt、edge queue、wait、loop guard、lease、interrupt/cancel epoch 和 effect ledger。

- 生命周期以 `runId` 为边界，由 runtime 独占写入。
- 不属于 Memory，也不通过 `MemoryReader` 暴露给 LLM。
- durable runtime journal 是其恢复历史；运行表是快速调度 projection。
- GraphRun 固定 `graphRevisionId + contentHash`，恢复不能切换拓扑。

### WorkingContext

`WorkingContext` 是可跨多个 GraphRun 延续、可 branch 的业务上下文，例如 conversation、scene、flags 和 scratch。

- 聚合根是 `contextId`，branch 属于 context，不属于 run。
- 每次 GraphRun 绑定一个 `branchId` 和一个 `inputCommitId`，完成后可产生 `outputCommitId`。
- 用户消息先提交到 WorkingContext，再从该 commit 创建 GraphRun。
- 普通工作流也可创建临时 context；core runtime 不需要理解 Conversation/Turn。

### LongTermMemory

`LongTermMemory` 是跨 context/run 的受控事实、偏好和项目记忆，由 `MemoryManager` 管理。

- LLM 只能提交 `MemoryChangeProposal`，不能直接改 store。
- speculative context branch 中的 proposal 不自动进入全局 memory head。
- apply 时重新校验权限、schema、evidence 和 expected head。
- 阶段一使用线性 head；长期记忆 branch、自动语义 merge 延后。

### ArtifactObject

`ArtifactObject` 是按 content hash 寻址的不可变内容，例如文件、图片、长文本和大型 tool output。

- version、message、event 和 memory 只保存引用。
- 修改内容会创建新对象；branch 只分叉引用，不复制 bytes。
- artifact metadata 可版本化，但 object bytes 不原地覆盖。
- Secret 不是 ArtifactObject，不能进入 object store、event、patch 或 LLM context。

## Canonical JsonSchemaSpec

Graph port、run input/output、LLM structured output、tool arguments/config 和 wait response 共用唯一的 schema 契约；其他文档中的 schema 字段都引用这里的 `JsonSchemaSpec`，不能各自定义“JSON Schema 子集”。

```ts
type JsonSchemaSpec = {
  schemaVersion: 1
  dialect: "https://json-schema.org/draft/2020-12/schema"
  validationProfileVersion: 1
  formatPolicyVersion: 1
  document: JsonValue
  limits: JsonSchemaLimits
}

type JsonSchemaLimits = {
  maxSchemaBytes: number
  maxSchemaNodes: number
  maxSchemaDepth: number
  maxLocalRefs: number
  maxRefDepth: number
  maxRegexBytes: number
  maxInstanceBytes: number
  maxInstanceDepth: number
  maxCollectionItems: number
  maxStringBytes: number
  maxNumberDigits: number
  maxNumberExponentMagnitude: number
  maxValidationErrors: number
  validationFuel: number
}

type JsonSchemaCompilation = {
  canonicalDocumentHash: ContentHash
  schemaHash: ContentHash
  compilerId: string
  compilerVersion: string
  payloadFormatVersion: number
  canonicalSchemaObjectId: string
  compiledPayloadObjectId: string
  compiledPayloadHash: ContentHash
}
```

`document` 只能是 object schema 或 boolean schema；其中 `$schema` 若存在必须与 `dialect` 完全一致。阶段一基线 hard cap 依次为 256 KiB schema、4096 schema nodes、128 schema depth、1024 local refs、64 ref depth、4 KiB 单个 regex、16 MiB instance、128 instance depth、100000 个单层 collection members、8 MiB 单个 string、128 个 number significant digits、1024 绝对 exponent、32 validation errors 和 1000000 validation fuel。workspace、Graph `RunLimits`、tool/wait owner 可以进一步收紧；发布时把各层最小值写回 spec，不能在执行时读取更宽松的“当前默认值”。所有 limit 必须为正。

Profile v1 为每次 keyword evaluation、subschema branch、local-ref traversal、collection member visit 和 bounded regex/`uniqueItems` work unit 扣减确定性 fuel；任一路径耗尽即返回 `schema_validation_limit_exceeded`，不能把未完成验证当成功。具体权重与遍历顺序属于 profile conformance vectors，不能由 adapter 自行选择。

阶段一使用 closed keyword profile。允许：

- core/local reference：`$schema`、`$defs`、`$ref`；`$ref` 只能是 `#` 或以 `#/$defs/` 开头的 RFC 6901 fragment，target 必须在同一 document；
- assertion/applicator：`type`、`enum`、`const`、数值与字符串 bounds、`pattern`、`format`、object/array keywords、`allOf/anyOf/oneOf/not`、`if/then/else`、`dependentRequired/dependentSchemas`；
- annotation：`title`、`description`、`default`、`examples`、`deprecated`、`readOnly`、`writeOnly`、`$comment`。

“数值与字符串 bounds、object/array keywords”具体包括 draft 2020-12 的 `multipleOf/minimum/maximum/exclusiveMinimum/exclusiveMaximum/minLength/maxLength`、`properties/required/additionalProperties/patternProperties/propertyNames/minProperties/maxProperties`、`prefixItems/items/contains/minContains/maxContains/minItems/maxItems/uniqueItems`。Compiler 对上下文未知 keyword 一律报错，避免拼错约束被标准的 annotation 行为静默忽略。

阶段一禁止 `$id`、`$anchor`、`$dynamicAnchor`、`$dynamicRef`、旧版 recursive ref、`$vocabulary`、remote/non-fragment URI ref、`unevaluatedProperties/unevaluatedItems`、`contentEncoding/contentMediaType/contentSchema` 和自定义 meta-schema/vocabulary。Local recursive `$ref` 只有在每次循环都推进 instance location 时才允许；无进展 ref cycle 在编译期拒绝，其余递归仍受 instance depth、ref depth 和 fuel 限制。Regex 使用 profile 固定的线性时间安全子集；backreference、look-around 或 compiler 不支持的 Unicode 语义直接拒绝。

`format` 是 assertion，不是可忽略 annotation。`formatPolicyVersion=1` 只支持 `date-time/date/time/duration/email/hostname/ipv4/ipv6/uuid/uri/uri-reference/json-pointer`，并由所有 adapter/runtime 共用的版本化 conformance vectors 固定边界；未知 format 在发布时失败。未来增加或改变 format 必须提升 policy version。

Schema 和 instance number 都必须是有限 JSON number，禁止 NaN/Infinity。Digit/exponent cap 同时约束原始 lexeme 和规范化后的 coefficient/exponent，不能用 `0e999999` 或冗长零绕过 parser limit。解析器在 cap 内用精确十进制有理语义处理 `integer`、range 和 `multipleOf`，不能依赖平台二进制浮点 rounding；`06-persistent-versioning.md` 的 `canonical_json_v1` 将等值 number 规范成同一表示且不收窄为 binary64。Validator 不做 string/boolean/number coercion，不插入 `default`，不删除 unknown property，也不因 `readOnly/writeOnly` 改写值；成功或失败都保持输入值不变。

Graph Apply、ToolDescriptor publish 和 WaitRecord 创建使用同一流水线：先用不可放宽的 baseline parser limits 读取 envelope，再校验并 pin 更紧的 effective limits、dialect/closed profile，解析全部 local refs并编译 regex/format。`canonicalDocumentHash` 是 `(schemaVersion,dialect,validationProfileVersion,formatPolicyVersion,document)` 的 canonical hash，不含 owner-specific effective limits，用作 exact document/compatibility identity；`schemaHash` 是完整 canonical `JsonSchemaSpec` 的 hash，包含 effective limits，用作执行与恢复身份。随后持久化 canonical full source 与不可变 compiled payload；同一 compiler/payload format 对同一 spec 的 payload bytes/hash 必须跨支持平台确定并进入 conformance vectors。Owner revision/descriptor/wait 保存 `JsonSchemaCompilation`；bundle 按 `(schemaHash,compilerId,compilerVersion,payloadFormatVersion,compiledPayloadHash)` 去重并排序。Owner content hash/digest 覆盖该 tuple，不覆盖 object ID，并在 owner retention 期内把 source/payload refs 注册为 GC roots。任一步失败都不能发布 owner 或创建 open wait。

系统版本化 contract（例如 `AssistantReplyPayloadV1`）发布 canonical document hash 和逐字段 canonical limit caps。Owner 只能在 document hash 完全相同且每个 effective limit 都 `<=` 对应 canonical cap 时声明 exact compatibility；更宽 limit、缺失/不同 profile 或不同 document 都拒绝，不能用 schema subsumption 猜测兼容。运行时仍按 owner 的完整 `schemaHash`/compiled payload 校验实例，因此 compatibility identity 不会放宽资源边界或替代执行身份。

加载、恢复和校验前从 canonical source 重算并验证 `canonicalDocumentHash/schemaHash`，同时验证 compiled payload hash。未知 `schemaVersion`、profile/format policy、compiler 或 payload format 必须 fail closed 为 typed compatibility error，不能退回“尽量校验”、忽略 keyword或用当前 compiler 静默重编历史 payload。升级通过显式兼容 reader或创建新 immutable owner；错误列表、branch exploration 和 regex/fuel 消耗也必须有界并按 profile conformance tests 一致。

## 统一变更模型

确定性的 WorkingContext 或 artifact metadata 变更使用 `StatePatch`：

```ts
type ActorRef = {
  kind: "user" | "system" | "node" | "tool" | "application"
  id?: string
}

type JsonPatchOp =
  | { op: "add" | "replace" | "test"; path: string; value: JsonValue }
  | { op: "remove"; path: string }
  | { op: "append"; path: string; elementId: string; value: JsonValue }

type StatePatch = {
  aggregateKind: "working_context" | "artifact_metadata"
  aggregateId: string
  lineageKey: string
  baseCommitId: string
  operationId: string
  ops: JsonPatchOp[]
  schemaVersion: number
  policyVersion: number
  author: ActorRef
}
```

WorkingContext 的 `aggregateId` 是 context ID、`lineageKey` 是 branch ID；阶段一 artifact metadata 的 lineage 固定为 `global`，branch-local 可见性只由 WorkingContext 中的 ArtifactRef 表达。ExecutionState 的 scheduler 转换不使用 StatePatch，而写 durable runtime journal 和对应 projection。

阶段一长期记忆内容和变更是闭合、版本化类型：

```ts
type LongTermMemoryContentV1 = {
  schemaVersion: 1
  text: string
  tags: string[]
  attributes: Record<string, JsonValue>
}

type MemoryProposalChange =
  | { type: "create"; contentRef: string }
  | { type: "replace_content"; contentRef: string }
  | { type: "mark_obsolete" }
  | { type: "delete_tombstone" }

type MemoryChangeProposal = {
  id: string
  memoryId: string
  expectedHeadCommitId?: string
  change: MemoryProposalChange
  reason: string
  evidenceRefs: string[]
  requestedBy: ActorRef
  idempotencyKey: string
  schemaVersion: number
  policyVersion: number
  originRunId?: string
  originNodeInstanceId?: string
  appliedCommitId?: string
  status: "proposed" | "awaiting_confirmation" | "awaiting_review"
        | "approved" | "rejected" | "applied" | "conflicted"
}
```

`contentRef` 指向不可变 canonical `LongTermMemoryContentV1` object；`tags` 按 Unicode code point 排序去重，缺失 attributes 在 proposal validation 时规范化为 `{}`，text/collection/JSON depth/bytes 均有 workspace hard limit。`create` 的 memoryId 由 MemoryManager 预留随机 opaque ID，不接受模型/调用方自选 ID；该 record 无 head 且 `expectedHeadCommitId` 缺失。其他三类必须携带当前 expected head。`replace_content` 只对 active record 生效，`mark_obsolete` 只允许 active → obsolete，`delete_tombstone` 允许 active/obsolete → deleted。阶段一没有文本 append、任意 merge/reducer 或隐式 revive；“追加一条记忆”表达为创建新 memory record。Create proposal 被拒绝/过期时把无 head record 置 `discarded`，memoryId 永不复用；其 content/proposal/evidence 按 audit retention 回收。

MemoryManager apply 把 change 编译为 canonical `LongTermMemoryProjectionV1 { status, contentRef? }`：`create` 产生 initial snapshot/root commit，`replace_content` 产生只替换 contentRef 的普通 patch，obsolete 产生 status patch，tombstone 产生 status=deleted 并移除当前 contentRef 的 patch。旧 content 仍可由 commit ancestry 审计，但 deleted projection 不可读/搜索。Proposal change object、最终 snapshot/patch 和 commit 都带 schema version，replay 不重新解释自然语言 reason。

两者最终都产生不可变 commit：

```ts
type Commit = {
  id: string
  aggregateKind: "working_context" | "long_term_memory" | "artifact_metadata"
  aggregateId: string
  lineageKey: string
  sequenceNo: number
  operationId: string
  parentCommitIds: string[]
  patchRef?: string
  snapshotRef?: string
  mergeResolutionRef?: string
  schemaVersion: number
  policyVersion: number
  author: ActorRef
  originRunId?: string
  originNodeInstanceId?: string
  sourceProposalId?: string
  createdAt: string
}
```

`lineageKey` 对 WorkingContext 是 branch ID，对阶段一 LongTermMemory 和 artifact metadata 是 `global`；artifact 的 branch-local 可见性由 WorkingContext 中的 ArtifactRef 表达。根 commit 可以没有 parent，普通 commit 必须有一个 parent，merge commit 必须有两个 parent。API/CAS 一律使用 `commitId`；`sequenceNo` 只作 lineage 内展示/稳定排序，不能单独定位版本。`operationId` 在 lineage 内唯一，用于提交重试去重。根可保存初始 snapshot，普通提交保存 patch，merge 必须保存已验证的最终 patch/ref；resolution 另外保存 provenance。

LongTermMemory proposal apply 时，commit `sourceProposalId` 与 proposal `appliedCommitId` 在同一事务互相校验/写入，`operationId = memory-proposal:<proposalId>`。这样 reason/evidence/proposal status 与最终 commit 之间有不可丢失的审计和 GC 链。

长期记忆不使用 WorkingContext 的 path-rebase/merge；expected head 不匹配就进入 `conflicted`，解决时提交基于新 head 的新 proposal。

确定性 runtime hook 也必须生成 StatePatch 并经过同一校验/提交路径。`MemoryManager` 是 proposal 的权威；runtime 只编排等待和应用结果。

## Read Set

节点读取多个 scope 时不能用单一 `inputMemoryVersion` 表示：

```ts
type ReadSetEntry = {
  aggregateKind: string
  aggregateId: string
  lineageKey: string
  commitId: string
  bindingId: string
  selectionOrdinal?: number
  contentHash?: string
  consistency: "snapshot" | "validate_on_commit"
}
```

ReadSet 中的每个 selected long-term record 带唯一 `selectionOrdinal`；有序整体结果、零结果、truncation 和 query scope token 由 `02-memory.md` 的 durable `BoundReadResult` 表达。两者必须互相验证，不能只存一组无序 commit IDs。

NodeInstance/attempt 记录完整 read set，输出记录 commit IDs。语义 search 还记录实际选中的 memory record version/content hash；replay 使用这些引用，不重新搜索。写目标始终 CAS；`validate_on_commit` 的纯读依赖也必须确认 head 未变化。

## Context Branch

```ts
type ContextBranch = {
  id: string
  contextId: string
  parentBranchId?: string
  forkCommitId: string
  headCommitId: string
  status: "active" | "merged" | "abandoned"
}
```

GraphRun 只引用 branch。regenerate 从同一 user-message commit 创建 sibling run；swipe 只更新 Conversation 的 active branch/head projection。branch 状态更新使用 expected head CAS。

scope 规则：WorkingContext branch-local；ArtifactObject 全局不可变但引用 branch-local；LongTermMemory proposal 可源于 branch，但只有显式 apply/promote 才更新全局 head。

## Merge MVP

阶段一只 merge WorkingContext，并执行三方 merge：merge base、source head、target head。

允许自动处理：

- append-only collection：按稳定 `operationId` 去重，并按 branch ID、branch-local sequence、operation ID 的稳定键合并；
- 两侧修改互不相交的 JSON Pointer path；
- 两侧最终值完全相同；
- final candidate、artifact ref 等由用户显式选择。

其他情况产生持久化 conflict，不移动 target head。解决后创建双 parent merge commit，并在同一事务 CAS target head、更新 projection、标记 branch、追加 journal event。阶段一不支持 LWW、任意 reducer 或 LLM 自动决定；LLM 只能提出待校验 resolution。

## 三种持久化权威

```text
version log
WorkingContext、LongTermMemory 和 artifact metadata 的不可变语义历史。

durable runtime journal
ExecutionState 的调度转换、恢复和 audit 历史；引用 commit/object，不复制其内容。

materialized projection
当前 branch head、当前 memory view 和调度表；只为读写效率，可由前两者重建。
```

Object store 保存不可变 payload。对外 stream 是 durable journal 加 ephemeral observation 的投影，不是第四份权威。只有 durable 事件参与恢复；token/partial object 不得改变状态。没有 run 的 branch/memory 操作以 version log 为权威，并在同事务追加 domain audit/outbox event，不伪造 runId。

## 原子提交协议

一次 node finalized transition 必须遵守：

```text
1. 先写 content-addressed object；未被引用的对象允许后续 GC。
2. 开数据库事务，检查 run epoch、lease、expected branch head/read set。
3. 校验并写 patch、commit，CAS branch head，更新 materialized projection。
4. 同事务 finalize NodeInstance/attempt，写入 output edge queue，更新 wait/effect/run；input queue 只在 firing 时消费。
5. 同事务追加 durable journal/outbox，并由存储层分配 run-local seq。
6. commit 后才 publish；失败则所有逻辑引用均不可见。
```

long-term proposal apply、branch merge 和 Conversation candidate selection 使用同样的 CAS/outbox 规则。网络或文件副作用不能放进数据库事务，必须走 effect ledger。

## 两种快照

`RuntimeCheckpoint` 是一致的执行切面，至少记录 `runId`、branch/head commit、graph revision、`throughSeq`、scheduler projection ref、wait/timer、edge queue、node attempt、control epoch、effect watermark、schema version 和 checksum。恢复从 `throughSeq + 1` 重放 durable journal；旧 running attempt 不会被假定仍在执行。

`VersionSnapshot` 是某个 commit 的物化内容，用于缩短 patch replay/compaction，不包含 scheduler 状态，也不能单独恢复 GraphRun。两种快照都引用底层 content-addressed object store，不必暴露为用户 Artifact，也不在每个版本复制全量状态。

## Effect Ledger

每次外部 model/count/tool 调用先创建 NodeInstance-owned 的稳定 logical call/effect，再为每次实际调用创建绑定 invoking NodeAttempt 的 EffectAttempt：

```ts
type EffectOwner =
  | { kind: "model_call"; modelCallId: string }
  | { kind: "count_call"; countCallId: string }
  | { kind: "tool_call"; toolCallId: string }
```

```text
effect:  pending -> succeeded | failed | outcome_unknown
         outcome_unknown -> succeeded | pending | abandoned_unknown
         pending -> cancelled_before_start | abandoned_unknown (run terminal only)
attempt: prepared -> started | superseded_before_start
         started -> succeeded | failed | outcome_unknown
```

Logical call/effect 记录 NodeInstance、originating attempt、分类 `pure | idempotent | non_idempotent`、idempotency key 和 retry policy；EffectAttempt 记录 `invokingNodeAttemptId`、provider request ID、request/result refs，实际发送/finalize 只由该 invocation 的 fence/control epoch 授权。调用成功后先持久化 result，再把它应用到 node transition。

`prepared -> started` 是发送前必需 CAS；旧 invocation fence 被回收时，recovery 可先 CAS `prepared -> superseded_before_start`，证明未发送。Run 仍非终态时原子把 owner/checkpoint 置 retry_ready、由新 NodeAttempt 创建新 effect attempt；run hard-cancel/failure 则以 logical owner inventory收敛全部空窗：pending 的 prepared/retry-ready effect置 `cancelled_before_start`，unresolved started/unknown置 factual outcome_unknown + logical `abandoned_unknown`，尚无 effect 的 awaiting-approval tool也取消并 abort blocker。System EffectResolution 只关联尚无 resolution的真实 attempt；已有人工结论或无 effect时由 terminal journal/blocker decision审计。Started 与 superseded 互斥。非终态恢复中，已 started 的 pure/idempotent 可按策略重试，non-idempotent 且无法查询的 `outcome_unknown` 必须等待人工协调。

每个 effect 恰好绑定一个 `EffectOwner`，owner ID 指向同一 NodeInstance 的唯一 logical row；model/count/tool 三类之外没有隐式 owner。人工 resolution 只适用需协调的 model/tool owner，pure count 自动重试/使用持久化 fallback；resolution 是新的不可变事实，不覆写仍为 `outcome_unknown` 的 EffectAttempt。Effect、resolution、normalized owner row、checkpoint、wait blocker、run journal/outbox 必须在同一事务转换；resume 后才把 owner/checkpoint 从 retry-ready 推进为 prepared 并创建新 attempt。Interrupt/cancel 使用 run epoch 拒绝或隔离 late completion，不能让旧 attempt 推进当前 branch。

## GC 与保留

可达根至少包括：active/retained branch heads、保留期内的 VersionSnapshot/RuntimeCheckpoint、durable event refs、Conversation message/candidate、pending/review 中 proposal/evidence、applied commit 关联的 proposal/evidence、仍在 audit retention 的 rejected proposal、audit retention 内的 effect request/result/resolution/decision/evidence 和用户 pin。标记过程从这些根遍历 commit parent、patch、content 和 evidence 引用；parent 是边，不是独立根。

GC 使用 mark-and-sweep 与宽限期；最终复核和新 owner ref 通过 `06-persistent-versioning.md` 的 `live -> deleting -> deleted` lifecycle/delete fence 线性化，不存在“复核后又新增 ref”的窗口。abandoned branch 只有超过 retention 且不受 audit/legal hold 时才可回收。compaction 通过增加 VersionSnapshot 或打包物理对象缩短读取，不修改 commit、parent 或 patch 的语义记录。

## 阶段边界

阶段一：单 context branch tree、commit/read set/CAS、有限 merge、runtime journal、两种快照、effect ledger、保守 GC。

延后：跨 context merge、长期记忆 branch、任意 reducer、LLM 自动 merge、多租户保留策略、分布式 worker 协调和在线跨库事务。
