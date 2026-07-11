# LLMNode Tool Loop 与流式聚合

## 两层执行

```text
ModelCall
一次 logical provider request，可因安全网络错误产生多个 effect attempt。

LLMNode activation
一个 NodeInstance 的语义执行，可包含多次 ModelCall 和多个 tool batch。
```

图 scheduler 只看到 NodeInstance finalized result；内部 model/tool/approval/effect 通过 `LlmLoopCheckpoint`、ledger 和 event 观察与恢复。

## 状态机

```text
LoadExecutionSnapshot
-> AssembleContext
-> BuildRequestIr
-> PrepareModelCall
-> StreamAndFinalize
-> PersistOrderedResponse
-> HasCustomToolCalls?
     -> ValidateAndAuthorizeBatch
     -> Approval / Waiting?
     -> PrepareEffects
     -> ExecuteAndPersistResults
     -> AppendResultsByCallIndex
     -> Checkpoint
     -> BuildNextRequestIr
-> ValidateFinalOutput
-> FinalizeNodeResult
```

每个可等待/崩溃点都必须已有 durable checkpoint；不能尝试序列化正在 poll 的 Rust Future。

## Execution Snapshot

NodeInstance 首次执行时固定：

- graph revision 和 node definition；
- ContextPreset revision、assembly snapshot 和 read set；
- `LlmOperationExecutionPin`：channel revision、model、OperationKey、operation taxonomy version 和 adapter decoder version；
- Tool Registry snapshot、ToolGrant/HostedToolBinding；
- policy/schema version、limits 和 deadline。

`LlmOperationExecutionPin` 的 canonical 类型与 exact-version support matrix 见 `07-llm-channels-counting.md`。Waiting、resume 和 attempt retry 不自动读取 “latest” 替换这些语义配置；snapshot 中任一 taxonomy/decoder version 未知或 payload digest 不符都在 provider Effect 前 fail closed。每次危险动作仍重查 deny-only live revocation overlay。用户要采用新 grant/config 时创建新 GraphRun/candidate。

## Request 构造

```text
ContextAssemblyOutput
  instructions + initial messages + provenance + budget report
-> Request Builder
  model + ordered transcript + tools/hosted tools
  + response format + generation + safe extensions
-> LlmRequestIr
```

工具只从当前 ToolGrant 快照生成 model-facing descriptor。全局 registry 中存在但未授予的 name 不会进入 request，也不能在模型调用后动态兜底。

所有 budget/count 针对尽量 wire-equivalent 的完整请求，包括 tool/response schema。Shape adapter 在首次调用前验证 tool-loop same-shape round-trip 能力。

## Model Call Journal

发请求前持久化 logical model call、pending effect、request ref、continuation ref、计数和 prepared EffectAttempt。开始网络调用时把 attempt 标记 `started`，terminal response/result ref 落盘后同时收敛 attempt 与 logical effect。

普通 generation call 没有外部业务副作用时可按 idempotent/pure transport policy 重试。包含 hosted tool 时，classification 取所有 hosted binding 的最高风险；存在不可查询的 non-idempotent hosted effect 时，断线后进入 `outcome_unknown`，不能自动重发整个 request。

Provider retry 不增加 `maxModelCalls`；只有新的 logical request 增加。请求格式、permission、context overflow 等确定性错误不做网络 retry。

## Streaming Finalizer

Provider stream：

```text
wire delta
-> ShapeAdapter 顺序/去重
-> LlmStreamEventIr
-> ephemeral UI events
-> StreamFinalizer
-> exactly one durable model-call terminal
```

Finalizer 同时聚合有序 assistant message、tool call、hosted tool、reasoning summary、usage 和 opaque continuation。Tool argument delta 未完成前不能 dispatch。

Delta 默认不持久化；ordered response/transcript、terminal、usage 和 continuation ref 必须 durable。中断/截断流不能伪造 completed response。允许 partial node output 时必须由显式 policy 产生，与正常 finalized output 区分。

## Custom 与 Hosted Tools

Custom tool：runtime 使用 `RegisteredTool` executor，并把 canonical `llm_result` 作为下一轮 `tool_result` item 回填。

Hosted tool：provider 在 model call 内执行，不进入 local dispatcher，但必须来自 `HostedToolBinding`，并在请求前完成 scope/风险/费用/approval 校验。Provider 返回的 hosted trace/item仍进入 ordered transcript 和 effect audit。

完整 descriptor、grant、output union、effect 和 artifact 规则见 `19-tools-artifacts.md`。

## Tool Batch

一次 response 中的完整 custom tool calls 按原始 item 顺序分配 `callIndex`：

1. 持久化 response items 和 batch checkpoint；
2. 用 exposed name 定位当前 request 的 binding；
3. 校验 local ID、arguments schema/size、material refs 和 permission；
4. 把本 batch 需要 approval 的 calls聚合成唯一 durable WaitRecord；
5. 为可执行 call 创建/reuse NodeInstance-owned tool/effect ledger，并用当前 invoking NodeAttempt 创建 prepared EffectAttempt；approval resume 不改写 call 的 originating attempt；
6. 按 parallel policy 执行并先持久化 result/object；
7. 校验唯一 `ToolOutputPart` union；
8. 所有 sibling 收敛后按 `callIndex` 追加 tool result；
9. 更新 checkpoint，再发下一 logical model call。

并发完成顺序不改变 transcript。一个 sibling waiting 时，已完成结果可以 durable 保存，但不能提前发下一 model call。

