# Runtime Control、Wait 与 Effect 状态机

## 定位

本文定义 GraphRun、NodeInstance、NodeAttempt 的控制面：wait/resume、retry/backoff、lease/fencing、timeout、soft interrupt、hard cancel、外部副作用和 crash recovery。

数据流 firing 与原子 completion 见 `03-async-runtime.md`；领域 commit、ContextBranch、ReadSet 和 effect ledger 权威见 `16-domain-consistency.md`。本文状态均属于 `ExecutionState`，不能写入 WorkingContext 的 StatePatch。

阶段一以单机 SQLite 为目标，但所有转换仍使用 lease、epoch、幂等键和 durable wakeup，避免实现依赖进程内 Future 的存活。

## Hard Run Limits

每个 applied GraphRevision 必须提供不可关闭的 hard limits：

```ts
type RunLimits = {
  maxNodeActivations: number
  maxAttemptsPerActivation: number
  maxTotalQueueValues: number
  maxPendingQueueValues: number
  maxOpenWaits: number
  maxCoordinatorBufferedValues: number
  maxRunWallClockMs: number
  maxValueBytes: number
}
```

所有值必须为正，并受 workspace policy 上限约束。`maxRunWallClockMs` 从 run `startedAt` 按数据库时间计算，包含 waiting 和 interrupted；人工作业可以配置很大的有限值，但不能设为无限。

Activation、attempt、queue append、wait/window 创建、Expand emission、resume 和 retry 都在各自事务中检查 limits。超限产生 `run_limit_exceeded`，原子终止 run 并 fencing 旧工作。Router limit 是业务路由，不替代这些 limits。

## Run Lifecycle

```ts
type RunLifecycleStatus =
  | "created"
  | "running"
  | "waiting"
  | "interrupting"
  | "interrupted"
  | "completed"
  | "failed"
  | "cancelled"
```

状态含义：

```text
created       durable run 已创建，尚未开始 dispatch。
running       可以创建 activation/attempt，或仍有 attempt 执行。
waiting       已静默且只有 durable wait/timer/backoff/window 能解锁进度。
interrupting  soft interrupt 已请求；停止新工作，允许旧 attempt draining。
interrupted   draining 完成；执行投影保留，可 resume。
completed     图静默且 output contract 满足。
failed        出现未处理错误、required output 缺失或 hard limit 超限。
cancelled     hard cancel 已逻辑生效。
```

允许的主要转换：

| From | Trigger | To |
| --- | --- | --- |
| created | start transaction | running |
| running | 只有 durable blocker | waiting |
| waiting | blocker resolved且允许调度 | running |
| running | 图静默且 contract 满足 | completed |
| running/waiting | soft interrupt | interrupting 或 interrupted |
| interrupting | draining 完成 | interrupted |
| interrupted | resume + settle | running、waiting、completed 或 failed |
| 任意非终态 | unhandled failure/limit | failed |
| 任意非终态 | hard cancel | cancelled |

`completed/failed/cancelled` 是不可逆终态。Terminal run 不接受 resume、wait response 造成的状态推进或 node late completion。

Run 只有一个 lifecycle status，但 open waits、timers、queue 和 NodeInstance 是独立 durable records。`waiting` 是 scheduler 聚合结论，不代表只有一个 wait。

## Control Epoch 与 Finalization Fence

GraphRun 保存单调递增 `controlEpoch`。每个 NodeAttempt 记录开始时的 epoch，并持有独立 `leaseFence`。

Result 只有满足以下条件才能 finalize：

```text
attempt/node 仍为允许终结的非终态；
lease owner、leaseFence 与 durable row 完全匹配；
run status/epoch 接受该 attempt；
result idempotency key 尚未由其他结果使用；
attempt deadline 和 hard limits 未使结果失效。
```

