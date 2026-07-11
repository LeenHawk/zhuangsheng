# RouterNode 与 Router DSL v1

## 定位

RouterNode 是确定性的 built-in control node。它负责规则判断、output port选择、fan-out、default route 和业务级 loop guard。

Router 不执行 LLM/tool、网络或文件 I/O，不写 WorkingContext，不创建 branch，不转换 payload，也不实现 join/aggregate。Edge 仍然只连接 output/input；Router fan-out 只在同一个 GraphRun execution namespace 内产生并行数据流。

本文固定 `router-dsl-v1`。GraphRevision 必须保存 DSL version和原始表达式；Apply 时解析校验。实现不能把“接近 CEL”当成可变语义，也不能调用 JavaScript、宿主回调或任意代码。

## Node 类型

```ts
type RouterNode = BaseNode & {
  kind: "router"
  dslVersion: "router-dsl-v1"
  inputs: InputPortDefinition[]
  outputs: OutputPortDefinition[]
  rules: RouterRule[]
  matchMode: "first" | "all"
  defaultOutputs?: PortName[]
  payloadPort?: PortName
  memory?: RouterMemoryBinding
  limits?: RouterLimits
}

type RouterRule = {
  id: string
  when: string
  outputs: PortName[]
}

type RouterReadBinding = StaticMemoryRead & {
  source:
    | { kind: "working_context"; scope: string; path: string }
    | { kind: "long_term_memory"; scope: string; query?: MemoryQuery }
  consistency: "snapshot" | "validate_on_commit"
}

type RouterMemoryBinding = {
  reads: RouterReadBinding[]
}

type RouterLimits = {
  maxVisitsPerRun?: number
  timeoutMsPerRun?: number
  maxReadReconciles?: number
  onLimitOutputs?: PortName[]
}
```

Applied Router 的唯一读取配置入口是 `memory.reads`。旧 draft 的顶层 `readBindings` 只能由 migration 在 Apply 前规范化；applied revision 出现该字段，或同时出现旧字段与 `memory.reads`，都是静态校验错误，runtime 不做 precedence 或合并猜测。

Applied Router 至少一个 input和一个 output。每条 rule outputs、defaultOutputs 和 onLimitOutputs 只引用已声明 ports，出现时必须非空且内部无重复。

`matchMode` Apply 后必须显式；draft 省略时规范化为 `first`。

`maxReadReconciles` 只限制 `validate_on_commit` 的 Router 专用重读，draft 省略时 Apply 规范化为 2，且必须小于 GraphRevision `maxAttemptsPerActivation`。它不是业务 loop visit，也不使用通用 RetryPolicy。

## Evaluation Environment

DSL 只有三个根对象：

```text
inputs
  consumer binding 和 schema validation 后的 port -> JSON value map。

memory
  memory.reads 按 as 命名后的 canonical BoundMemoryValue envelope map；optional miss 也保留 alias。

control
  当前 Router activation 的 durable visits、elapsedMs 和 limit 信息。
```

例：若 Router 声明名为 `score`、`route`、`payload` 的 input ports，可以写：

```text
inputs.score < 0.8 && control.visits <= 3
inputs.route in ["done", "stop"]
memory.scene.found && memory.scene.value.phase == "ending"
has(inputs.payload, "needsHuman") && inputs.payload.needsHuman == true
```

单默认 input port收到对象时应写 `inputs.default.score`，不能依赖自动 flatten。

DSL 无法访问 Secret、env、时间 API、run store、未绑定 Context、tool、network、filesystem或任意宿主对象。`control.elapsedMs` 由 runtime提供，不允许表达式自己读取时钟。

## 一致 ReadSet

Router activation 输入由 edge queue固定。NodeAttempt 准备阶段在一个存储快照中解析全部 `memory.reads`：

1. WorkingContext logical scope 解析到 GraphRun 的 contextId/branchId，LongTermMemory scope 解析到授权 record/global lineage；
2. 每个 binding解析到明确 commitId和ValueRef；
3. 所有条目写入该 NodeAttempt 的完整 `ReadSetEntry[]`；
4. `memory` 只从这些 refs构造，规则求值期间不再 live read；
5. decision event记录 read set refs，不复制完整敏感值。