只有 descriptor `supportsParallel`、policy 允许且 write/resource scopes 不冲突时才并发；否则按 callIndex 顺序执行。阶段一小并发有硬上限。

## Tool Output 分发

唯一 union 在 `19-tools-artifacts.md` 定义：

```text
llm_result               -> 有界、untrusted tool_result item
artifact                 -> staging/commit ArtifactRef；可由 result/ref 引用
state_patch              -> 暂存到当前 node transition，finalize 时校验/提交
memory_change_proposal   -> MemoryManager 创建 durable proposal，不直接 apply
user_message             -> 标注 tool provenance 的 UI event
evidence                 -> proposal/trace 引用
debug                    -> 默认 ephemeral、脱敏
```

WorkingContext StatePatch 在 NodeInstance finalized transaction 中提交；同一 batch 的 patches 按 callIndex 组合，path 冲突失败。工具不能让后续 sibling 从未提交的新 head 偷读；整个 activation 的 static/context read snapshot 固定。唯一例外是后续 logical call 显式请求的内建 `search_memory`，它按 `02-memory.md` 持久化独立 call-level snapshot/result，不改写 static read set。

Artifact bytes/metadata可在工具结果落盘时提交，由 tool call/effect 持有 ref；是否把 ref 写入 WorkingContext 仍等到 node finalize。Failed/cancelled run 的 artifact 按 retention/GC 处理。

MemoryChangeProposal 可以在 loop 中 durable 创建并进入 review/wait，但 speculative branch 不自动推进 LongTermMemory global head。

## Approval 与 Waiting

Approval 绑定 `toolCallId + callDigest + policyVersion + actor + expiry`。恢复时重新校验 grant/policy；arguments、materials 或 scope 变化会使批准失效。相同 digest 的不同 call 仍需逐 toolCallId 决策。

LLM loop waiting 记录：

- WaitRecord 与 response schema；
- `LlmLoopCheckpoint` / transcript / continuation；
- current tool batch、effect watermark 和 counters；
- deadline、delivery/idempotency data。

满足 wait 后继续同一个 logical activation，从已持久化 batch 恢复。普通聊天输入没有 wait ID 时创建新 Turn/GraphRun，不能注入该 loop。

## Tool Failure

受控失败可以变成 model-visible tool result，让模型解释或选其他能力：

```text
unknown name / invalid arguments / denied / ordinary executor error
```

是否回填由 `ToolFailurePolicy` 决定；错误必须有界、脱敏并计入 `maxToolCalls`。以下错误默认 fail node或等待协调，不能伪装为普通 tool result：

- registry/implementation digest 漂移；
- patch/artifact integrity invariant 失败；
- run fencing/control epoch 失效；
- non-idempotent `outcome_unknown`；
- checkpoint/transcript 损坏。

Tool retry 由 effect classification 决定。Timeout、连接断开、cancel 都不能单独证明外部副作用未发生。

## Preamble 与 Final Output

模型可能在 tool call 前产生可见文本。它可流给 UI并保留 transcript，但默认 final node output只取最后收敛轮的 assistant message items。

最终文本/JSON 的选取范围、part 过滤、无分隔连接和 exact JSON parser 只由 `10-llm-node.md` 定义。阶段一文本默认 `last_assistant_turn`；`all_assistant_text` 必须显式启用。Tool arguments/result、reasoning 和 hosted trace 永不混入 final text/JSON。

## JSON Output Repair

最终 JSON parse/schema 失败时，允许在同一 transcript 后追加受控 repair instruction并创建新的 logical ModelCall。已完成 tool/effect result直接复用，绝不从 activation 开头重跑工具。

Repair 消耗 `maxModelCalls`，受单独 `maxOutputRepairs` 限制。错误只给模型必要的 schema/path 摘要，不回填敏感 raw output。耗尽后 NodeInstance failed。

## Limits

`LlmNodeLimits` 的 canonical 类型与默认值见 `10-llm-node.md`。

- tool call 在完整请求被模型产生时计数，包括 invalid/denied；
- logical model call 进入 prepared 时计数，transport retry 不重复计数；
- `BaseNode.timeoutMs` 覆盖当前 attempt 内的 Context Assembly、model 和 tool 活跃执行；durable approval wait 结束当前 attempt，run deadline继续计时；
- provider/tool 自身还应用更小的 timeout/size limits；
- 到达限制默认 fail，不依赖模型自觉停止。需要人工介入时必须显式转换为 durable wait。

## Crash Recovery

恢复顺序：

```text
校验 execution snapshot/checkpoint digest
-> 恢复 ordered transcript/continuation
-> 读取 current tool/model effect ledger
-> prepared 可启动
-> started 先 query/deduplicate；无法判断的 non-idempotent -> outcome_unknown
-> succeeded 复用 resultRef
-> 收敛 batch并按 callIndex回填
-> 继续下一 model call或 finalize
```

任何恢复路径都不能仅因内存 task 消失就把外部 started effect 当作 failed，也不能在旧 attempt fencing token 失效后推进 branch。

Opaque continuation 在 resume 前校验 digest、OperationKey、operation taxonomy/adapter decoder version 和 `expiresAt`。未知/不匹配 version fail closed；已过期且无法从完整 durable transcript安全重建同 shape request时，节点失败为 `continuation_expired`/等待人工重新运行；不能为获得新 continuation 而重跑已完成 side-effect tools。