正常 running 时只接受 `attempt.runControlEpoch == run.controlEpoch`。Soft interrupt 会把旧 epoch 记录为 `drainEpoch`；在 `interrupting` 中只额外接受属于 drainEpoch 且 lease 仍有效的既有 attempt。Hard cancel、failure 和 run deadline 不保留 drainEpoch，所有旧结果立即失去推进权限。

Epoch 保护 run 控制权，leaseFence 保护某次 attempt 的 worker 所有权，两者不能互相替代。

## NodeInstance 与 NodeAttempt 状态

NodeInstance 是一次 activation，其状态转换为：

```text
ready -> running -> completed
                 -> waiting -> running ...
                 -> failed
                 -> cancelled
```

Retry 和 resume 都留在同一 NodeInstance，不重新消费 input queue。

`NodeInstance.waiting` 有两个持久原因：executor wait 由 open WaitRecord 表达；Aggregator internal wait 由 open AggregationWindow + RuntimeTimer 表达。后者不创建 WaitRecord、不计入 `openWaits`、不会出现在 `listOpenWaits`，只由 coordinator resume attempt关闭。

NodeAttempt 是一次可 lease 的 executor invocation：

```ts
type AttemptStatus =
  | "queued"
  | "leased"
  | "running"
  | "completed"
  | "waiting"
  | "failed"
  | "timed_out"
  | "cancelled"
  | "outcome_unknown"
```

```text
queued -> leased -> running
running -> completed | waiting | failed | timed_out | outcome_unknown
queued/leased/running -> cancelled
```

Attempt 的所有终态不可覆写。一个 NodeInstance 同时最多一个非终态 attempt。`start`、`retry`、`resume` 和 `reconcile` 都创建新 attemptNo；其中只有因失败重试创建的 attempt 增加 retry ordinal。

## Lease 与 Worker Recovery

Claim attempt 的事务必须：

1. 校验 run 允许 dispatch、attempt 为 queued 且 backoff 已到期；
2. 增加该 attempt 的 `leaseFence`，写 workerId、leaseUntil 和当前 controlEpoch；
3. 把状态改为 leased，追加 journal；
4. commit 后才调用 executor。

Worker 开始调用后 CAS 为 running。Heartbeat 只能用相同 owner/fence 延长 lease，不能延长 execution deadline。

Lease 到期只表示 worker 所有权未知，不等于 executor 没产生副作用：

- 没有 external effect，或 effect 为 pure/idempotent且可安全重试：原子撤销旧 fence并按 policy 创建 retry；
- effect 可向 provider 查询：先创建 reconcile attempt；
- non-idempotent effect 已 started且不能查询：进入 `outcome_unknown`，禁止 blind retry。

旧 worker 随后返回时 fence 已失效，结果只能写隔离诊断，不能 finalize node。

## Retry 与 Durable Backoff

```ts
type RetryPolicy = {
  maxRetries: number
  retryOn: string[]
  initialBackoffMs: number
  multiplierMicros: number
  maxBackoffMs: number
  jitterRatioMicros: number
  refreshReadSet: "never"
}
```

`maxRetries` 不包括第一次 start，也不包括成功 wait 后的 resume。所有 invocation 仍受 `maxAttemptsPerActivation` 限制。

只有结构化错误码命中 `retryOn` 且 effect policy 允许时才重试。默认不可重试：schema/selector failure、permission denial、hard limit、non-idempotent `outcome_unknown` 和显式 policy rejection。

Backoff 只用整数固定点计算。`retryOrdinal=0` 表示第一次 retry；`multiplierMicros >= 1_000_000`，`jitterRatioMicros` 在 `0..=1_000_000`。令 `base_0=min(initialBackoffMs,maxBackoffMs)`，`base_n=min(maxBackoffMs, floor(base_(n-1)*multiplierMicros/1_000_000))`，每步用 checked u128，超界直接 clamp 到 max。`span=floor(base_n*jitterRatioMicros/1_000_000)`。

