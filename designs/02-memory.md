# Memory Capability 与变更提案

## 定位

节点可以通过一个统一的读取门面获得持久化上下文，但持久化对象不因此变成同一个 aggregate。权威边界见 `16-domain-consistency.md`：

```text
ExecutionState    runtime 独占的调度状态，不暴露为 Memory
WorkingContext    context-local、可 branch 的业务上下文
LongTermMemory    MemoryManager 管理的长期事实/偏好/项目记忆
ArtifactObject    不可变大对象，Memory 中只保存 ArtifactRef
```

Conversation 是 WorkingContext 的一个领域视图，不是第四套存储。Secret 不是 Memory，也不能通过 MemoryReader 读取。

## 三层边界

```text
MemoryBinding
节点被授予哪些确定性 read/write/propose 能力。

Memory capability tools
模型需要语义检索或判断时可请求的高层能力。

MemoryManager
校验长期记忆 proposal、权限、evidence、冲突和审批的权威组件。
```

LLMNode、RouterNode 和 custom tool 都不能直接操作 SeaORM entity、数据库连接或底层 memory store。

## Memory Binding

节点定义只引用图中声明的逻辑 scope；Run binding 把逻辑 scope 解析到具体 aggregate/commit。节点激活时固定全部 `StaticMemoryRead` 的 read set/envelope，同一 NodeInstance 的普通 executor retry/resume 不悄悄切换这些已绑定值。例外只有 Router `validate_on_commit` 的有界 reconcile（重读其 Router bindings）和模型执行中才给出 query 的 `search_memory` call-level snapshot；两者都有独立 durable identity，不伪装成 activation 前已知的 static read。

```ts
type NodeMemoryBinding = {
  reads?: StaticMemoryRead[]
  workingWrites?: StaticContextWrite[]
}

type MemoryBinding = NodeMemoryBinding & {
  tools?: MemoryToolGrant[]
}

type StaticMemoryRead = {
  id: string
  as: string
  source:
    | { kind: "working_context"; scope: string; path: string }
    | { kind: "long_term_memory"; scope: string; query?: MemoryQuery }
    | { kind: "artifact"; scope: string; artifactRefFrom: PreExecutionValueSelector }
  required?: boolean
  consistency?: "snapshot" | "validate_on_commit"
  limit?: number
  maxBytes?: number
}

type PreExecutionValueSelector = {
  source: "input"
  sourceName: string
  selector: InputSelector
}

type FinalValueSelector = {
  source: "input" | "output" | "binding"
  sourceName: string
  selector: InputSelector
}

type MemoryQuery = {
  text: string
  tags?: string[]
  status?: "active" | "obsolete"
}

type MemoryToolGrant = {
  capability: "search_memory" | "propose_memory_change"
  scopes: string[]
  maxResults?: number
  maxProposalBytes?: number
}

type MemorySearchResult = {
  records: Array<{
    memoryId: string
    commitId: string
    contentHash: string
    summary: string
    evidenceRefs: string[]
  }>
  truncated: boolean
}

type BoundMemoryValue =
  | {
      kind: "working_context"
      found: boolean
      commitId: string
      value?: JsonValue
    }
  | {
      kind: "long_term_memory"
      records: MemorySearchResult["records"]
      truncated: boolean
    }
  | {
      kind: "artifact"
      found: boolean
      artifactRef?: ArtifactRef
    }

type BoundReadResult = {
  bindingId: string
  envelope: BoundMemoryValue
  envelopeDigest: string
  scopeSnapshotToken?: string
}
```

`consistency` 默认是 `snapshot`；只有 Router/节点决策明确要求提交前再次验证 read head 时才使用 `validate_on_commit`。`StaticMemoryRead.limit` 是 long-term read 的唯一结果数上限；draft 缺失时 Apply 补 workspace 的有界默认值（阶段一建议 20）并写入 applied revision。Working-context/artifact read 必须不配 `limit`，其基数由 path/ref 契约决定。

`path` 使用 RFC 6901 JSON Pointer。语义检索结果必须有稳定排序、上限和实际 record version/content hash；这些引用进入 NodeInstance read set，replay 不重新搜索并假设结果相同。

阶段一 `search_memory` 使用当前 LongTermMemory projection 的 SQLite FTS5 lexical ranking + 结构化 tag/status filters，score tie 按 memory ID；返回后固定 record commit/content hash。Static read 未给 query 时按 memory ID 读取 scope 内 active records。对 Context 的 `relevanceScoreMicros` 使用稳定结果 ordinal 映射，不持久化平台浮点 BM25 值。FTS index 是可重建 projection，不是 Memory 权威。Embedding/vector retrieval 可以使用 `07-llm-api-overview.md` 的 derived vectors，但高性能 vector index 延后。

