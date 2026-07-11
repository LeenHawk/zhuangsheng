# Agentic Role Play 与框架设计总览

## 产品定位

本项目面向 Agentic Role Play：用户通过角色、Persona、世界、故事、候选、记忆和剧情分支进行沉浸式交互；角色背后可以运行具备工具、等待、恢复和长期状态的 Agent Graph。默认前端是Role Play用户模式，专家模式才直接暴露Graph、Run、Trace和配置细节。

技术核心是以 LLM 调用为主要语义节点的异步有向图 runtime。它不是 prompt 拼接器、LLM 反代、普通聊天壳或只执行一次拓扑排序的 DAG 库。

```text
GraphDefinition（不可变 applied revision）
  -> GraphRun（持久化因果边界）
      -> NodeInstance / NodeAttempt
      -> FIFO edge queues
      -> State/context commits
      -> Durable runtime journal
      -> finalized run outputs
```

产品目标是让角色与世界能够持续、可解释、可分支和可恢复；runtime目标是让异步执行、工具循环、等待/恢复、版本化上下文、branch、可恢复副作用和流式观察在同一个一致模型中工作。产品表面与runtime边界分别见 `24-agentic-role-play-ui.md` 和本文后续章节。

## 架构边界

```text
Web UI     -> Axum adapter  ---+
Tauri UI   -> Tauri adapter ---+-> Core service ports
Worker/Test -> RuntimeService -+       |
                                      +-> RuntimeService
                                      +-> narrow application services
                                      +-> storage/LLM/tool/object ports
```

Core 不依赖 Axum、Tauri、SeaORM entity、HTTP DTO 或 UI 类型。RuntimeService 只承载 run/control/query；Graph、Conversation、Memory、Artifact、Secret 等窄 application services 组合对应领域事务。两种 adapter 调用同一组 core service ports，只做认证、校验、协议/DTO 转换和流控，不承载 scheduler、branch 或 memory 业务逻辑。

LLM API 层只调用 OpenAI/Claude/Gemini 的标准 wire shape；跨 provider 转换属于上游 gproxy，不进入应用 runtime。

## 四个持久化领域

统一读取门面不等于统一写模型：

```text
ExecutionState
run、instance、attempt、queue、wait、lease、control、effect。

WorkingContext
跨 GraphRun 延续、可 branch 的 conversation/scene/flags/scratch。

LongTermMemory
由 MemoryManager 审批和版本化的事实、偏好、项目记忆。

ArtifactObject
content-addressed 不可变大对象；其他领域只保存 ref。
```

权威边界、commit 和事务见 `16-domain-consistency.md`。Secret 不属于以上任何数据域，只在 provider client 发送边界短暂解析。

## 节点模型

阶段一节点集合：

```text
InputNode        从 immutable run input 产生入口 value
LLMNode          context assembly、model calls、tool loop、final output
RouterNode       安全确定性 DSL、fan-out 和业务 loop guard
OutputNode       提交命名 run output
MergeNode        任意一路按 durable arrival order 合流
JoinByKeyNode    按显式 scalar key 配对
AggregatorNode   按数量/持久 timer 形成有界批次
ExpandNode       把数组显式展开为多次 emission
```

普通多输入节点使用 all-input FIFO zip。确定性 memory read/write 通过 node binding/runtime hook 完成，阶段一不保留语义模糊的通用 `MemoryNode`。

工具默认是 LLMNode capability。只有具备独立生命周期、权限、人工审批、审计、补偿或恢复边界的业务操作才提升为 graph node。

## 图与 Runtime 的职责

GraphDefinition 只声明：

- 节点、显式 ports、consumer selector 和 schema；
- output port 到 input port 的静态 edge；
- node 的 model/context/tool/memory grants 和 limits；
- required run outputs。

Runtime 负责：

- 持久化 run input、queue、activation、attempt 和 output；
- 调度、retry、wait、interrupt、cancel、deadline 和 recovery；
- state commit、branch head CAS、event 和 effect ledger；
- 在同一事务提交 node completion 的所有逻辑结果。

Edge 不承载 condition、mapper、join policy 或 state patch。控制流属于 Router/协调节点；字段投影属于消费者 binding；状态变更属于 StatePatch/MemoryManager。

## 核心不变量

1. 一个 GraphRun 固定一个 applied GraphRevision；已有 run 不受 draft 或新 revision 影响。
2. NodeInstance 表示一次 activation；NodeAttempt 表示该 activation 的一次执行尝试。
3. Readiness 检查、FIFO 消费、activationSeq 分配和实例创建是一个原子操作。
4. Node finalized 时，patch/commit、output、edge emission、attempt 状态和 durable events 同事务提交。
5. Notifier、内存 ready queue 和 live token 都只是加速/观察层，丢失不影响恢复。
6. Waiting 只发生在 executor 可持久化 continuation 的安全边界；任意 Rust Future 不可直接恢复。
7. 外部 effect 先记账；非幂等未知结果必须人工协调，不能盲目重试。
8. Router fan-out 不创建 context branch；branch 由应用/runtime API 显式 fork。
9. Conversation candidate 隔离，failed/cancelled run 不推进 active head。
10. Secret 不进入 graph、run input、state、memory、event、trace、artifact 或 node output。
11. 所有 schema 使用同一 JSON Schema 2020-12 closed profile，并在发布时编译、hash、持久化 exact payload。
12. OperationKey 只能按 graph/channel/snapshot 固定的 taxonomy/adapter decoder version 解码；未知版本 fail closed。

## 判断能力放置位置

```text
触发时机、scope、输入和结果都确定
  -> graph/runtime binding 或确定性节点。

需要语义判断、搜索、歧义处理或提案
  -> 明确授权的 LLM tool。

需要独立生命周期、权限、审批、retry、audit 或恢复
  -> graph node 或 durable external effect。
```

## 文档导航

```text
01  node/tool 概念边界        12  Secret Store
02  Memory capability         13  Conversation/Turn/Candidate
03  async scheduler            14  Router DSL
04  context version/branch     15  设计审计状态
05  journal/streaming          16  领域一致性与原子提交
06  version/object store       17  runtime control/recovery
07* LLM API/IR/tool/channel    18  coordination nodes
08  Context Assembly           19  tools/artifacts
09  阶段一范围                 20  SeaORM/SQLite schema
10  LLMNode                    21  core/adapters API
11  graph definition           22  实现蓝图与验收
                                23  前端与可视化架构
                                24  Agentic Role Play 产品 UI
                                25  页面与交互规格
                                26  视觉与组件系统
```

`15-review-status.md` 记录冻结决策、明确延后项和重新打开设计的条件。`22-implementation-blueprint.md` 是进入编码后的顺序与验收入口。