Jitter hash 是 `SHA-256(UTF8("retry-jitter/v1\0") || UTF8(nodeInstanceId) || 0x00 || ASCII(retryOrdinal无前导零十进制))`；`r` 取 digest 前 8 bytes 的 big-endian u64。`offset=floor(u128(r)*(2*span+1)/2^64)-span`，最终 `delayMs=clamp(base_n+offset,0,maxBackoffMs)`，没有浮点、平台 rounding 或随机源。结果 due time 使用创建 timer 事务的数据库时间加 delay，并持久化为绝对时间。RetryTimer 与失败 attempt、journal、wakeup 同事务创建。进程重启或 soft interrupt不会丢 timer；interrupt 期间到期只把 timer 标记 ready，不 dispatch，resume 后再创建 attempt。

StatePatch head 已推进时，finalize transaction 可以按 canonical policy直接处理非重叠 path 或 operationId append 的确定性 rebase，并记录原 base。不可 rebase 的重叠成为 `state_conflict`；阶段一它不在同一 activation 内刷新 ReadSet/retry，调用方需要时从新 head 创建新 activation/run。禁止 arbitrary/LWW 式复用旧 output 覆盖新 head。

`refreshReadSet="never"` 约束通用 executor RetryPolicy。Router built-in 不走该 retry 分支；它的 `validate_on_commit` CAS conflict 使用独立、有界 `reconcile` invocation，仅重读 Router bindings，且同时受 Router `maxReadReconciles` 和 run `maxAttemptsPerActivation` 限制。

## Execution Timeout

Node `timeoutMs` 在 attempt 开始时转换成 durable `deadlineAt`。数据库时间达到 deadline 后，timeout transaction CAS attempt fence、标记 timed_out、发出 best-effort cancel，再按 retry/effect policy决定下一步。

Execution timeout、lease expiry、wait deadline、retry backoff 和 run wall-clock deadline是五种不同计时器：

- lease expiry 只回收 worker ownership；
- execution timeout 限制一次 invocation；
- wait deadline 限制外部响应；
- backoff 决定下次尝试最早时间；
- run deadline fencing 整个 GraphRun。

Soft interrupt 不暂停这些绝对 deadline。Timer 触发、状态 CAS 和 durable wakeup 必须同事务；进程内 sleep 不是权威。

## Durable Executor Waiting

任意 Rust Future 都不可序列化。能返回 waiting 的 executor 必须实现显式 continuation contract：

```ts
type WaitRequest = {
  kind:
    | "human_response"
    | "approval"
    | "webhook"
    | "timer"
    | "external_job"
    | "effect_resolution"
    | "secret_store_unlocked"
  request: JsonValue
  responseSchema?: JsonSchemaSpec
  correlationKey?: string
  deadlineAt?: string
  onTimeout: "fail" | "resume_with_timeout"
}

type WaitRecord = {
  id: WaitId
  runId: RunId
  nodeInstanceId: NodeInstanceId
  attemptId: NodeAttemptId
  kind: WaitRequest["kind"]
  requestRef: ValueRef
  continuationRef: ValueRef
  responseSchema?: JsonSchemaSpec
  responseSchemaCompilation?: JsonSchemaCompilation
  correlationKey?: string
  deadlineAt?: string
  status: "open" | "resolved" | "expired" | "cancelled"
  responseRef?: ValueRef
  blockers: WaitBlockerRef[]
  acceptedDeliveryId?: string
  createdAt: string
  resolvedAt?: string
}

type WaitBlockerRef = {
  kind: "tool_call" | "memory_proposal" | "effect"
  id: string
  order: number
  status: "open" | "satisfied" | "rejected" | "aborted"
  decisionRef?: ValueRef
}
```

Continuation 是 executor 定义的 versioned、可序列化 JSON/object，必须包含恢复所需的逻辑位置，且与 NodeInstance execution snapshot 中的 executor/preset version兼容。它不能包含 secret 或进程地址。无法生成 continuation 的 executor 不能返回 waiting。