`snapshot` 表示该 activation 对固定 commit求值，finalize不要求外部 head仍相同。`validate_on_commit` 表示 decision transaction必须验证对应 head/record仍是 read set commit；失败时整个 decision/emission不可见，并在 `maxReadReconciles` 内用 `invocationKind=reconcile` 的新 attempt/read set重算。这是无外部副作用 Router built-in 的唯一例外，不放宽普通 executor `refreshReadSet=never`。

一次 evaluation不能让不同 aliases来自不同时间的非一致查询。ReadSet和版本引用遵循 `16-domain-consistency.md`，禁止使用裸整数 memory version。

## JSON 到 DSL 类型映射

`router-dsl-v1` 使用以下封闭值域：

| JSON | DSL value |
| --- | --- |
| `null` | `null` |
| boolean | `bool` |
| string | Unicode string |
| integer number | signed 64-bit `int` |
| fractional/exponent number | exact base-10 `decimal` |
| array | ordered list |
| object | string-keyed map |

Decimal 最多 38 个有效数字，scale绝对值最多 18；解析后去除不影响数值的尾零并保存 canonical decimal。超出 int/decimal域、NaN、Infinity 和无法规范化的 number产生 `router_numeric_out_of_range`。DSL literal使用相同域。

V1 不提供算术运算。Numeric equality和排序把 int精确提升为decimal后比较，不经过 binary float。String equality按 Unicode scalar sequence精确比较；string排序按 Unicode code point顺序，不使用 locale。Array/object只支持 equality的逐值结构比较和专用函数，不支持 `<` 等排序。

## Missing、Null 与 Truthiness

`missing` 是字段或 index不存在时的内部 sentinel，不等于 `null`，也不能作为最终普通值。

- `has(object, key)` 是唯一把不存在字段变成 `false` 而不报错的操作；
- missing参与其他 operator/function/access 均产生 `router_missing_value`；
- `null == null` 为 true，`null != non-null` 为 true；
- 对 null做 field/index access、ordering、size或string function产生 type error；
- `when` 最终结果必须是 bool；null、number、string、list和object没有 truthiness；
- `&&`、`||` 使用左到右 short-circuit，未执行分支不产生 missing/type error。

因此配置应显式写：

```text
has(inputs, "score") && inputs.score >= 0.8
```

而不是依赖 missing被当成 false。

## Syntax 与 Operators

V1 支持：

- literal：null、bool、string、int、decimal、list；
- 根标识符：inputs、memory、control；
- object field、`["key"]` 和 list integer index access；
- `!`、`&&`、`||`；
- `==`、`!=`、`<`、`<=`、`>`、`>=`；
- `value in list`；
- 圆括号和白名单函数调用。

`in` 逐项使用 V1 equality；右侧必须是 list。不支持 assignment、mutation、loop、comprehension、lambda、ternary、object construction、arithmetic、regex、dynamic function lookup或用户函数。

## Function Whitelist

函数名、arity和类型固定：

```text
has(object, string) -> bool
size(string | list | object) -> int
contains(string, string) -> bool
starts_with(string, string) -> bool
ends_with(string, string) -> bool
lower_ascii(string) -> string
upper_ascii(string) -> string
```

`lower_ascii/upper_ascii` 只转换 ASCII code points，其他字符原样保留，避免 locale差异。V1 不提供 regex、Unicode locale case folding、日期或随机函数。

参数类型错误、越界 list index、未知函数和错误 arity均是 evaluation error，不返回 false。

## Complexity Limits

Apply 和 runtime 都执行固定上限：

```text
rules per Router                  <= 128
source bytes per expression       <= 4 KiB
AST nodes per expression          <= 256
AST nesting depth                 <= 32
literal list length               <= 128
resolved inputs + memory JSON     <= 1 MiB
single string bytes               <= 64 KiB
evaluation fuel per expression    <= 10,000
evaluation fuel per activation    <= 50,000
```

Workspace policy可以设置更低值，不能提高到超过 DSL v1上限。每次 field/index/operator/function都有明确定义的 fuel成本；遍历 list/object按访问元素计费。Fuel耗尽产生 `router_complexity_exceeded`。

DSL v1 固定以下可移植 cost schedule；实现和所选 evaluator crate 不能自行改价：

