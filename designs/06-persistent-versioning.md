# 持久化节点版本化

## 问题

持久化节点需要支持版本化、回滚、审计、分支和恢复。

但版本化不能简单实现为每个版本复制一份完整数据。否则如果有 10 个版本，体积可能膨胀 10 倍。

目标是实现逻辑版本化，而不是全量复制版本化。

## 核心模型

推荐模型：

```text
当前物化视图 + 增量 patch log + 周期性 checkpoint
```

也就是：

```text
base snapshot
  + delta#1
  + delta#2
  + delta#3
  = version N
```

而不是：

```text
version1 full copy
version2 full copy
version3 full copy
```

持久化节点可以拆成三层：

```text
Object Store
保存实际内容，按 content hash 去重。

Version Log
保存版本提交记录，只保存 patch、引用和元信息。

Materialized View
保存当前版本的快速读取视图。
```

## Object Store

大内容不直接嵌入版本记录，而是进入 object store。

```ts
type PersistentObject = {
  id: string
  contentHash: string
  contentRef: string
  size: number
  createdAt: string
}
```

如果两个版本引用同一段内容，它们应该指向相同 `contentHash`，而不是存两份。

示例：

```text
version 1:
  refs: [hash_a, hash_b, hash_c]

version 2:
  refs: [hash_a, hash_b, hash_d]
```

只有 `hash_d` 是新增存储。

## Version Commit

版本记录应该保存提交关系、patch 引用和 snapshot 引用。

```ts
type VersionCommit = {
  id: string
  nodeId: string
  parentCommitId?: string
  version: number
  patchRef?: string
  snapshotRef?: string
  createdBy: "llm" | "user" | "system"
  createdAt: string
}
```

当前状态记录只需要指向 head commit 和当前物化视图。

```ts
type PersistentNodeState = {
  nodeId: string
  headCommitId: string
  currentSnapshotRef: string
  updatedAt: string
}
```

## Patch，而不是 Full Copy

不同数据类型应该使用不同 patch 策略。

结构化数据可以用 JSON Patch 或 JSON Merge Patch：

```json
[
  { "op": "replace", "path": "/title", "value": "new title" },
  { "op": "add", "path": "/facts/3", "value": "..." }
]
```

文本可以用 text diff：

```text
base text + text diff = new text
```

大文件或 artifact 可以用 content-addressed chunks：

```text
chunk A
chunk B
chunk C
```

新版本只引用变化的 chunk。

## Checkpoint

如果一直只存 patch，读取历史版本时可能需要 replay 很长的 patch 链。

因此需要周期性 checkpoint。

```text
snapshot@0
  delta 1
  delta 2
  ...
snapshot@50
  delta 51
  ...
snapshot@100
```

checkpoint 策略可以是：

```text
每 N 个版本做一次 snapshot
或者 delta 总大小超过 snapshot 大小的某个比例后做 snapshot
或者重要状态节点完成后做 snapshot
```

示例配置：

```ts
type CompactionPolicy = {
  snapshotEveryVersions?: number
  snapshotWhenDeltaRatioExceeds?: number
  keepRecentFullSnapshots?: number
}
```

## 当前版本读取

读取当前版本不应该每次 replay patch。

runtime 应该维护当前物化视图：

```text
current view = 最新可读状态
commit log = 可追溯历史
```

读取当前状态直接读 `currentSnapshotRef` 或数据库当前行。

只有读取历史版本时，才使用：

```text
nearest checkpoint + subsequent patches
```

## 写入流程

一次持久化节点写入可以按以下流程执行：

```text
1. 读取当前 headCommitId
2. 基于当前状态生成 patch
3. 校验 patch base 是否等于当前 head
4. 写入 patch object
5. 写入 VersionCommit
6. 更新 materialized current view
7. 更新 headCommitId
```

如果发生并发冲突：

```text
baseCommit != currentHead
  -> reject
  -> merge
  -> rebase patch
  -> fork branch
```

具体策略由节点类型和 state path policy 决定。

## 按数据类型选择策略

不要所有 persistent node 都使用同一种版本化机制。

```text
Append-only log
只追加，不需要 diff。适合 conversation log、trace、event log。

Structured state
使用 JSON Patch。适合 user profile、project state、配置。

Text document
使用 text diff、piece table 或 snapshot + diff。适合 summary、draft、文档。

Large artifact
使用 content-addressed chunks。适合文件、长输出、二进制 artifact。

Vector memory
embedding 通常可以重算，版本记录 metadata、source ref 和 content hash，不一定存多份 embedding。
```

## Memory 场景

Memory 可以拆成三部分：

```text
原始 evidence
append-only，按 hash 去重，不改。

memory view
当前可检索视图，允许物化更新。

memory edit history
保存 patch、reason、evidenceRefs。
```

示例：

```ts
type MemoryRecord = {
  id: string
  currentVersion: number
  currentContentRef: string
  evidenceRefs: string[]
  status: "active" | "obsolete" | "merged"
}
```

```ts
type MemoryVersion = {
  memoryId: string
  version: number
  parentVersion: number
  patchRef: string
  reason: string
  evidenceRefs: string[]
}
```

大内容放在 `contentRef`，版本表只保存 patch 和引用。

## Compaction

长期运行后，patch log 和历史对象需要压缩。

Compaction 可以做：

```text
合并旧 patch 为 checkpoint snapshot
删除不可达 branch 的临时对象
对重复 content 做 hash 去重
压缩旧 token/debug event
保留关键 audit event
```

Compaction 不能破坏审计要求。对于需要完整审计的 memory 或用户数据，可以只压缩物理存储，不删除关键元信息。

## Branch 与版本化

Branch 不复制全量状态，只从某个 commit fork。

```text
main:    commit0 -> commit1 -> commit2
                          \
branchA:                   -> commit3a -> commit4a
```

每个 branch 只是新的 commit 链。

大内容仍然通过 object store 去重。

这样 branch 很便宜，不会因为创建多个分支导致状态成倍膨胀。

## 总结

持久化节点应该像 Git、MVCC 和 materialized view 的混合。

```text
1. 当前状态物化，保证读快
2. 历史版本记录 patch，不复制全量
3. 大对象用 content hash 去重
4. 周期性 checkpoint，避免 replay 过长
5. 不同数据类型使用不同 delta 策略
6. branch 和 merge 基于 commit graph，而不是拷贝状态
```

一句话：

```text
版本化的是提交关系和增量变化，不是每个版本的一整份完整数据。
```
