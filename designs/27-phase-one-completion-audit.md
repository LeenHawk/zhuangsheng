# 阶段一完成审计

## 结论与口径

截至 2026-07-13，`09-minimal-scope.md`、`15-review-status.md` 与
`22-implementation-blueprint.md` 定义的阶段一代码语义均已有直接实现和自动化证据。
本审计只把代码、真实 SQLite 集成测试、adapter contract test 和 production build
算作证据；文档设想、类型占位或测试名称本身不算完成。

`09-minimal-scope.md` 的显式延后项不作为缺口。当前环境同时验证了 Tauri
`--no-default-features` contract、完整 WRY link build 和实际 Linux `.deb` bundle。

## 里程碑证据

| 里程碑 | 直接证据 |
| --- | --- |
| M1 Graph 与存储 | `graph_apply`、`graph_commands`、`config`、core graph/schema/canonical tests；fresh create、CAS、immutable revision、exact decimal、compiled schema/version pin 均通过真实 SQLite。 |
| M2 FIFO Runtime | `runtime_scheduler`、`runtime_start`、server public vertical slice；queue、activation、attempt、output、event sequence 与 terminal settle 同库验证。 |
| M3 Control/Recovery | `runtime_control`、`runtime_timers`、`runtime_human_wait`、Router/Merge/Join/Aggregator/Expand suites、`runtime_checkpoint`。 |
| M4 LLM/Tool/Secret/Artifact | core 四种 shape conformance；server executor recovery/tool/stream/output-repair suites；storage effect ledger、approval、secret、opaque bundle、artifact staging/commit suites。 |
| M5 State/Memory/Conversation | context patch/fork/merge/replay、memory proposal/search、conversation root/turn/regenerate/selection/projection suites；GC 后真实 branch/checkpoint/memory/effect root 仍可读。 |
| M6 Adapters | Axum HTTP/SSE tests、Tauri adapter tests、HTTP/Tauri exact JSON client fixtures、public Role Play journey。Core 不依赖 Axum/Tauri。 |
| Frontend | api-client 82、graph-view 1、Web 32 tests；user setup/story/candidate/wait/memory，expert Graph/Run/Context，platform capability、Shell status、mobile semantic contracts与双 production builds。 |

## 阶段一完成判据

| 判据 | 结论与代表证据 |
| --- | --- |
| queue、completion、state head、event 边界恢复 | Proved：`restart_after_finalize_uses_durable_wakeup_without_duplicate_enqueue`、context CAS/replay、checkpoint tail replay。 |
| 重复 start/webhook/wait 不重复 activation 或副作用 | Proved：start receipts、human wait delivery replay、approval/memory batch replay、effect/tool logical-call ledger。 |
| interrupt/cancel 与 completion 唯一线性化 | Proved：control race/fence tests、running drain、terminal fencing与 late result rejection。 |
| non-idempotent unknown 不盲重试 | Proved：started non-idempotent recovery进入 durable effect-resolution wait；generic wait response不能绕过。 |
| concurrent patch/candidate/merge race 使用 CAS | Proved：overlap conflict/disjoint rebase、sibling candidate isolation/historical selection、merge head/selection guards。 |
| SSE cursor 恢复 durable facts，token 可丢 | Proved：server `Last-Event-ID`、client reconnect cursor、ephemeral overlay disconnect clear、terminal durable projection。 |
| Secret 不进入领域状态或可见 trace | Proved：session-bound restart tests扫描 SQLite/WAL/events，HTTP只返回 metadata，provider request/events不含 credential。 |
| user/expert 投影同一权威对象 | Proved：共享 `domain-ui`、同一 api-client DTO；roleplay compatibility fail-closed，partial/expert-only保存不覆盖未知配置。 |
| GC 后 branch/checkpoint/evidence/effect roots 可读 | Proved：domain-root GC assertions、foreign-key/owner-ref sweep fence、checkpoint/event compaction recovery。 |

## 故障注入矩阵