1. 每个实际求值的 AST node先同时从 expression fuel和activation fuel各扣 1。literal、root、field/index access、unary/binary operator和function call都各是一个 node；括号不单独计费。
2. `&&` / `||` 仍按左到右短路；未求值子树不扣费。其他 binary operator先按左到右求值 operands，再支付本操作的下述 surcharge。
3. string 的 `== != < <= > >=` 额外扣两侧 UTF-8 byte长度之和；numeric/null/bool scalar comparison无额外费用。
4. list/object structural equality在比较前对两侧各计算 `deep_cost` 并全额扣费，不因首个不等成员提前少扣。`deep_cost(null|bool|number)=1`，`deep_cost(string)=1+UTF-8 bytes`，`deep_cost(list)=1+sum(element deep_cost)`，`deep_cost(object)=1+sum(key UTF-8 bytes + value deep_cost)`；object key按UTF-8 byte lexical order遍历。
5. `value in list` 按list顺序逐项执行相同 equality；每个实际检查的元素先额外扣 1，再扣对应 equality surcharge，只计到首次匹配或列表结束。
6. `has`、field/index access和 `size` 无额外费用；`contains`、`starts_with`、`ends_with` 额外扣两个参数 UTF-8 byte长度之和；`lower_ascii`、`upper_ascii` 额外扣输入 UTF-8 byte长度。
7. 所有加法使用checked `u64`；overflow等同fuel不足。每个node先扣基础1；operands求值且missing/type/arity检查通过后，再在执行本操作前原子扣surcharge。任一计数不足都返回 `router_complexity_exceeded`，不能部分执行操作。

因此相同 AST 和输入在所有实现上消耗完全相同的 fuel。Apply-time AST/data上限与runtime fuel共同构成语义边界。实现仍可用wall-clock watchdog发现 evaluator/host fault，但 watchdog 只能产生 `router_evaluator_fault` 并隔离本次执行，不能伪装成 false、default route或 `router_complexity_exceeded`。

## Rule Evaluation 与错误

规则严格按 GraphRevision 中声明顺序求值。

`first`：第一条结果为 true的 rule决定 outputs，后续不求值。

`all`：求值全部 rules，按规则顺序收集所有 true rule的 outputs；同一 port重复出现时保留第一次，后续去重。

任一实际求值 rule发生 missing、type、numeric、fuel或 evaluator error时：

```text
Router NodeInstance failed
不继续后续 rule
不使用 defaultOutputs
不产生任何 edge emission
记录 router.decision_error 与 rule id/error code
```

错误不是 false。只有合法求值为 false才继续下一条 rule。

如果所有规则合法但没有匹配：存在 defaultOutputs时选择它们；否则失败为 `router_no_match`。Router 不静默吞掉路径。

## Output 顺序与 Payload

Selected ports顺序固定：

- `first`：匹配 rule内声明顺序；
- `all`：rule顺序，再按每条 rule的 outputs顺序，第一次出现优先；
- default/on-limit：各自配置顺序。

每个 selected port每 activation最多一个 emission。写 edge queue时先按 selected port顺序，再按 applied edge id顺序分配 run-local enqueueSeq。

Payload 规则：

```text
声明 payloadPort：payload = inputs[payloadPort]
未声明：payload = 按 applied input port顺序构造的完整 inputs object
```

Router 只转发同一个 immutable payload ValueRef，不添加字段、不按 route修改内容。多个 selected ports和广播 edges可共享 ValueRef。`payloadPort` missing在 applied graph中是静态错误；运行时 schema/binding失败在 Router activation前失败。

## Visits 与 Timeout 精确定义

Router control row以 `(runId, routerNodeId)` 为 key；Context branch固定在 GraphRun binding中，不参与 scheduler key。

每个通过 input contract的 Router NodeInstance恰好创建一个 durable visit snapshot。Retry/resume attempt不会增加 visits：

```text
旧 visits = n
当前 activation candidateVisits = n + 1
首次 activation candidateVisits = 1
```

创建 Router activation的事务锁定 control row、增加 visits并把以下 snapshot写到 NodeInstance：

```ts
type RouterControlSnapshot = {
  visits: number
  firstVisitedAt: string
  decisionAt: string
  elapsedMs: number
  limitReasons: ("max_visits" | "timeout")[]
}
```

首次 activation设置 `firstVisitedAt = decisionAt`，所以 elapsedMs为 0。后续使用数据库时间计算 `max(0, decisionAt - firstVisitedAt)`，包含 run waiting/interrupted wall time。

Limit 在普通 rules之前判断：

```text
maxVisitsPerRun存在且 candidateVisits > maxVisitsPerRun -> max_visits
timeoutMsPerRun存在且 elapsedMs >= timeoutMsPerRun       -> timeout
```

