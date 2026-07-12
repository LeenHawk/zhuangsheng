# 阶段一实现范围

## 目标

阶段一不是内存 DAG demo，而是 SQLite-first、单进程、可持久恢复的 agentic graph runtime。它必须能在真实 LLM/tool 调用、进程崩溃、等待、重试和用户分支下保持一致结果。

```text
applied graph revision
-> durable run/input/FIFO scheduling
-> LLMNode tool loop
-> state/context commit
-> durable event stream
-> interrupt/wait/resume/recovery
-> finalized outputs
```

## 必须实现

### Graph

- Draft 与不可变 applied `GraphRevisionId + contentHash`
- 显式 input/output port、consumer selector 和 JSON Schema
- 多 InputNode、多 OutputNode；入口仅零入边 InputNode
- InputNode 从 immutable RunInputRef 读取，OutputNode 支持 `single/append`
- LLMNode、RouterNode、MergeNode、JoinByKeyNode、AggregatorNode、ExpandNode
- 普通多输入 all/FIFO zip；MergeNode 表达 any
- 静态 port/output/cycle/SCC/limit/权限校验

### Runtime

- 持久化 GraphRun、NodeInstance activation、NodeAttempt
- 原子 `activate_if_ready`、FIFO edge queue、广播和 repeated firing
- 同一 `(run,node)` 默认串行，不同节点异步并发
- lease、fencing token、durable wakeup 和周期扫描
- completion 中 output/StatePatch/edge/event 的共同事务
- run completion、stranded values、required outputs 和结构化错误

### Control 与 Recovery

- 有限 RetryPolicy、持久 backoff/timer
- durable WaitRecord、continuation、幂等 response 与 resume
- soft interrupt draining、resume、hard cancel 与 late result isolation
- node/run deadline、global activation/pending/tool limits
- effect ledger 和 `outcome_unknown` 人工协调
- RuntimeCheckpoint + durable journal replay/reconciliation

### State、Memory 与 Branch

- WorkingContext `StatePatch + commit + branch-aware projection`
- 完整 read set、head CAS、非重叠 rebase、重叠 conflict
- Context branch fork；append-only/不相交/显式选择的有限三方 merge
- LongTermMemory search 与 `MemoryChangeProposal` 审批状态机
- branch-local artifact refs；失败 candidate 不推进 active head
- VersionSnapshot、content-addressed objects 和保守 GC roots

### LLM、Tool 与 Context

- OpenAI Responses/Chat Completions、Claude Messages、Gemini GenerateContent 的标准 shape adapter
- models/count，以及显式 image/embedding/compact service boundary；后者不伪装成隐藏 Context 操作
- 有序 LLM transcript IR、stream finalizer、usage/error 映射
- 多轮 tool loop、多个 tool call、顺序稳定回填和 loop checkpoint
- Tool Registry、显式 grant、hosted tool binding、approval 和副作用分类
- `search_memory` / `propose_memory_change` 是固定名称、固定 schema、受 pinned memory grant 约束的内建 capability；search 结果和 proposal 审批结果都作为有序 ToolResult 回填同一 LLM loop
- ContextPreset revision、snapshot、受信角色/provenance、确定性预算与安全 preview
- provider count；失败时使用许可证兼容、版本固定的 local tokenizer，否则明确标记为 `estimate`
- text output；opt-in JSON output + schema validation

### Storage、Events 与 Adapter

- SeaORM migration、SQLite WAL、短写事务和 repository contract
- Critical runtime journal 与 run-local durable sequence
- Ephemeral token delta、final semantic event、SSE cursor 重连和背压
- 本地 content object/artifact staging、hash、retention 和下载权限
- 单用户 Secret Store、provider-client 内 credential 注入和全链路脱敏
- Core RuntimeService；Axum HTTP commands + SSE
- Tauri/WebSocket adapter 可以随后接入，但不得改变 core 语义

### Conversation Domain

- Conversation/Message/Turn/Candidate/Selection schema
- 用户消息、Turn 和首个 candidate run 的原子创建
- regenerate sibling branch、candidate isolation、swipe active head
- 普通 user input 与 wait response 的严格区分
- Web Agentic Role Play 用户闭环：故事、角色/世界设置投影、消息/候选、approval、memory与branch
- 专家模式最小 Graph/Run/Trace诊断；两种模式共享领域对象，不建立第二份执行配置

## 明确延后

- 分布式 worker、leader election、跨区域恢复
- PostgreSQL 生产部署优化、多用户/多租户/RLS
- dynamic graph mutation、插件 ABI、运行中切换 graph revision
- per-node 高并发实例、priority/fairness scheduler
- quorum/latest/session window、arbitrary reducer
- CRDT、branch rebase、跨 context merge、任意自动 conflict merge
- LLM 自动决定 merge、自动补偿事务
- 长期记忆 branch 和自动全局 promotion
- content-defined chunking、远程 object store、vector index 优化
- 所有 token 的长期持久化和完整 provider raw response 保留
- 完整 SillyTavern 历史行为兼容
- Tauri 内嵌 Axum server
- 同一 model response 内把 memory capability 与 custom tool 混合编排，或把 search 与 proposal 混在同一 batch；阶段一要求同一 memory batch capability 同质并在违反时 fail closed，多个同质调用仍按 call order 执行/审批
- desktop/mobile/web 完整功能对等、离线体验和最终视觉打磨

## 实现顺序

以 `22-implementation-blueprint.md` 的里程碑为准：

```text
M1 graph + storage skeleton
M2 FIFO runtime + event stream
M3 control + coordination + recovery
M4 Context + LLMNode + tools + secrets/artifacts
M5 State/Memory + Conversation/branch
M6 adapters + end-to-end hardening
F1–F5 Role Play Web vertical + expert diagnostics + Tauri/mobile follow-up
```

UI 在 M2 后可以开始接真实 event/API；默认用户模式在M5 Conversation/Memory稳定后闭环。不能先围绕临时内存结构冻结接口，也不能把专家runtime术语当默认产品信息架构。

## 阶段一完成判据

以下场景全部有自动化验收且没有未定义结果，才算阶段一完成：

- queue consume、node complete、state head、event publish 各边界 crash 后可恢复；
- 重复 start/webhook/wait response 不产生重复 activation 或副作用；
- interrupt/cancel 与 completion 竞态有唯一线性化结果；
- 非幂等 effect crash 后不会盲目重试；
- concurrent patch、candidate regenerate/swipe、merge head race 按 CAS 处理；
- SSE 断线可恢复全部 durable facts，token 丢失不影响 finalized output；
- Secret 不出现在 graph、input、state、event、trace、artifact 或错误中；
- 用户/专家模式对同一Story/Run得到一致权威状态，用户设置不能覆盖无法映射的专家配置；
- GC 后 branch/checkpoint/evidence/effect 引用仍完整可读。
