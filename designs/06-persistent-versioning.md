# 版本化聚合与 Object Store

## 定位

持久化版本属于 WorkingContext、LongTermMemory 或 artifact metadata aggregate，不属于某个 graph `nodeId`。Graph node 可以产生变更，但 node definition 不是持久化数据的聚合根。

统一模型：

```text
immutable object store
+ append-only commit graph
+ branch-aware materialized projection
+ periodic VersionSnapshot
```

Runtime journal 和 RuntimeCheckpoint 另见 `05-streaming-events.md`；二者不能与 version commit/snapshot 混用。

## Content Object

大 JSON、patch、文本、tool result、provider response 和文件先写入 content-addressed object store：

```ts
type ContentObject = {
  id: string
  contentHash: string  // sha256:<lowercase hex>
  byteSize: number
  storageKind: "inline" | "filesystem"
  storageKey?: string
  createdAt: string
}
```

Hash 针对实际保存的 bytes。结构化 JSON 使用项目固定的 `canonical_json_v1`，不是 RFC 8785/JCS：对象 key 按 Unicode scalar sequence 排序，字符串以 UTF-8 和最小必要 JSON escape 编码并拒绝 unpaired surrogate，数组保序；number 解析为有界 exact base-10 `(sign, coefficient, exponent)`，零写成 `0`，非零写成 `[-]d[.digits]eN`，coefficient 去前导零、去尾随零时等量增加 exponent，且 exponent 无 `+`/前导零。它拒绝 NaN/Infinity，并与 `16-domain-consistency.md` 的 digit/exponent limits 共用 conformance vectors，因此 `1`、`1.0`、`10e-1` 产生相同 bytes且不会降精度到 binary64。结构化 owner/ref 把 `formatVersion` 与 object ID/hash 一起持久化；未知 format fail closed。读取时重新校验 size/hash；hash 不匹配视为存储损坏。

ContentObject 只声明 bytes/hash/size；media type、展示名与 classification 属于引用它的 Artifact/owner metadata。同一 bytes 可被不同 artifact 以各自经 policy 验证的 media type 引用，object dedup 不让首次写入者决定后续解释。

阶段一小对象可内联 SQLite，大对象放本地文件系统。文件流程：

```text
写随机 temp 文件 -> 限制大小并计算 hash -> fsync
-> 以 hash 原子 rename/deduplicate
-> 数据库事务创建 metadata/ref
```

Crash 产生的无引用对象由宽限期 GC 清理。路径只由 storage adapter 根据 hash 生成，不能使用上传文件名。Secret 永远不进入 object store。

## Commit Graph

Canonical commit：

```ts
type VersionCommit = {
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
  createdAt: string
}
```

`lineageKey` 对 WorkingContext 是 context branch ID，对阶段一 LongTermMemory 是 `global`。Root 无 parent，普通 commit 一个 parent，merge commit 两个 parent。

Commit、parent 和 patch 都 append-only。`operationId` 在 lineage 内唯一并用于提交重试去重。对外并发令牌使用 `commitId`；`sequenceNo` 只在相同 aggregate/lineage 内单调递增，不能作为全局 version ID。

Merge commit 必须保存已验证的最终 patch/ref，不能只保存“以后重新调用 reducer/LLM”的指令。历史 replay 使用记录时的 schema/policy/reducer version。

## Patch 策略

不同 aggregate 使用不同物理 delta，但共享 commit/CAS：

```text
WorkingContext JSON
  受限 RFC 6902 StatePatch；append 使用 operation ID。

LongTermMemory record
  版本化 content object/ref 或 status tombstone + reason/evidence proposal history；不修改 evidence object。

Artifact metadata
  小型结构化 patch；artifact bytes 创建新 ContentObject。

Append-only audit/event
  直接追加记录，不包装成 VersionCommit。
```

阶段一不实现 text diff、binary chunk delta 或 vector embedding 的多版本复制。长文本作为不可变 content object；变化版本可引用新 object，hash 自动去重完全相同内容。真实存储压力出现后再引入 chunk manifest。

## Branch-aware Projection

当前读取不应每次 replay patch：

```ts
type MaterializedProjection = {
  aggregateKind: string
  aggregateId: string
  lineageKey: string
  headCommitId: string
  projection: JsonValue | { objectRef: string }
  schemaVersion: number
  updatedAt: string
}
```

Key 必须包含 lineage/branch；不能为一个 aggregate 只保存单一 current view。Projection 是可更新缓存，不是每次提交产生的不可变 full snapshot。

读取当前值直接读 projection并验证 head。历史值使用最近 `VersionSnapshot + subsequent patches`。发现 projection head 与 branch head 不一致时停止写入该 aggregate，重建并记录 integrity error。

## VersionSnapshot

VersionSnapshot 是独立加速记录：

```ts
type VersionSnapshot = {
  commitId: string
  snapshotRef: string
  schemaVersion: number
  checksum: string
  createdAt: string
}
```

创建 snapshot 不修改历史 commit。策略可以按 patch 数、replay bytes 或重要业务 commit 触发；阶段一可先固定每 N 个 commit，并允许手动触发。