因此 `maxVisitsPerRun = 4` 时 visits 1、2、3、4可执行普通 rules，第 5 次直接 limit。若两个条件同时成立，limitReasons按 max_visits、timeout顺序记录。

发生 limit时不执行普通 rules/default：存在 onLimitOutputs则按其顺序选择；否则 Router failed为 `router_control_limit_exceeded`。Static SCC规则要求 onLimitOutputs离开原 cyclic SCC。

Router timeout是业务 guard，不暂停且不能替代 `17-runtime-control.md` 的 activation、queue、attempt和run wall-clock hard limits。即使 onLimit route误配，global limits仍会终止执行。

## Decision 与 Emission 原子性

Visit snapshot按 NodeInstance唯一记录，crash/retry不会重复增加。Router evaluator使用固定 inputs、ReadSet和control snapshot生成 decision plan；最终只能通过 runtime finalize事务提交。

事务必须原子完成：

1. 校验 run control epoch、attempt fence、NodeInstance和visit snapshot；
2. 校验 `validate_on_commit` ReadSet entries；
3. 写结构化 router decision或decision error；
4. finalize attempt/NodeInstance；
5. 对 selected ports写 finalized ValueRef和全部 edge queue items；
6. 写当前 node、下游 node和 settle_run durable wakeups；
7. 追加 run-local sequenced journal/outbox。

任一步失败则 decision和emissions都不可见。ReadSet CAS冲突时原子终结当前 attempt 为 `router_read_conflict`并创建 durable reconcile wakeup；新 attempt 只重解析 Router `memory.reads`，不消费第二次 input、不增加 visit、不改 control snapshot。耗尽 `maxReadReconciles` 时 NodeInstance 失败为 `router_read_conflict_exhausted`，不无界自旋。Hard cancel后的 late decision因 epoch/fence失效被隔离。

`router.decision` 至少记录：dsl version、matched rule ids、evaluated rule ids、selected ports、reason、control snapshot、ReadSet refs、payload ref和output refs。默认不复制完整 payload、memory value或secret。

## Static Validation

Apply GraphRevision时至少验证：

- dslVersion恰为 `router-dsl-v1`；
- rule id唯一，rules数量和每个 source/AST/深度在上限内；
- 表达式可解析，仅引用允许的根、operator和函数；
- `when` 可静态推断部分必须为 bool；动态部分运行时严格检查；
- outputs非空、无重复且全部存在；default/on-limit同样；
- matchMode合法，payloadPort存在；
- read binding id/as唯一，scope、path、schema、consistency和权限合法；
- Router limits为正且不超过 workspace policy；
- input/output/payload schema明显兼容；
- cyclic SCC中的 guard和onLimit出口满足 `11-graph-definition.md`。

Compiled AST可以缓存，但不是持久化权威。修改表达式、DSL version、bindings、rules顺序、ports或limits都创建新的 GraphRevision/contentHash。

## Example

```ts
{
  id: "route_after_critic",
  kind: "router",
  dslVersion: "router-dsl-v1",
  isEntry: false,
  inputs: [
    { name: "score", schema: { type: "number" }, binding: { selector: { type: "whole_value" } } },
    { name: "needsHuman", schema: { type: "boolean" }, binding: { selector: { type: "whole_value" } } },
    { name: "payload", binding: { selector: { type: "whole_value" } } }
  ],
  outputs: [
    { name: "retry" },
    { name: "done" },
    { name: "needs_human" },
    { name: "limit_reached" }
  ],
  matchMode: "first",
  payloadPort: "payload",
  rules: [
    { id: "human_review", when: "inputs.needsHuman == true", outputs: ["needs_human"] },
    { id: "retry_low_score", when: "inputs.score < 0.8 && control.visits <= 3", outputs: ["retry"] },
    { id: "finish", when: "inputs.score >= 0.8", outputs: ["done"] }
  ],
  defaultOutputs: ["done"],
  limits: {
    maxVisitsPerRun: 4,
    onLimitOutputs: ["limit_reached"]
  }
}
```

## Summary

Router DSL v1是固定、纯函数、无 truthiness的封闭语言。Missing、null、numeric和evaluation error都有明确语义；错误绝不被当成 false。Router从一次一致 ReadSet读取 memory，visits按 activation从 1开始且retry不重复计数。规则选择顺序和edge emission顺序完全确定，decision、NodeInstance finalize和emission同事务提交。Router limit只表达业务循环出口，runtime hard limits始终不可绕过。