Executor attempt 返回 waiting 时，`finalize_attempt` 先按 `16-domain-consistency.md` 的 canonical pipeline 编译/持久化 response schema，再在同一事务：验证 fence/read set，提交可选 `NodeTransition.statePatches[]`，finalize attempt 为 waiting，把 NodeInstance 置 waiting，创建唯一 open WaitRecord并绑定 exact compilation，追加 journal，并触发 run settle。编译失败零写入且 attempt 以 typed executor contract error 失败；一次 NodeInstance 同时最多一个 external open wait。Aggregator 的 internal window 走 `18-coordination-nodes.md` 的专用 transition。

一个 wait 可以聚合多个 blockers；`wait_blockers` 按 order 持久化。`open` 是唯一非终态，`satisfied/rejected/aborted` 都是 terminal decision projection。Approval/Memory response 必须逐 blocker 产生合法 decision，并在同一事务更新 tool/proposal 状态；effect blocker 只能由 `resolve_effect_unknown` system command 更新。只有全部 blockers terminal、没有 `aborted` 且 canonical wait response 有效时才能 CAS wait resolved、NodeInstance ready；`rejected` 仍由 continuation 按 pinned failure policy处理。

## Wait Response 与普通 User Input

普通语义用户输入和 wait response 使用不同 command：

```text
create_user_turn(message, expectedConversationHead)
  -> 先提交 WorkingContext user-message commit，再创建新 GraphRun

submit_wait_response(waitId, deliveryId, payload)
  -> 只解决被明确引用的既有 WaitRecord
```

不能把自由文本自动投递给“最近的 waiting run”。Human response、approval、webhook 和 external job callback 都必须携带 waitId 或唯一 correlationKey，并通过 adapter 权限校验。

Adapter 可以先根据 wait 快照校验并预写 immutable response object；最终只有以下 `submit_wait_response` 事务能使其可见：

1. 先按 `(waitId, deliveryId)` 和 request digest 查 `wait_deliveries`；同 digest 重放直接返回旧结果，同 key 不同 digest 返回 `idempotency_conflict`，再锁定 open wait、run、NodeInstance 和按 `order` 排列的 blocker rows；
2. 校验 run 非 terminal、kind/correlation 和 response variant。无 blocker 的 human/webhook/external-job response 按 response schema 校验；有 blocker 时 payload 必须恰好逐项覆盖当前所有 open、外部可响应的 blocker，不得缺失、重复或夹带未知 ID；
3. 只要存在 open `effect` blocker，generic command 整体返回 `effect_resolution_required`，不得先提交其他 decision。对 tool blocker 校验 `(toolCallId, callDigest)`、actor、expiry、pinned policy/current revocation和 `approve | reject`；对 memory blocker经 MemoryManager 校验 proposal/current status、actor、policy/head 和 `approve | reject`；
4. 所有项先校验成功，才在一个事务中逐项更新 tool/proposal 领域状态，将 blocker CAS 为 `satisfied` 或 `rejected`，保存各自 immutable decision ref，并从该 wait 的全部 blocker rows按 order生成 canonical response object；任一项失败则整批零写入；
5. 重新读取同一事务内的 blocker projection。仍有 `open` 时 wait 保持 open、NodeInstance 保持 waiting；只有全部 terminal 且无 `aborted` 时才写 `responseRef/acceptedDeliveryId`、CAS wait resolved，并把 NodeInstance 从 waiting 改为 canonical `ready`；此时尚不创建 attempt；
6. 若 run 允许 dispatch，转 running并写 deduplicated resume wakeup；scheduler claim 时创建带当前 controlEpoch 的 queued resume attempt。若 interrupted/interrupting，只保存 ready 状态和 resolution，显式 resume 后再补 wakeup；
7. 同事务写 `wait_deliveries` result、durable journal/outbox。未通过校验的 delivery 不占用 idempotency key。

