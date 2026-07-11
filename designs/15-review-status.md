# 设计基线状态

## 当前结论

截至 2026-07-11，阶段一设计已形成可实现闭环：graph、runtime、control/recovery、state/context、memory、LLM/tool、event、storage、adapter 和 UI 之间没有仍需靠实现者猜测的核心语义。

`design-complete` 不表示所有未来能力已设计，也不表示具体 crate、SQL DDL 或 UI 像素已经冻结。它表示阶段一可以按 `22-implementation-blueprint.md` 开始实现；实现证据若推翻假设，应按本文规则重新打开对应决策。

## 冻结的核心边界

- Core RuntimeService 独立于 Axum、Tauri、SeaORM entity、provider SDK 和 UI。
- GraphRevision 不可变并带 content hash；GraphRun 固定 revision。
- Schema 统一为 versioned `JsonSchemaSpec`，发布时 compile/hash/persist；`canonicalDocumentHash` 用于 exact contract compatibility，含 effective limits 的 `schemaHash` 用于执行恢复；不做 coercion/default mutation，未知 profile/compiler fail closed。
- Graph/channel/LLM execution snapshot 固定 operation taxonomy 与 adapter decoder version，ShapeAdapter 只按 exact support matrix dispatch。
- Edge 只连接 output/input port；selector 在消费者，控制/协调用显式节点。
- 工具默认是 LLMNode capability，高风险/独立生命周期操作提升为节点或 durable effect。
- ExecutionState、WorkingContext、LongTermMemory、ArtifactObject 是四个写边界；Secret 独立。
- Context branch 跨 GraphRun；Router fan-out 不创建 branch。
- NodeInstance 是 activation，NodeAttempt 是 retry/resume/reconcile invocation。
- Runtime journal 与 normalized rows 同事务；ephemeral delta 不承诺 replay。
- 外部副作用进入 effect ledger，non-idempotent unknown outcome 不盲目重试。
- Conversation candidate 使用 sibling branch；failed/cancelled run 不推进 active head。
- Fresh install 通过公开命令原子创建 Graph/Channel/Preset、Conversation root context 和 Secret Store；所有创建都有 durable receipt。
- ConversationContext 从 `messages: []` root 开始，只允许带稳定 element/operation ID 的 `/messages` append。
- Secret initialize/unlock receipt 绑定当前进程 session generation；失效重放不能复活旧 session。
- CountCall 固定 execution pin 与 trim candidate，并和 effect/checkpoint/预算原子恢复；artifact staging 以 generation/delete fence 收敛。
- Context/LLM/tool/channel/semantic policy 在 NodeInstance 首次执行时 pin snapshot，已有 activation 恢复不切换；当前 deny-only revocation overlay 可继续收窄权限。
- Web/Tauri UI 只消费 command/query/event projection，不复制 scheduler。
- 产品默认表面是 Agentic Role Play 用户模式；专家模式暴露 Graph/Run/Trace，但两者只投影同一领域对象和权限。
- 用户设置必须无损映射到 GraphDraft、ContextPreset、MemoryBinding、model/tool policy；专家自定义无法映射时只允许 partial/只读，不能维护第二份执行配置。

## 原 Pending Review 的关闭结果

