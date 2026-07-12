# 实现蓝图与验收门槛

## 目标

本文件把设计基线转换为实现顺序和验收门槛，不新增另一套领域模型。阶段一的完成标准是：单进程 SQLite 部署能够持久、可恢复地运行真实异步图，而不是只在内存中演示一次 DAG。

## 初始 Workspace

模块边界已经稳定到可以使用小型 Rust workspace：

```text
crates/
  core/
    graph/       definition、revision、validation
    runtime/     run、scheduler、node、wait、control
    state/       patch、context commit、branch、merge
    memory/      binding、record、proposal、policy
    events/      durable event 与订阅抽象
    tools/       descriptor、grant、executor port
    artifacts/   metadata 与 object-store port
  llm/
    ir/          provider-neutral semantic IR
    context/     context assembly
    executors/   LLMNode/tool loop
    adapters/    标准 API shape adapter
  storage/
    entities/    SeaORM entity，只在本 crate 暴露
    migrations/
    sqlite/
    object_store/
  adapters/
    axum/
  server/
```

初期不必为每个 core 子目录拆 crate。`core` 不能依赖 SeaORM、Axum、Tauri、provider SDK 或 UI 类型；`storage` 和 `adapters` 依赖 core 定义的 port。

PostgreSQL、Tauri 和前端应用在 SQLite 闭环稳定后接入。同一领域 API 不因 adapter 增加而改变；前端工程、Role Play 产品、页面和组件规范见 `23-ui-architecture.md` 至 `26-ui-design-system.md`。

## 依赖方向

```text
server -> axum adapter -> core service ports <- storage implementation
                              |
                              +-> application services -> RuntimeService
                              +-> llm executor -> shape adapter -> gproxy-protocol
                              +-> tool/object/secret ports
```

允许的共享依赖是 serde、thiserror、tokio 抽象和少量稳定值类型。HTTP request、数据库 entity、provider wire response、secret plaintext 都不能穿过 core 边界。

## Core Ports

RuntimeService 与 Graph/Conversation/Memory/Artifact/Secret 等窄 application service ports 位于 core；adapter composition root 分别注入，不能把它们揉成全局 context。存储 port 应表达高层原子操作，而不是把数据库 transaction 暴露给 scheduler：

```rust
trait RuntimeStore {
    async fn create_run(&self, command: CreateRun) -> Result<RunRecord, StoreError>;
    async fn activate_if_ready(&self, key: ActivationKey) -> Result<Activation, StoreError>;
    async fn claim_attempt(&self, command: ClaimAttempt) -> Result<AttemptLease, StoreError>;
    async fn commit_node_result(&self, command: CommitNodeResult) -> Result<CommitOutcome, StoreError>;
    async fn record_attempt_failure(&self, command: FailAttempt) -> Result<RetryOutcome, StoreError>;
    async fn apply_control(&self, command: RunControlCommand) -> Result<RunRecord, StoreError>;
    async fn satisfy_wait(&self, command: SatisfyWait) -> Result<WaitRecord, StoreError>;
}
```

方法名不是冻结的 Rust API，但原子边界是冻结的。实现不能把 `consume queue -> create instance -> append event` 拆成几个可独立成功的 repository 调用。

Application storage ports 同样按领域保持窄接口：Graph/Config store 提供 create/publish，ConversationStore 提供 `create_conversation_root`、`submit_turn_and_create_run`、`project_candidate`、`select_candidate`，SecretStore 提供 initialize/session command。尤其 `submit_turn_and_create_run` 必须让 storage adapter 在一个事务中复用 RuntimeStore 的 run rows/counters/wakeup 写入；不能由 ConversationService 连续调用“提交消息”和 `RuntimeService.startRun` 两个独立事务。

其他 ports：

```text
NodeExecutorRegistry  node kind -> executor
LlmClient             标准 shape request/stream
ToolRegistry          descriptor/grant -> executor
ObjectStore           staging、commit、open、delete
SecretInjector        仅 provider client 内部使用
Clock                 可测试 deadline/timer
IdGenerator           稳定 ID，不从数据库自增推导领域身份
EventNotifier         commit 后提示订阅者；event log 才是权威
```

## Scheduler 结构

单进程仍按可扩展的 claim 模型实现：

```text
durable state/event commit
-> best-effort notifier
-> scheduler 扫描可激活节点/到期 timer/retry/lease
-> transaction claim + fencing token
-> 在 transaction 外执行 node
-> transaction commit result
```