读取结果一律使用上述 `BoundMemoryValue` envelope并按 binding `as` 暴露，`id` 用作稳定配置/trace 身份。Envelope 以 canonical JSON 整体持久化为 `BoundReadResult`，long-term records 保留 selection ordinal 和 `truncated`；retry/resume/replay 直接读该 envelope 并验证 digest，不重搜或从无序 record set 重建。`required` 默认 true：working/artifact missing 或 long-term 零结果使 attempt 在执行前失败；false 时返回 `found=false` 或 `records=[]`，永远不省略 alias。Long-term records 按查询稳定顺序取前 `StaticMemoryRead.limit` 个并用 `truncated` 标记；`maxBytes` 只允许丢弃尾部完整 records，单个 value/record 已超限则失败 `memory_read_too_large`，不能截成无效 JSON。Context Assembly 只能引用这些已授权结果，不能在 `ContextItem` 内绕过 binding 查库。

Long-term query 在读取事务中同时固定 scope revision 为 `scopeSnapshotToken`。`consistency=snapshot` 只保留该 token 供 trace；`validate_on_commit` 必须在 finalize 同时校验每个 selected record head 与 scope revision 仍不变，因此新增/删除/改状态的匹配 record（query phantom）也会使提交冲突。Working/artifact read 只验证已选 head/ref，不伪造 scope token。

## 确定性 WorkingContext 写入

触发时机、目标 path 和 value 来源均确定时，由 runtime 产生 `StatePatch`：

```ts
type StaticContextWrite = {
  id: string
  timing: "after_node_completed"
  targetScope: string
  path: string
  op: "add" | "replace" | "append" | "remove"
  valueFrom?: FinalValueSelector
}
```

`StaticContextWrite.id` 在节点内唯一，与 NodeInstance ID 都是有界 UTF-8 opaque ID；Apply 将 writes 的声明顺序和 ID 写入 revision。`add/replace/append` 必须有 `valueFrom`，`remove` 必须没有。Append 的 canonical `elementId = "sha256:" + lowercase_hex(SHA-256(UTF8("static-write/v1\0") || UTF8(nodeInstanceId) || 0x00 || UTF8(write.id)))`，因此 retry/replay 会去重而不同 activation 不会偶然合并；实现不得使用随机 ID、value hash 或 completion order。

阶段一 WorkingContext write 冲突固定返回 `state_conflict`。不支持 `create_proposal`；`MemoryChangeProposal` 只属于 LongTermMemory，不能被借用为未定义的 State proposal。未来若需 WorkingContext 人工解决，必须另行定义独立领域类型、存储和 API。

Pre-execution artifact read 只能从已经固定的 NodeInstance input 取得 ArtifactRef，不能引用本节点尚不存在的 output，也不支持 binding chaining。After-completion `FinalValueSelector` 才能从 finalized input/output/已解析 binding 做 JSON Pointer/JSONPath 选择。两者都不允许 JavaScript、网络调用或隐藏数据库读取；Apply 必须按 phase 校验 selector source。

一次 node completion 中的所有确定性写入组成一个 `StatePatch`，与 NodeInstance completion、edge emission 和 durable event 在同一事务提交。Patch 使用 `baseCommitId + operationId`；不能用脱离 branch 的裸版本数字。

Conversation 的用户消息和已选择 assistant message 由 Conversation service 通过同一 StatePatch/commit 路径追加，不需要让 LLM 调用低层 append 工具。

## Memory Capability Tools

只有需要语义判断的能力才暴露给模型。阶段一内建：

```text
search_memory
在明确 scope、filter 和 limit 内语义检索，返回有界摘要与 evidence refs；该动态 query 使用 durable call-level snapshot。

propose_memory_change
提出 create/replace_content/mark_obsolete/delete_tombstone，不直接推进长期记忆 head。
```

可以在后续增加 `compare_memories` 或有固定 reducer 的 `propose_memory_merge`，但不默认暴露以下低层 CRUD：

```text
readMemoryById / listAllMemory / updateMemory / deleteMemory
```

工具仍受 `19-tools-artifacts.md` 的 ToolDescriptor、ToolGrant、approval 和 effect 规则约束。工具返回 `MemoryChangeProposal` part；dispatcher 只能创建 proposal，不能直接 apply。

`search_memory` 是 MemoryManager 实现的内建 capability，custom executor 不获得底层 FTS/database port。完整 arguments/grant 验证后，runtime 在一个数据库读事务中固定当前 scope revision，执行查询，并在返回给模型前持久化 query、有序 result envelope、selected record commits/ordinals、truncated 和 scope token。同一 model batch 的多个 memory search 按 callIndex 在同一 read snapshot 解析并原子落记录。

