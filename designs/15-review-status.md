# 设计审计状态

## 定位

本文件记录已经由用户逐项确认的关键设计决策，以及尚未完成讨论的部分。

`reviewed` 表示核心边界和方向已经人工确认，不表示文档中的每个实现细节都已最终冻结。

## Reviewed

截至当前设计基线，以下关键决策已完成人工审计：

- Rust core runtime 与 Tauri/Axum adapter 边界
- Tool 默认是 LLMNode capability，不默认作为图节点
- `gproxy-protocol` 与 `gproxy-tokenize` 的复用边界
- Channel、OperationKey 和 model ref 的最小结构
- Context Assembly 是 LLMNode 内部阶段，Preset 独立版本化
- Preset 默认读取最新版，恢复历史版本是用户可选行为
- Memory 统一 working、long-term 和 artifact 持久化上下文
- Conversation 是 working memory domain，不是独立存储系统
- Graph edge 只表达 output port 到 input port 的连接
- Edge 不承载 condition、mapper、join 或 memory patch
- Input selector 位于消费者侧，支持 RFC 6901 JSON Pointer
- Input selector 支持 RFC 9535 JSONPath `one` / `many`
- Graph runtime 使用持久化 FIFO finalized-output queue 与 firing 语义
- 同一 node + branch 默认串行，但节点可以在一个 run 中反复 firing
- 普通多输入按 FIFO 位置配对，复杂汇聚使用显式节点
- LLMNode 默认 text 裸字符串输出，JSON 模式是 opt-in
- 字段投影不放在生产者 output contract 或 edge 上
- RouterNode 集中负责规则 DSL、路径选择、fan-out 和 loop guard
- Router DSL 以安全 CEL 语义为目标，不在 edge 上表达条件
- 不引入独立 LoopNode
- Conversation 跨多个 GraphRun，一次用户输入通常创建一个 run
- Regenerate 创建 sibling candidate run，Swipe 选择 active memory head
- 一个 GraphRun 固定一个 graph revision
- GraphRun 本身是 causal boundary，不引入 Cause 实体或 `causeId`
- Waiting/Resume 作为通用 runtime capability 保留，新的语义输入通常创建新 run
- Secret Store 当前按单用户、本地优先、主密码不持久化设计

## Pending Review

以下部分尚未完成人工审计，不能视为最终设计：

1. Waiting / Resume 最小持久化模型
2. InputNode 入口与 Run Input 契约
3. Retry 与 attempt 语义
4. Timeout / Interrupt / Cancel 语义
5. FIFO Edge Queue 存储与事务边界
6. Merge / JoinByKey / Aggregator / Expand 节点
7. Memory scope、version、patch 与 conflict
8. Conversation / Message / Turn / Candidate schema
9. ContextPreset 内容、RP 组装与兼容边界
10. Router DSL 实现、类型与安全限制
11. Tool Registry、权限、失败与副作用
12. Artifact Memory 存储与生命周期
13. Event Log schema、replay 与 compaction
14. SeaORM schema、事务和 migration

## Review Rule

后续每完成一项讨论：

```text
更新对应设计文档
-> 从 Pending Review 移入 Reviewed
-> 做相关残留和冲突检查
-> 再进入下一项
```

实现阶段如果发现需要改变 reviewed 决策，应重新讨论并更新本文件，不能静默偏离。