Snapshot 只回答“某个 commit 的聚合内容是什么”，不包含 NodeInstance、edge queue、wait、lease、effect 或 run cursor。GraphRun recovery 使用 RuntimeCheckpoint。

## 写入事务

预写 object 后，一次逻辑变更在一个数据库事务中：

```text
读取并 CAS expected head
-> 校验 patch schema/policy/read set
-> 插入 immutable commit + parents + object refs
-> 更新 branch/global head
-> 更新 materialized projection
-> 更新 origin node/proposal/merge 状态
-> 追加 durable audit/runtime event
-> commit
-> publish notifier
```

任一步失败不暴露 commit/head/projection 的部分结果。SQLite 使用短 `BEGIN IMMEDIATE` + conditional update；事务内禁止等待网络、LLM、tool 或大文件写入。

Node finalized 时，这个版本事务与 attempt completion、edge emission、run output 和 events 是同一外层数据库事务，详见 `16-domain-consistency.md`。

## 历史读取与 Schema 演进

- 每个 patch/snapshot/commit 携带 schema version。
- Reader 至少能解码所有仍受 retention 保护的历史版本。
- Migration 不重写 immutable commit；通过新 reader/upcaster 或显式 migration commit 演进。
- Upcaster 必须确定、无 I/O、版本化且可测试。
- Policy 变化不改变历史合法性；新写入使用新 policy，merge 时按目标当前 policy 重新校验。

## Memory 与 Evidence

LongTermMemory 拆为：

```text
immutable evidence objects/refs
memory record current projection
MemoryChangeProposal + status audit
version commits
```

Proposal apply 后才创建 memory commit并推进 global head。Commit 保存 `sourceProposalId`，proposal 保存 `appliedCommitId`，并在同一事务设置；`reason/evidenceRefs` 通过这条关系进入 audit/GC 链。删除或 obsolete 只改变 record 状态，不物理销毁仍被 evidence/audit 引用的内容。

Embedding 是派生 projection，key 至少包含 source content hash、embedding model ID 和 operation key。它可以重算，不必进入每个 semantic commit。

## Compaction

阶段一 compaction 只做安全的物理优化：

- 为旧 commit 增加 VersionSnapshot；
- 压缩 object bytes但保持解码后 bytes/hash 契约；
- 去重相同 content hash；
- 清理过期 staging/orphan object；
- 按 event policy 丢弃非关键 debug/token payload。

不得改写 commit ID、parent、author、origin、reason/evidence 或 merge 结果。阶段一不删除受保留保护的 patch/critical event。若未来截断 patch 链，必须先定义审计保留、snapshot attestation 和可验证 replay 边界。

## Reachability 与 GC

GC roots 至少包括：

- active、merged-retained 和 pinned branch heads及其 ancestors；
- live/retained lineage 的 current projections，以及仍在 retention 或被 pin 的 VersionSnapshots；
- RuntimeCheckpoints、durable event payload refs；
- Conversation messages/turn candidates/selections；
- pending/reviewed proposal、applied commit 关联的 proposal/evidence，以及仍在 audit retention 的 rejected proposal；
- effect request/result 和 outcome_unknown；
- artifact metadata、用户 pin 和 legal/audit hold。

使用 mark -> grace period -> fenced sweep。宽限到期后，GC 必须在数据库事务中锁定 object row、再次校验 roots/owner refs/lease，并 CAS `live -> deleting` 同时分配 delete fence；任何 owner transaction 只能引用 `live` object，遇到 deleting 必须失败/重试，不得在 GC 复核后插入新 ref。物理删除成功后再用同 fence CAS `deleting -> deleted`；失败保留 deleting 供 repair/retry，只有 storage 明确验证 bytes 完整时才可受控恢复 live。Abandoned branch 不等于不可达；只有 retention 到期且无任何 root 引用才能进入 deleting。

Inline bytes 可在 fenced 数据库事务中清除，外部文件在事务外按 fence 删除。Deleted tombstone 在有界 repair retention 内保留；若后续重新上传相同 hash，必须校验全部 bytes 并以新 lifecycle generation 原子 rehydrate 后才能创建 owner ref。Staging 和 internal-sensitive object 使用各自的等价 lifecycle/fence，不绕过该规则。

阶段一 SQLite maintenance 先实现最保守的 orphan sweep：只扫描超过宽限期、`lifecycle=live` 且没有 `content_object_refs` 的 inline object，并通过 SQLite FK metadata 再检查所有直接引用 `content_objects` 的列。发现“有 FK root 但缺 owner ref”时保留对象并告警，不猜测 owner 或删除；owner ref insert/update 由数据库 trigger 拒绝指向 deleting/deleted object。确认无 root 后在同一短事务内以 lifecycle generation 和随机 delete fence 完成 `live -> deleting -> deleted`，清空 inline bytes但保留 hash/size tombstone；相同完整 bytes 再次写入时以 CAS 增加 generation 并 rehydrate，而不是复用未经验证的空 tombstone。

## 阶段一边界

实现：SHA-256 整对象寻址、SQLite inline/filesystem store、commit parents、branch-aware projection、JSON StatePatch、VersionSnapshot、保守 mark-and-sweep。

延后：content-defined chunking、text diff、远程 S3、多租户加密/去重、跨 context merge、任意 reducer 和历史链物理截断。