该 toolCallId 的 retry/resume/replay 只读 durable result，不按新 current projection 重搜。后续 model call 新请求的 search 是新 logical tool call，可固定当时的新 scope revision；这是显式、可审计的 call-level read，而非无声修改 static/context snapshot。如果结果影响 proposal，selected commits 必须进入 evidence/expected-head 校验；它不绕过 WorkingContext StatePatch 的 base/read-set 校验。

## MemoryChangeProposal

统一类型定义见 `16-domain-consistency.md`。最小状态机：

```text
proposed
  -> awaiting_confirmation | awaiting_review
  -> approved | rejected
approved
  -> applied | conflicted
```

状态转换 append-only 审计，proposal 实体保存当前投影。每个 proposal 至少包含：

- 目标 memory record/scope 与 `expectedHeadCommitId`；
- discriminated change 和有界 content/change ref；
- 人类可检查的 `reason`；
- `evidenceRefs`；
- requester、origin run/node 和 idempotency key；
- policy/schema version。

不保存 LLM 自报的数字 `confidence`。可信度由 evidence、来源、冲突检查和 policy 决定。

审批后 apply 时必须重新校验当前 head、权限、schema 和 evidence；审批不等于跳过 CAS。Head 已变化时进入 `conflicted`，不能静默 LWW。Conflict resolution 是新的、可审计 proposal 或用户确定 patch。

重启后 `awaiting_confirmation/awaiting_review` 保持可继续。若 proposal 在 LLMNode 内要求当前 run 等待，把一个或多个 proposal IDs 写入该 durable WaitRecord 的 `wait_blockers`；proposal 可通过 join index 反查 wait，但二者不是同一实体。逐 proposal decision 全部 terminal 后才 resolve wait。

## Scope 与 Policy

逻辑 scope 由 workspace/run binding 解析，不允许节点提交任意字符串访问全局数据：

```ts
type MemoryGrant = {
  readableScopes: string[]
  workingWritePaths: string[]
  proposalScopes: string[]
  artifactReadScopes: string[]
  maxSearchResults: number
  maxReadBytes: number
}
```

有效权限是以下集合的交集：

```text
workspace/run policy
∩ graph revision grant
∩ node binding
∩ tool descriptor requirements
∩ 当前 actor permission
```

常见 policy：

- WorkingContext：允许受限 path 的频繁确定性写入；
- Conversation messages：append-only，message id/operation id 去重；
- User profile/preferences：结构化 schema，通常需要确认；
- Project/long-term facts：新记录通过 create proposal，整份替换、obsolete 和 tombstone 需要 evidence；
- Artifact：bytes 不可变，只改变 branch-local ref 或 metadata commit。

默认拒绝未声明 scope/path。权限错误不回显其他 scope 是否存在，也不把敏感内容写入 event。

## Conflict 与 Branch 隔离

WorkingContext 使用 context branch。Router fan-out 只是一个 GraphRun 内的数据流，不创建 context branch。

并发 WorkingContext patch：

```text
head == base                  -> apply
head changed + paths disjoint -> 按统一 merge policy rebase/apply
append-only + operationId     -> 去重后稳定追加
overlapping change            -> conflict
```

具体 commit/merge 算法见 `16-domain-consistency.md`。阶段一不提供 LWW、任意 reducer 或 LLM 自动冲突决策。

Speculative/candidate branch 创建的 LongTermMemory proposal 可以保留审计，但只有显式 promote/apply 才改变 global head。Failed/cancelled run 的 WorkingContext commit 不会自动推进 Conversation active head。

## MemoryReader 契约

Core 可以提供统一只读门面：

```ts
type MemoryReader = {
  readBound(bindingId: string): Promise<BoundMemoryValue>
  searchBound(bindingId: string, query: MemoryQuery): Promise<MemorySearchResult>
  openArtifact(bindingId: string, ref: ArtifactRef): Promise<BoundedArtifactReader>
}
```

Reader 已绑定 snapshot/grant；调用方不能自行传 aggregate ID 绕过权限。返回大内容使用 ref/stream，不把整份 bytes 复制进 NodeInstance JSON。

Memory search 是可观察外部步骤：记录 query hash、实际 record refs、policy 和 token/size limit；默认不在普通 event 中保存完整 query/result。

## 常见执行流程

```text
激活 NodeInstance并固定 context commit/read set
-> 执行 deterministic reads
-> Context Assembly 消费 bound results
-> LLMNode 可调用 search/propose tools
-> finalized output 生成 StatePatch 或 MemoryChangeProposal
-> MemoryManager 校验
-> completion transaction 提交 commit/proposal/event
-> 需要审批时创建 wait，恢复后继续同一 durable continuation
```

Memory capability 让模型在明确边界内使用持久化上下文；它不是把数据库变成 prompt 的通用后门。