Notifier 丢失不能影响正确性；周期扫描必须能发现所有 durable work。不要在持有数据库 transaction 时等待 LLM、tool、文件 I/O 或 subscriber。

第一阶段每个 `(run, node)` 只允许一个 active NodeInstance。不同节点可以并发，工具内部并发受 binding 限制。

## 里程碑

### M1：静态图与持久化骨架

- Graph draft/applied revision、content hash 和静态校验
- Graph/Channel/ContextPreset 顶层 create command、空 head/draft 与 application receipt
- bounded exact-decimal `JsonValue`/`canonical_json_v1`，以及 `JsonSchemaSpec` parse/compile/hash、compiled payload persistence 和 resource/fuel limits
- operation taxonomy/adapter decoder support matrix 与 graph/channel revision version pin
- Input/Output/Router 与显式 ports
- SeaORM migration、SQLite WAL 配置、object staging
- 最小 contexts/root commit/root branch 与 temporary context 创建（供所有 GraphRun FK/binding）
- Run input 持久化和幂等 start
- 领域错误与 tracing ID

验收：fresh install 可只经公开 application command 创建 Graph/Channel/Preset；非法图不能 applied；remote/dynamic schema ref、未知 keyword/format/schema compiler 或 operation/decoder version 均 fail closed；同一 idempotency key 不会创建两个资源或 run；temporary/existing context binding 均通过 head CAS；重启后从已验证 compiled payload读取 run input、revision 和 root branch。

### M2：FIFO Runtime 闭环

- GraphRun、NodeInstance、NodeAttempt、lease/fencing
- edge queue、`activate_if_ready`、completion transaction
- 普通 all-input FIFO firing、重复 activation
- durable events、SSE reconnect
- OutputNode single/append 和 run completion

验收：在每个事务前后注入 crash，恢复后的实例数、queue consumption、outputs 和 critical events 与无 crash 相同。

### M3：控制与协调

- Retry/backoff/timer
- waiting continuation、幂等 response、resume
- soft interrupt、hard cancel 和 late completion isolation
- Router DSL/loop guard/global limits
- Merge、JoinByKey、Aggregator、Expand
- Runtime checkpoint

验收：重复 webhook 只恢复一次；interrupt 与 completion 任意竞态均不重复传播；非幂等未知结果进入人工协调；所有 cycle 有不可绕过上限。

### M4：Context、LLMNode 与工具

- Context preset revision、assembly snapshot、预算/preview
- LLM IR 与四种 generation shape 的最小 adapter
- exact `(operationTaxonomyVersion, adapterDecoderVersion, OperationKey)` encoder/stream/terminal conformance fixtures
- models/count 与显式 image/embedding/compact client boundary
- stream finalizer 与多轮 tool loop checkpoint
- Tool Registry/grant/approval/effect ledger
- Secret Store initialize/unlock、session-bound HMAC receipt 与 provider auth 注入
- ArtifactRef 与大 tool output

验收：同一 shape/version 的 tool transcript 可继续，未知或不匹配 decoder 在 provider effect 前拒绝；等待审批后重启仍能恢复；已完成副作用工具不因 output repair 重跑；初始化不能覆盖已有 store，失效 unlock receipt 不复活 session；secret 不出现在 trace/event/state。

### M5：Context、Memory 与 Conversation

- 完整 StatePatch/commit/projection、fork/merge（M1 已有最小 root branch 骨架）
- Memory binding、proposal、review/apply
- 固定 `search_memory` capability 记录 query/scope snapshot/read set 并回填确定性 ToolResult；固定 `propose_memory_change` capability 创建 canonical proposal 与 durable review wait，审批后恢复同一 LLM loop
- CreateConversation root snapshot/commit/branch、append-only ConversationContext、Turn/Candidate 与选择 head
- fork 和阶段一 merge
- compaction/GC roots

验收：fresh workspace 可原子得到 `messages: []` 的 Conversation root；消息 row/append patch/commit/head 不会部分可见；regenerate 候选相互隔离；失败 run 不推进 active head；并发相同路径写产生 conflict；不同路径按已定策略提交；memory search 重放不漂移，proposal wait 的批量审批/拒绝与 resume 原子且可幂等重放，terminal cancel 会中止 blocker；GC 后所有被 branch/checkpoint/evidence 引用的对象仍存在。

### M6：Adapter 与端到端