1. Waiting/Resume：`17-runtime-control.md` 定义 WaitRecord、continuation、delivery 幂等和新 resume attempt。
2. InputNode/Run Input：`11-graph-definition.md` 与 `03-async-runtime.md` 定义零入边 source、RunInputRef 和启动事务。
3. Retry/attempt：activation 与 attempt 分离，持久 backoff、lease/fence 和 effect-aware retry 已冻结。
4. Timeout/Interrupt/Cancel：execution/wait/backoff/run deadline 分离；soft interrupt draining，hard cancel fencing late result。
5. FIFO queue/事务：`activate_if_ready` 与 `finalize_attempt` 是不可拆分的存储原语；当前节点终态后必须重检自身。
6. Merge/Join/Aggregator/Expand：`18-coordination-nodes.md` 给出 durable order、buffer/window 和 limits。
7. State/Memory/version/conflict：`16-domain-consistency.md` 统一 StatePatch、proposal、commit/read set、CAS 和 merge MVP。
8. Conversation schema：`13-conversation-turn-run.md` 定义 root `ConversationContextV1`、canonical append、Message/Turn/Candidate/Selection、原子提交和历史 swipe。
9. ContextPreset/RP：`08-context-assembly.md` 固定 snapshot、trust/provenance、排序、预算、preview 和兼容边界。
10. Router DSL：`14-router-node.md` 固定 v1 值域、operator/functions、missing/error、fuel、visits 和原子 decision。
11. Tool Registry：`19-tools-artifacts.md` 定义 descriptor/grant/hosted binding、approval、failure 和副作用。
12. Artifact lifecycle：同文档定义 staging、content hash、refs、retention、安全和 GC。
13. Event/replay/compaction：`05-streaming-events.md` 区分 durable sequence/live delta，并固定发布、背压和 retention。
14. SeaORM schema/transaction/migration：`20-storage-schema.md` 给出 SQLite-first 表组、fresh-workspace bootstrap、约束、事务和 PostgreSQL 等价语义。

LLM IR transcript round-trip、opaque continuation、Secret threat model/provider auth 注入和 adapter API 分别在 `07-llm-ir.md`、`12-secret-store.md`、`21-adapters-api.md` 闭合。前端工程、Agentic Role Play 双模式、页面交互和设计系统分别由 `23-ui-architecture.md`、`24-agentic-role-play-ui.md`、`25-ui-screen-specs.md`、`26-ui-design-system.md` 冻结。

## 阶段一明确延后

这些是有意的 scope 边界，不是待设计缺口：

- 分布式 worker、PostgreSQL 生产优化、多租户/RLS；
- dynamic graph、插件 ABI、任意脚本节点；
- per-node 多 activation 并发和高级 scheduler fairness；
- quorum/latest、keyed/sliding/session window、arbitrary reducer；
- CRDT、branch rebase、跨 context merge、任意自动冲突 merge；
- 长期记忆 branch/自动 promotion、LLM 自动解决 merge；
- chunked/remote object store、完整 token/raw 长期保留；
- 完整 SillyTavern 历史行为复刻；
- Tauri 内嵌 Axum server，以及 desktop/mobile/web 完整功能对等与离线打磨。

完整清单以 `09-minimal-scope.md` 为准。

## 实现期可调参数

以下选择可以在不改变语义契约的前提下通过 spike/benchmark 确定：

- Router DSL v1 的具体 Rust parser/evaluator crate；
- Argon2id 设备校准后的参数和自动锁定默认时长；
- VersionSnapshot 频率、inline object 阈值、GC 宽限期；
- SQLite batch size、scheduler scan interval、subscriber buffer；
- provider adapter 的 capability probe 与本地 tokenizer safety margin；
- React Flow layout 实现和视觉 token。

这些参数必须受文档中的 hard bounds、determinism、安全和 migration 规则约束。不能用“实现细节”名义改变 error、retry、ordering、permission 或 recovery 语义。

## 重新打开设计的触发条件

出现以下任一情况时，先更新对应设计，再扩实现：

- 同一概念出现第二个不兼容 source of truth；
- 无法在 SQLite 单事务满足既定原子边界；
- provider 标准 shape 无法满足 IR same-shape round-trip；
- 新 node/tool 需要目前没有的 side-effect、wait、compensation 或 permission 语义；
- 需要删除 critical history/object 但现有 GC roots 无法证明不可达；
- UI 必须从 raw event 猜 scheduler/branch 状态；
- 真实场景需要已延后的多 worker、多租户或高级 coordination。

## 变更规则

```text
记录失败场景或新需求
-> 指明受影响的不变量和权威数据
-> 更新 canonical 文档与引用文档
-> 增加 migration/recovery/compatibility 方案
-> 更新本文件和阶段一范围
-> 用故障注入或 contract test 验证
```

Reviewed decision 不能在代码中静默偏离。若只是命名或内部重构，仍需保证持久化 payload/schema version、event decoder 和旧 run recovery 兼容。