Public `submit_wait_response` 必须拒绝 `secret_store_unlocked`（以及其他 system-only timer/reconcile kind）；这些只能由对应 application service 的 authenticated system delivery 解决。主密码永远不是 wait payload。

同 delivery 重放返回原结果；已解决 wait 收到不同 delivery/payload 返回 `wait_already_resolved`，不能覆盖。除幂等旧结果外，terminal run 的新响应返回 `run_terminal`。

Timer wait 由 scheduler 使用相同 resolution transaction 生成系统 delivery。到期时 `fail` 终止 NodeInstance；`resume_with_timeout` 产生符合固定 schema 的 `{ timedOut: true }` response。

`secret_store_unlocked` 由 Secret Store application service 在 initialize/unlock receipt 提交且当前进程 session 已 active 后，以 `unlock:<randomSessionId>` 系统 delivery 解决；response 只含非敏感随机 session ID，不含主密码、key 或 SecretValue。同 idempotency key 仅在该 session/generation 仍有效时返回原结果；失效重放返回 `idempotency_key_expired` 且不发送 delivery。进程重启/再次锁定后，resume 若发现 session 已失效会创建新的 wait，不能把旧 ID 当 credential。

## Resume Invocation

Resolved wait 不恢复旧 call stack。Scheduler 在 run 允许 dispatch时为 ready NodeInstance 创建 `invocationKind: "resume"` 的新 NodeAttempt；Executor 收到原始 activation inputs、continuationRef 和 responseRef。

Resume 不重新消费 edge queue。它使用该 NodeInstance snapshot pin 的同一 executor contract；不兼容的 continuation schema是 `continuation_incompatible`，不能切换 preset“试着恢复”。如用户希望改 graph/preset，应取消旧 run并创建新 run。

Resume attempt 可以再次 completed、failed 或返回新的 waiting，形成可审计的多段状态机。

## Soft Interrupt：Draining

Soft interrupt 是停止继续推进而保留进度：

```text
request_soft_interrupt(expectedControlEpoch)
```

事务行为：

1. CAS expected epoch，保存旧 epoch 为 drainEpoch，递增 controlEpoch；
2. running -> interrupting；waiting/created 且无 active attempt可直接 -> interrupted；
3. 停止新 activation、retry、resume 和 coordination consumption；
4. 保留 queue、wakeup、wait、timer、window 和 resolved response；
5. 允许 drainEpoch 下已 leased/running attempt 用原 fence finalize；
6. 追加 control event/outbox。

Draining attempt 可以 completed、failed 或 waiting。Completed output/commit/edge 仍按原子协议持久化，但不会触发新 dispatch。Retryable failure只持久化 backoff/ready 状态，等待 resume 后 dispatch；不可重试的 NodeInstance failure仍按 failure fencing使 run failed，失败优先于 interrupt。其余 drain attempt进入逻辑终态后，settle transaction把 interrupting改为 interrupted。

如果 drain attempt 超时或 lease 丢失，按 timeout/effect 规则先得到明确结果；non-idempotent outcome unknown 可以形成 durable effect-resolution wait，然后 run 进入 interrupted。

`resume_run(expectedControlEpoch)` 再次递增 epoch、清除 drainEpoch并检查 projection：有 resolved wait、ready input 或 retry 时转 running并补 wakeup；只有未解决 blocker 时转 waiting；图已静默则执行正常 completion check。

## Hard Cancel 与 Late Result

Hard cancel 不 draining：

```text
request_hard_cancel(expectedControlEpoch, reason)
```

一个事务内递增 controlEpoch、清除 drainEpoch、把 run 标记 cancelled并先阻止新 logical work，撤销全部 attempt leases，再锁定该 run 的所有非终态 NodeInstance/attempt/wait/timer/window、model/count/tool owner、Effect 和 EffectAttempt。终止 inventory 以 logical owner/effect 为入口，不能只扫 active EffectAttempt；否则 `retry_ready` 或 `awaiting_approval` 空窗会残留可恢复工作。Commit 后才向 executor/provider发 best-effort cancel。