- Axum JSON commands、SSE、artifact streaming
- Graph/Channel/Preset/Conversation/Secret initialize 的 HTTP 与 Tauri bootstrap wrappers
- 可选 WebSocket control
- SDK 的 `start/events/wait/invoke` 组合
- 配额、payload 上限、错误脱敏

验收：所有顶层资源都能从空数据库经公开 adapter 创建且幂等重放；SSE 断线按 durable cursor 无重复/遗漏语义事件；慢客户端不阻塞 scheduler；HTTP/Tauri 与直接 application/core 调用产生相同领域结果。

## Frontend 产品轨道

前端不阻塞 M1–M6 runtime 语义闭环，但不能被当作通用 Graph 控制台。M1–M2 后可以实现 token、双模式 shell、api-client、expert draft/run diagnostics；M4–M6 接入真实生成、approval、Secret、Context preview；M5 Conversation/Memory 稳定后完成默认 Agentic Role Play 用户闭环。

阶段性交付：

1. Foundation：Web shell、Light/Dark/High Contrast token、user/expert navigation、transport和 durable reducer。
2. Expert vertical：GraphDraft Apply、Run/Trace、wait/control，用于验证后端真实契约。
3. User vertical：Channel/角色模板→CreateConversation→消息/stream→candidate/regenerate→approval/memory/branch。
4. Settings：user-mode compatibility view、角色/世界/Context/生成/能力分层设置，保存到 canonical revision。
5. Platform：Web闭环稳定后接 Tauri desktop；mobile优先用户模式，复杂Graph编辑继续使用宽屏。

验收：普通用户不进入专家模式即可完成新故事和候选/审批/设置；专家修改 incompatible Graph 后用户表单不覆盖未知配置；两种模式对同一 Story/Run 的状态与权限一致；断线时 live overlay可丢而 durable消息/候选可恢复；关键流程满足 `25-ui-screen-specs.md` 与 `26-ui-design-system.md` 的响应式和可访问性要求。

## 必测行为

测试以不变量和竞态为中心，不追求覆盖率数字：

```text
graph validation         port、cycle、output、schema compile、taxonomy/decoder pin
activation               FIFO、广播、重复 firing、原子消费
completion               output/patch/edge/event 同事务
recovery                 expired lease、wait、timer、checkpoint cursor
control                  retry、interrupt、cancel、late result
coordination             any order、key pairing、window、expand index
state                    CAS、path conflict、fork、three-way merge
conversation             user commit、candidate isolation、swipe
bootstrap                config roots、conversation root、secret initialize、receipt replay
tool                     grant、approval、idempotency、outcome_unknown
events                   durable sequence、reconnect、compaction
security                 secret redaction、artifact path、input limits
```

使用真实 SQLite transaction 做核心集成测试；纯 scheduler 状态和 DSL 可用内存 fixture。Provider、tool、clock 和 notifier 使用可控 fake，不能通过真实付费 API 验证基本状态机。

## 故障注入矩阵

至少在以下线性化点前后注入进程终止或错误：

- object staged / metadata committed
- top-level resource inserted / application receipt committed
- secret header+HMAC receipt committed / process session installed
- queue heads selected / activation committed
- external effect prepared / started / succeeded
- node result received / completion committed
- context commit inserted / branch head advanced
- durable event committed / notifier called
- wait response accepted / instance made runnable
- cancel epoch advanced / provider late result returned
- checkpoint written / old event compacted

每个案例必须声明恢复后的权威记录、允许的重试和用户可见状态；不能以“通常不会发生”结束测试。

## 可观察性

所有运行日志使用结构化 tracing，至少携带 `traceId/runId/nodeInstanceId/attemptId` 中可用的字段。高频 token 不写普通 info log。错误 detail 在进入 event/API 前分类和脱敏。

建议指标：ready/running/waiting 数、activation 延迟、attempt 时长、retry 数、lease expiry、event subscriber lag、queue 深度、context tokens、tool outcome_unknown、object staging bytes。

## 明确延后

- 分布式 worker 与跨区域一致性
- PostgreSQL 特有优化和多租户 row-level security
- arbitrary reducer、quorum/latest/session window
- 任意自动 conflict merge、branch rebase、CRDT
- 动态改图、插件 ABI、自动补偿事务
- 完整 SillyTavern 历史行为兼容
- Tauri 内嵌 HTTP server

延后项不允许在阶段一类型中留下没有语义的占位字段。出现真实需求时通过新 revision/schema version 扩展。