| 线性化点 | 恢复后的权威记录、允许重试与用户状态 | 直接证据 |
| --- | --- | --- |
| object staged / metadata committed | validated staging可重试；commit原子写 Artifact、metadata commit/projection/ref/receipt；generation冲突不猜结果。 | `artifact_staging`、`artifact_commit`、HTTP multipart tests |
| resource inserted / receipt committed | create/publish/start/conversation root同 key同 digest返回原结果，不同 digest冲突；无半资源。 | graph/config/runtime/conversation idempotency tests |
| secret header+receipt / process session | DB commit后才有当前进程 session；重启必锁定，旧 unlock receipt不能复活。 | `encrypted_secret_lifecycle_is_session_bound_and_restart_locked` |
| queue selected / activation committed | selection、consume、instance、attempt、events、wakeup同事务；重启扫描 durable work，不重复 consume。 | FIFO/restart/activation rollback suites |
| effect prepared / started / succeeded | prepared可按 policy恢复；started non-idempotent变 unknown；completed logical result复用且受 fence保护。 | ledger/recovery/model/tool suites |
| result received / completion committed | finalize重放不重复 edge/output；过期 lease或 cancel epoch的 late result被拒绝。 | runtime scheduler/control/terminal fencing tests |
| context commit / branch head | commit、projection、head CAS、event原子；stale overlap conflict，安全 disjoint rebase。 | context patch/static write/fork/merge suites |
| durable event / notifier | journal cursor是权威；HTTP定时 drain、Tauri callback只作 wake hint，重复/乱序 hint仍输出严格递增 cursor。 | server SSE test、api-client `TauriTransport` test |
| wait accepted / runnable | delivery、blockers、checkpoint、timer/wakeup原子；重启后一次 resume，重复 delivery返回 replay。 | human/approval/memory/secret wait suites |
| cancel epoch / provider late result | epoch先线性化；prepared证明未开始，started记录 unknown；旧结果不能推进 owner、branch、output或queue。 | runtime control、LLM terminal fencing、model recovery |
| checkpoint / event compaction | checkpoint checksum与 projection一致才允许 compact；critical journal保留，tail可重放。 | runtime checkpoint/event compaction tests |

## Frontend 与平台边界

- 首页 `attention` 是 Conversation 列表的一次轻量 SQL 投影，覆盖 approval、human input、
  memory review、secret unlock、effect resolution 与 projection conflict；不为每个 Story 拉 timeline/waits。
- Shell 始终接收 composition root 注入的 connection 与 Secret metadata；不持有 secret bytes/session。
- Web 和 Desktop 共享 domain components，分别注入 HTTP/SSE 与 Tauri command/cursor transport。
- mobile contract 有故事/资料库/记忆/设置底栏、44px 级目标、sticky composer、按钮式 candidate/wait
  action，以及 Graph node list + 宽屏编辑提示；JSDOM 证明语义与 responsive class contract，
  不冒充真实设备像素截图。
- Web route-level splitting 后主 chunk 约 386 KiB；Desktop 主 chunk 约 476 KiB，Graph canvas
  与 Studio/Run 均独立 lazy chunk。

## 显式延后

延后范围保持 `09-minimal-scope.md` 原文：分布式 worker/leader、PostgreSQL 生产优化、
多租户/RLS、dynamic graph/plugin ABI、per-node 多 activation 并发与高级 fairness、
quorum/latest/session window/arbitrary reducer、CRDT/rebase/跨 context 或自动 merge、长期记忆
branch/自动 promotion、远程或 chunked object store、完整 token/raw response 长期保留、完整
SillyTavern 兼容、Tauri 内嵌 Axum、跨平台完整功能对等与最终视觉/离线打磨，以及同一
model response ���混合 memory capability 与 custom tool batch。

## 最终门禁

- `cargo test --workspace --all-targets`：Core 103、Server 59、Storage 141、Tauri adapter 2，全通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `corepack pnpm -r typecheck`：6 个前端 workspace project通过。
- api-client 82、graph-view 1、Web 32 tests：全通过。
- Web/Desktop production build：通过，无 chunk size warning。
- Desktop Rust `--no-default-features` check 与完整 WRY `cargo build`：通过。
- 官方 Tauri CLI debug `.deb` bundle：通过，产出 `庄生_0.1.0_amd64.deb`。
