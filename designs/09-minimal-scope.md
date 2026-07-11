# 阶段一核心范围

## 目标

阶段一目标不是做 minimal demo，而是实现能支撑真实 agentic graph 的核心闭环：

```text
异步图执行
LLMNode 内部 tool loop
确定性 memory binding
语义化 memory tool
patch-based state
event stream
interrupt/resume
基础 branch
多输入多输出
```

## 应该实现

阶段一应该实现：

- 持久化 `GraphRun`
- `NodeInstance`
- 多 `InputNode` / 多 `OutputNode`
- 多 `isEntry` 入口节点
- 异步节点执行
- Event log
- Critical event streaming
- Memory patch 和 memory version
- Checkpoint
- Soft interrupt
- 从 waiting 或 interrupted 状态 resume
- 基础 branch fork
- append-only path 和 selected output 的基础 branch merge
- Router decision
- Router fan-out
- `all` 和 `any` join
- Loop limit
- LLMNode 内部工具调用 trace
- 确定性 memory reads/writes
- `proposeMemoryEdit` 形式的记忆编辑提案
- 标准 LLM API shape 调用边界
- compact 能力边界，包括 OpenAI compact operation 和 Claude compact tool 映射
- 基础 Context Assembly

## 可以延后

这些能力可以延后，但不能影响阶段一真实图运行能力：

- 分布式 worker
- quorum/window 等高级 join
- 完整 CRDT state
- branch rebase
- 任意自动冲突 merge
- 自动补偿事务
- 动态改图
- LLM 自动解决所有 merge conflict
- 所有 token 的长期持久化
- 应用层跨 provider 协议转换
- 完整 SillyTavern 行为兼容

## 推荐实现顺序

建议顺序：

```text
1. Graph definition
2. GraphRun / NodeInstance
3. Async scheduler
4. Event log
5. State patch
6. LLM API client boundary
7. 基础 Context Assembly
8. LLMNode executor
9. RouterNode
10. Memory binding
11. Streaming API
12. Interrupt / resume
13. Checkpoint
14. Branch fork / basic merge
```

## Runtime Summary

图执行器应该是一个持久化、事件驱动的异步调度系统。

节点是异步执行实例。边决定调度。持久化上下文变化通过版本化 memory patch 完成。interrupt 只停止继续调度，不销毁进度。branch 从 memory version fork。streaming events 暴露完整执行过程，但只有语义化 memory patch 会改变 working memory。
