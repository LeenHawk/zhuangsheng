# Conversation、Turn 与 GraphRun

## 定位

RP 默认采用每次用户输入创建一个 `GraphRun` 的模型，而不是让一个 run 永久代表整段对话。

```text
Conversation
长期容器，包含对话 Memory 和分支。

Turn
一次用户输入以及基于它生成的一个或多个回复候选。

GraphRun
一次图执行尝试，通常生成一个回复候选。
```

Core runtime 不绑定 Conversation。Conversation 和 Turn 属于 RP adapter/domain；普通工作流仍可独立创建 GraphRun。

## 关系

一个 Conversation 包含多个 Turn，一个 Turn 可以对应多个 GraphRun：

```text
Conversation
  Turn 1
    GraphRun A -> candidate A
    GraphRun B -> candidate B (regenerate)
  Turn 2
    GraphRun C -> candidate A
```

GraphRun 内仍可执行任意多个 NodeInstance、tool loop、Router 循环和 repeated firing。

## User Message

用户消息必须先写入 branch-aware working memory，再创建 GraphRun。

```text
memory@10
  -> memory@11 (user message)
  -> GraphRun
```

这样模型调用失败或进程崩溃时用户输入不会丢失，regenerate 也可以重复从同一个 user-message memory version 开始。

GraphRun 可以接收 `userMessageRef`，不需要复制完整对话历史。Context Assembly 从 working memory 读取实际消息。

## GraphRun Binding

RP 创建的 GraphRun 可以记录可选领域引用：

```ts
type GraphRunBinding = {
  conversationId?: ConversationId
  turnId?: TurnId
  branchId: BranchId
  baseMemoryVersion: number
  userMessageRef?: MemoryRef
}
```

这些字段用于关联和恢复，不让 core runtime 理解 Conversation 业务规则。

## Assistant Candidate

中间 LLMNode 不自动写入 conversation。一个图可能包含 classifier、planner、actor、critic 和 summary，只有被指定的最终 output 才是角色回复。

```text
GraphRun completed
  -> Conversation Service 读取 outputs.reply
  -> 提交 assistant candidate memory version
```

Conversation Service 属于 RP adapter/domain，负责决定哪个 OutputNode 或 run output 成为 assistant message。

failed/cancelled run 不创建正式 assistant candidate。Streaming partial output 只用于 UI；用户显式选择保留时，Conversation Service 才提交部分回复。

## Regenerate And Swipe

多个候选从同一个 user-message memory version 分叉：

```text
                 -> memory@12a (candidate A)
memory@11(user)  -> memory@12b (candidate B)
                 -> memory@12c (candidate C)
```

Regenerate 从 `memory@11` 创建新的 GraphRun。

Swipe 只切换 Conversation 的 active head，不重新执行图：

```text
activeHead = memory@12b
```

下一条用户消息从当前 active head 继续。

Memory version tree 和 Message records 是 source of truth。Turn 只是方便 UI 查询和候选管理的索引，不保存另一份聊天历史。

```ts
type ConversationTurn = {
  id: TurnId
  conversationId: ConversationId
  userMessageId: MessageId
  candidateRunIds: RunId[]
  selectedCandidateRunId?: RunId
}
```

## Graph And Preset Versions

一个 GraphRun 固定一个 graph revision。运行中不能切换拓扑版本，因为 node、port 和持久化 edge queues 都基于该 revision。

用户修改 graph 后，新的 revision 从下一次 GraphRun 生效。需要立即使用时，取消当前 run，并从同一个 base memory version 创建 regenerate run。

Preset 独立版本化。每个 NodeInstance 开始时默认读取 preset 最新版本；恢复时可以由用户选择历史 preset 版本。

## Causal Boundary

GraphRun 本身是 causal boundary，不额外引入 Cause 实体或 `causeId`。

Waiting resume、approval、webhook、timer 和 tool callback 如果是某个 NodeInstance 请求的结果，就通过 wait/node/event 引用继续原 GraphRun。

新的独立语义用户输入不注入旧 run，而是先更新 Conversation working memory，再创建新的 GraphRun。不同输入因此由 `runId` 和各自的 edge queues 天然隔离。

NodeInstance input refs、producer instance、event parent id、wait id 和 memory patch author 足以追踪 run 内因果链。

## Summary

```text
Conversation 跨多个 GraphRun。
一次用户输入通常创建一个 GraphRun。
一个 Turn 可以有多个 candidate GraphRun。
用户消息先持久化，最终回复由 Conversation Service 提交。
Regenerate 创建 sibling memory branch，Swipe 选择 active head。
GraphRun 固定 graph revision，Preset 默认使用执行时最新版。
```