已经提交的 Context commit、effect result、output、edge 和 event 不回滚。每个 logical owner按以下互斥顺序收敛：

1. Effect 已 succeeded/failed/abandoned/cancelled及其 terminal owner保持不变。
2. 存在未解决的 `started/outcome_unknown` attempt 时，`started` 先与 finalizer 竞争 CAS为事实 `outcome_unknown`，logical effect/owner/checkpoint 置 `abandoned_unknown`，并为尚无 resolution 的该 attempt写 system `run_terminal_abandon`；这不宣称副作用未发生。
3. Effect 仍 pending但没有未解决的 started/unknown 时，把仍 prepared 的 attempt CAS为 `superseded_before_start`，再把 `prepared/retry_ready` owner/checkpoint和 effect置 `cancelled_before_start`。只对尚无 resolution且本次被 supersede的 attempt写 `run_terminal_cancel_before_start`；若旧 unknown 已由 `confirm_failed_retry_safe` 解决或最新 attempt 已 failed/superseded，terminal journal引用既有事实，不向同一 attempt硬塞第二条 resolution。
4. `requested/validated/awaiting_approval` 且尚未创建 Effect 的 ToolCall直接置 `cancelled_before_start`，其 approval blocker以 system decision置 aborted；它没有 EffectAttempt，因此不创建伪造的 EffectResolution。

同一事务随后 abort/关闭其余 open blocker/wait、timer/window，终结 NodeInstance/attempt并追加 journal/outbox。事务结束前复核该 run 不再存在非终态 logical owner/effect或可 dispatch work；任一 CAS失配就重新读取分类并整笔重试，不能部分收敛。

任何 cancel 前启动、cancel 后返回的结果都因 epoch/fence 失效而被拒绝。可把密文/脱敏 `late_result_rejected` metadata/ref 附加为该 system resolution 的 audit evidence，但不覆写 EffectAttempt/effect/owner 的终态，也不更新 branch head、NodeInstance、run output 或 edge queue。

## Failure Fencing

未处理 NodeInstance failure、required output 缺失、run deadline 或 hard limit超限，使用与 hard cancel相同的 epoch/lease fencing事务，但 run 终态为 failed并保存结构化 `RunError`。

同一事务阻止新调度、撤销其他 leases，并对全部 logical owner/effect/approval blocker执行与 hard cancel 相同的完整 inventory 收敛，再追加 failure journal/outbox。Commit 后才 best-effort cancel物理任务。并发 failure 通过 run status CAS只选出一个 terminal cause，其余作为关联诊断事件。

## Effect Ledger 与 `outcome_unknown`

网络、模型、工具、文件系统等副作用不能放入数据库 transaction。Logical effect 使用稳定 idempotency key，每次实际调用创建 attempt：

```text
effect:  pending -> succeeded | failed | outcome_unknown
         outcome_unknown -> succeeded | pending | abandoned_unknown
         pending -> cancelled_before_start | abandoned_unknown (run terminal fencing only)
attempt: prepared -> started | superseded_before_start
         started -> succeeded | failed | outcome_unknown
```

Effect 记录所属 NodeInstance、operation key、`pure | idempotent | non_idempotent` 分类、idempotency key、retry policy 和最终 result ref；EffectAttempt 记录 invoking NodeAttempt、attemptNo、provider request id、request/result refs。

