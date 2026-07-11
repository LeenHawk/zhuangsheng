# Agentic 框架设计总览

## 核心设想

这个框架以有向图作为执行模型。图中的节点代表一次有语义边界的执行阶段，边代表节点 output 到 input 的连接。

最核心的节点是 `LLMNode`，也就是一次 LLM 驱动的语义步骤。除此之外，图中还可以包含输入节点、输出节点、记忆节点和路由节点。

图不应该表达每一个底层动作。图应该表达语义阶段、资源边界和状态流转。

## 基础节点类型

第一版可以保留较小的节点集合：

```text
InputNode        外部输入入口
LLMNode          一次 LLM 驱动的语义步骤
MemoryNode       必要时显式出现的记忆操作节点
RouterNode       条件路由节点
OutputNode       外部输出节点
```

`ToolNode` 不应该默认作为一等图节点。大多数工具调用应该是 `LLMNode` 内部的能力，而不是独立的工作流阶段。

但是，如果某个工具类操作需要独立生命周期、权限、重试、审计、补偿或恢复语义，它可以被提升为图节点。

## 图的职责

图负责：

- 声明每个节点可以访问哪些资源
- 定义节点 output/input 如何互联
- 通过 RouterNode 等显式节点表达控制流
- 管理运行状态、权限、审计日志和恢复
- 在需要时显式表达记忆读写
- 提供跨节点执行的 trace 能力

图应该关注资源和数据流，而不是每个底层模型调用或工具调用细节。

## 设计原则

判断一个操作应该放在哪里，可以用这个规则：

```text
如果触发时机、目标 scope 和输入数据都是确定的，放到 graph/runtime。

如果需要语义判断、歧义处理、冲突解决或主动探索，暴露为 LLM tool。

如果需要独立生命周期、权限边界、重试、人工 review 或恢复，提升为 graph node。
```

## 文档结构

- `01-nodes-and-tools.md`：节点、LLMNode 和工具边界
- `02-memory.md`：记忆系统、确定性读写、LLM memory tool 和 patch 模型
- `03-async-runtime.md`：异步图执行器、调度、join 和 loop
- `04-state-branching.md`：Memory 版本、中途截止、checkpoint、branch 和 merge
- `05-streaming-events.md`：流式事件、事件持久化、token 流和背压
- `06-persistent-versioning.md`：持久化节点的版本化、去重、checkpoint 和 compaction
- `07-llm-api-overview.md`：LLM API 交互总览、gproxy-protocol 和 API shape 边界
- `07-llm-ir.md`：LLMNode 统一 IR、provider extensions 和 shape adapter
- `07-llm-tool-loop.md`：LLMNode tool loop、流式聚合和工具 I/O
- `07-llm-channels-counting.md`：LLM 渠道、模型引用、计数和共享 crate 维护
- `08-context-assembly.md`：上下文装配引擎、Prompt item、预算、剪裁和预设兼容
- `09-minimal-scope.md`：阶段一核心范围，不按 minimal demo 设计
- `10-llm-node.md`：LLMNode 结构、Context Assembly、tool loop、streaming 和输出契约
- `11-graph-definition.md`：Graph definition、output/input edge、RouterNode 和 loop
- `12-secret-store.md`：单用户本地 Secret Store、主密码和 apiKeyRef 解析边界
- `13-conversation-turn-run.md`：Conversation、Turn、GraphRun、regenerate 和 swipe
- `14-router-node.md`：Router 规则 DSL、fan-out、Memory binding 和 loop guard
- `15-review-status.md`：人工审计完成的关键决策和待讨论清单
