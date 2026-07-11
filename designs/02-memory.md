# 记忆系统设计

## 总体模型

Memory 既是图级资源，也是 LLM 可访问的受控能力。

本项目把所有可持久化、可读取、可版本化的上下文统一称为 Memory。

不要把 `state`、`conversation`、`artifact` 和 `memory` 设计成四套并列 reader。它们应该是 Memory 的不同 scope、lifecycle 或 storage class。

基础边界：

```text
Edge token
节点之间传递的逻辑临时数据流，用于调度和本次执行输入。未消费值为了 crash recovery 会持久化在 run 的 edge queue 中，但不属于 Memory。

Memory
所有持久化上下文，节点执行时按权限和配置读取。
```

Memory 的推荐 lifecycle：

```text
working
当前 conversation/run branch 内的可变上下文，可以跨多个 GraphRun 延续。
conversation、scene、flags、scratch 都属于 working memory。

long_term
跨 run 保留的长期上下文。
角色卡、世界书、用户偏好、长期事实属于 long_term memory。

artifact
大对象或 blob 类上下文。
文件、图片、长文本、tool 大输出属于 artifact memory。
```

`conversation` 不是独立顶层系统，而是 working memory 里的一个 domain。这样 swipe、regenerate、回溯和 branch 都能自然作用在对话历史上。

RP 中 Conversation 的 working memory 跨多个 GraphRun 存在。每个用户消息先形成 memory version，候选回复从该版本分叉；具体关系见 `13-conversation-turn-run.md`。

节点执行上下文可以简化为：

```ts
type NodeExecutionContext = {
  inputs: Record<string, JsonValue>
  memory: MemoryReader
}
```

`inputs` 来自 edge token。`memory` 负责读取 working、long_term 和 artifact scopes。

可以分为三层：

```text
Graph-level memory operation
由 runtime 或显式图节点执行的确定性读写。

LLM-level memory tool
由 LLM 主动请求的语义化记忆操作。

Memory manager
负责校验、应用、审计和冲突处理的权威组件。
```

LLM 不应该直接修改底层 memory store。它应该通过受控 API 请求记忆操作。

```text
LLMNode
  -> Memory Capability API
      -> Policy / Validator / Conflict Resolver
          -> Memory Store
```

## 确定性记忆操作

简单读和简单 append 不应该暴露给 LLM。如果 runtime 可以确定性完成，就应该由 graph/runtime 处理。

适合确定性处理的例子：

- 读取当前用户画像
- 读取当前项目上下文
- 读取最近对话历史
- append 用户输入
- append 最终输出
- append run trace
- append 结构化节点输出

这些操作可以通过 memory binding 或 runtime hook 表达。

```ts
type MemoryBinding = {
  reads?: StaticMemoryRead[]
  writes?: StaticMemoryWrite[]
  tools?: MemoryToolGrant[]
}
```

示例：

```ts
{
  nodeId: "answer_llm",
  memory: {
    reads: [
      {
        scope: "user_profile",
        mode: "inject",
        as: "userProfile"
      },
      {
        scope: "project_memory",
        query: "$input.projectId",
        mode: "inject",
        as: "projectContext"
      }
    ],
    writes: [
      {
        timing: "after_node",
        scope: "conversation_log",
        value: "$node.output",
        mode: "append"
      }
    ],
    tools: [
      {
        name: "search_long_term_memory",
        scopes: ["user_memory", "project_memory"]
      },
      {
        name: "propose_memory_edit",
        scopes: ["working_memory"]
      }
    ]
  }
}
```

## LLM Memory Tools

只有需要语义判断的操作才应该暴露给 LLM。

适合暴露的能力：

- 语义化 memory search
- 判断某件事是否值得长期记忆
- 提议编辑已有记忆
- 合并相关记忆
- 标记记忆过时
- 检测冲突
- 为偏好或事实补充 scope

不要默认暴露低层 CRUD：

```text
appendMemory
readMemoryById
listMemory
updateMemory
deleteMemory
```

这些工具会增加 LLM 噪音，而且很多操作本来可以由 runtime 确定性完成。

更合适的是语义化工具：

```ts
type MemoryTools = {
  searchMemory(query, scope, filters)
  appendMemory(content, type, tags)
  proposeMemoryEdit(memoryId, patch, reason)
  mergeMemories(sourceIds, mergedContent, reason)
  markMemoryObsolete(memoryId, reason)
}
```

对于长期或敏感记忆，应该优先使用 `proposeMemoryEdit`，而不是直接 `editMemory`。

## Memory Patch

复杂记忆更新应该表示为 patch 或 event，而不是直接覆盖。

```ts
type MemoryPatch = {
  op: "append" | "replace" | "merge" | "delete" | "mark_obsolete"
  targetIds?: string[]
  content?: string
  diff?: unknown
  reason: string
  evidenceRefs?: string[]
}
```

不引入 `confidence` 数字字段。LLM 自报的置信度数值不可靠，校验应依赖 `reason` 和 `evidenceRefs` 这两个可检查的字段。

每次变更尝试都应该产生可审计事件。

```ts
type MemoryEvent = {
  id: string
  runId: string
  nodeId: string
  actor: "llm" | "user" | "system"
  patch: MemoryPatch
  status: "proposed" | "applied" | "rejected"
  createdAt: string
}
```

这样可以支持回滚、审计、冲突解决和多 agent 并发。

## Memory Scope

不同记忆类型应该有不同的变更规则。

```text
Working Memory
可以频繁 edit，适合作为当前任务草稿板。

Conversation Log
基本 append-only，用于记录交互历史。

Project Memory
可以 append，也可以在有证据和 scope 的情况下 edit。

User Profile / Preferences
应该结构化，只有高置信度或用户确认后才更新。

Long-term Semantic Memory
尽量 event-sourced，不直接覆盖。
```

## 权限与校验

每个 `LLMNode` 应该显式获得 memory capability。

```ts
type NodeMemoryPermission = {
  readableScopes: string[]
  writableScopes: string[]
  editableScopes: string[]
  tools: string[]
}
```

示例策略：

```text
ResearchLLM 可以读 project memory，但不能编辑 user preferences。
ReflectionLLM 可以提出长期记忆更新。
ProfileManagerLLM 可以在严格校验下更新结构化用户画像字段。
WorkerLLM 只能写 working memory。
```

Memory manager 在应用变更前应该校验：

- 权限
- schema
- target 是否存在
- evidence 是否足够
- 是否与已有记忆冲突
- 是否需要用户确认

可能结果：

```text
applied
rejected
requires_confirmation
requires_review
```

## 推荐执行模式

常见流程：

```text
Input
  -> 确定性 memory read / context assembly
  -> MainLLMNode with selected tools
  -> RouterNode
  -> OutputNode
  -> Optional MemoryReflectionLLM
  -> 确定性 memory write
```

任务过程中，runtime 自动注入简单 memory context，LLM 只在必要时调用语义化 memory tools。

任务结束后，可以用专门的 reflection 节点判断哪些内容值得进入长期记忆，再由 memory manager 校验和应用。