Logical model call/count call/tool call/effect 归 NodeInstance，并记录首次创建它的 originating attempt 供 causation；每个 EffectAttempt 另存 `invokingNodeAttemptId`，只用该 invocation 的 lease fence/control epoch 授权实际外部调用与结果提交。Approval waiting 终结旧 NodeAttempt 后，resume attempt 在同一 NodeInstance/checkpoint 上继续已有 toolCallId，并作为新 EffectAttempt 的 invoking attempt；不需要、也不允许修改 logical ledger 的 originating attempt。Provider count 使用独立 countCallId/ordinal 和 pure effect，不伪装成 generation ModelCall。

执行协议：

1. 事务创建或读取 pending effect，并创建 prepared EffectAttempt；
2. commit 后 CAS attempt 为 started，执行外部调用并尽早记录 provider request id；
3. 先持久化不可变 result/error，再事务 finalize effect attempt 和 logical effect；
4. node completion 只引用 finalized effect outcome。

`prepared` 还未发送；外调用必须先在事务中用 invoking NodeAttempt fence CAS `prepared -> started`，CAS 失败时禁止发送。Invoking NodeAttempt lease/fence 被回收时，recovery 在同一事务把该 invocation 所有仍 prepared 的 EffectAttempt CAS 为 terminal `superseded_before_start`（这证明从未发送），logical effect 保持 pending，owner/checkpoint 置 `retry_ready`，并为同 NodeInstance 创建有界 reconcile wakeup/新 NodeAttempt。新 invocation 才创建下一 prepared EffectAttempt；不转移或启动旧 row。若 worker 已先 CAS started，recovery 不能 supersede，必须走正常 effect reconcile/outcome_unknown 路径。

Crash 发生在请求发出后、结果持久化前时：pure/idempotent effect 可按策略创建新的 EffectAttempt；可查询 provider 的 effect进入 reconcile；无法查询的 non-idempotent effect/attempt标记 outcome_unknown，并为 NodeInstance 创建 `effect_resolution` wait。Run 因该 blocker进入 waiting或在 soft interrupt后进入 interrupted，不能自动 retry。

需要保存 opaque continuation 的 provider effect 在 started 前还必须持有 purpose-bound SensitiveWriteLease。Secret Store lock 不撤销该 effect 的加密落盘能力；lease 丢失/过期且结果无法安全持久化时按 outcome_unknown/reconcile 处理，不能把 late plaintext写入普通 object、event 或日志。

`EffectAttempt.outcome_unknown` 是“当时无法观测结果”的不可变事实。所有 Effect 恰好绑定一个 normalized owner row（`model_calls | count_calls | tool_calls`）；下表的人工 `outcome_unknown` 协调只允许需协调的 model/tool owner，pure count owner 只自动重试/使用持久化 local fallback，不创建 effect-resolution wait。人工结论写新的 immutable `EffectResolution`，不能把旧 attempt row伪装成 succeeded/failed；logical Effect、owner row/checkpoint、wait projection 和 journal则按结论推进：

| Command | Logical Effect | 原 EffectAttempt | Owner/checkpoint 与 wait/run |
| --- | --- | --- | --- |
| `confirm_succeeded` | CAS `outcome_unknown -> succeeded`，绑定已校验的 canonical `resultRef/evidenceRefs` | 保持 `outcome_unknown`，由 resolution row 关联 | owner row与checkpoint均 CAS `outcome_unknown -> completed`并引用结果；blocker `open -> satisfied`，随后按全 blockers规则 settle wait |
| `confirm_failed_retry_safe` | 校验 policy/attempt budget 和证据后 CAS `outcome_unknown -> pending` | 保持 `outcome_unknown`，resolution 记录 confirmed-failed | owner row与checkpoint均 CAS `outcome_unknown -> retry_ready`；blocker `open -> satisfied`。本事务不调用 provider、不创建新 EffectAttempt |
| `abort_run` | CAS `outcome_unknown -> abandoned_unknown`，保留“物理结果仍未知”语义 | 保持 `outcome_unknown` | owner row与checkpoint均 CAS `outcome_unknown -> abandoned_unknown`且只供隔离审计；blocker `open -> aborted`，wait/NodeInstance cancelled，并以 hard-cancel fencing同事务把 run置 cancelled |

`resolve_effect_unknown` 必须携带 expected effect-attempt ID、expected run epoch、command idempotency key 和 request digest。事务先查幂等旧结果，再锁定 run/effect/该 unresolved attempt、唯一 owner row、checkpoint、引用它的 open blocker/wait；验证 actor、classification、evidence/result ref 后，原子写 `EffectResolution`、上述 projections、checkpoint ref、journal/outbox，必要时 settle wait/wakeup。`confirm_succeeded` 的 result必须在事务前已成为不可变对象并通过 owner output contract；`confirm_failed_retry_safe` 若不能证明 retry安全或已超 attempt limit则零写入返回 `retry_not_allowed`。后续 resume只有同时看到 owner row/checkpoint为 `retry_ready` 才能在一个事务把二者 CAS为 `prepared`并创建下一 EffectAttempt；任一不一致返回 `effect_projection_corrupt`，不能猜测修复或调用 provider。

该事务的 run-local journal按因果顺序记录 resolution、logical effect/owner projection、blocker decision，随后才是可选的 `wait.resolved + node.ready + wakeup` 或 `run.cancelled`；这些记录共享同一 commit，replay不能看见“effect 已解决但 owner/checkpoint/blocker仍未更新”的中间态。settle wait时的 `responseRef` 是按 blocker order合成的 canonical decision envelope，不接受协调者另传一份可能分叉的 generic response。

同一 idempotency key + digest 重放返回原 resolution，即使 effect/run 已推进；同 key不同 digest 返回 `idempotency_conflict`。每个 unknown EffectAttempt最多一条 resolution，竞争的另一命令返回 `effect_already_resolved`。迟到 provider result只能写隔离 audit evidence，不能覆盖人工结论。

阶段一同一 batch 的 non-idempotent effects 串行，因此一次 NodeInstance 最多一个 unknown effect blocker。只有全部 blockers收敛后 NodeInstance才 ready；generic `submit_wait_response` 不能替代 effect resolution。

## Crash Recovery

恢复不复活内存 Future：

1. 从 RuntimeCheckpoint 与 durable journal重建 projection；
2. 校验 graph revision/hash、manifest、controlEpoch 和 counters；
3. 对过期 lease 按 effect state 分类为 retry、reconcile 或 outcome_unknown；
4. 对已到期 wait/backoff/window/run deadline执行 durable timer transaction；
5. 为 ready activation、attempt、resolved wait和 settle_run补 wakeup；
6. interrupted run只恢复 projection，不 dispatch，直到显式 resume。

每个恢复动作都有幂等键并通过 CAS；重复 replay、重复 callback和迟到 worker不能产生第二次 logical transition。

## 最小控制 API

阶段一 core command 至少包括：

```text
start_run
request_soft_interrupt(expectedControlEpoch)
resume_run(expectedControlEpoch)
request_hard_cancel(expectedControlEpoch, reason)
submit_wait_response(waitId, deliveryId, payload)
claim_attempt / heartbeat_attempt
resolve_effect_unknown(effectId, expectedEffectAttemptId, expectedControlEpoch, command, evidenceRefs)
```

Adapter 只做协议、鉴权和 DTO 转换。所有 command 返回新的 status/controlEpoch 或幂等旧结果，控制冲突返回 `control_epoch_conflict`，不能 last-write-wins。

## Summary

Wait 是 durable continuation，不是挂起的 Rust Future；resume 创建新 attempt但不重新消费输入。Retry 使用持久 backoff，lease 和 control epoch共同 fencing。Soft interrupt停止新工作并 draining旧 attempt；hard cancel立即逻辑终止并拒绝 late result。外部副作用先进入 effect ledger，无法确认结果时进入 outcome_unknown而不是盲重试。所有 timer、control和 response transition 都与 journal/wakeup同事务提交。
