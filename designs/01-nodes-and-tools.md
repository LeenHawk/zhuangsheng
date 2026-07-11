# 节点与工具边界

## 节点是语义与恢复边界

Graph node 表示一次可独立观察、调度和恢复的执行阶段。Node definition 是静态配置；每次 firing 创建 NodeInstance，每次 retry 创建该 instance 下的新 NodeAttempt。

图不应该表达每个 HTTP request、token 或底层函数调用。它表达稳定的数据流、控制流、资源权限和生命周期边界。

正式节点结构、ports 与 revision 见 `11-graph-definition.md`；本文件不再维护另一份简化 `LLMNode` 类型。

## LLMNode

`LLMNode` 是一次 LLM 驱动的语义阶段，不等于一次 provider request。一个 NodeInstance 可以包含：

```text
Context Assembly snapshot
-> model call #1
-> custom/hosted tool calls
-> durable tool results
-> model call #2
-> finalized text/JSON output
```

Graph scheduler 只基于 finalized output 推进 edge；model/tool/delta 通过内部 trace 和 event 观察。结构、恢复和限制见 `10-llm-node.md` 与 `19-tools-artifacts.md`。

## 工具默认不是图节点

服务于模型推理过程的能力保留在 LLMNode 内：

```text
ResearchLLM[search_memory, web_search]
  -> DraftLLM
  -> CriticLLM
  -> OutputNode
```

这样图保持语义清晰，但工具不会成为黑盒：每个调用仍有 descriptor/grant、schema validation、approval、effect ledger、result ref 和 durable events。

## 何时提升为节点

满足任一条件时，应考虑把操作建模为明确 graph node，而不是 LLM tool：

- 由图确定触发，不需要模型决定是否调用；
- 有独立等待、retry、deadline、人工确认或补偿流程；
- 权限/审计边界需要在图中可见；
- 结果要被多个节点复用或形成稳定业务阶段；
- 非幂等副作用需要独立恢复与 outcome reconciliation；
- 即使没有 LLM，该步骤仍有完整业务含义。

例如 `DeployService`、`CollectApproval`、`ChargeAccount` 不应因为技术上可包装成 function tool 就藏进任意 LLMNode。

提升为节点不代表提供通用 `ToolNode`。阶段一按稳定业务语义实现 executor/node kind；不设计可执行任意代码的插件节点。

## 确定性操作

如果 runtime 已知触发时机、目标和 value source，就不应浪费一次 LLM tool call：

```text
读取固定 WorkingContext path       -> MemoryBinding read
写 finalized output 到固定 path     -> StaticContextWrite / StatePatch
按 score 选择 route                 -> RouterNode
把数组逐项发射                       -> ExpandNode
按 key 等齐多个结果                  -> JoinByKeyNode
```

需要语义检索、判断是否值得长期保留或提出修改时，才使用 `search_memory` / `propose_memory_change` 等高层 capability。

## Router 与协调节点

RouterNode 负责无副作用的条件选择和 fan-out，不执行 tool、不写 Memory、不创建 branch。

Merge/JoinByKey/Aggregator/Expand 负责确定性数据协调。Edge 只连接 output/input port，不隐藏 condition、mapping、join 或 patch。

## 权限组合

节点的有效能力是多层 grant 的交集：

```text
workspace/run policy
∩ applied graph revision grants
∩ node binding
∩ tool/Memory descriptor requirements
∩ 当前 actor permission
```

LLM 只能看到通过交集且对当前 API shape 可表达的 tool descriptor。模型输出一个未授予工具名时，dispatcher 返回受控 `tool_not_granted`，不能从全局 registry 自动兜底命中。

## 可观察性

Node-level trace：

```text
scheduled -> attempt started -> waiting/retry -> completed/failed
```

LLMNode 内部 trace：

```text
assembly snapshot
model call(s)
tool call(s) / approval / effects
stream terminal
output validation
```

大 payload、tool arguments 和 provider raw response保存受限 ref/脱敏 preview；Secret 永不进入 trace。Ephemeral delta 可以丢失，terminal、effect、wait 和 node completion 必须 durable。

## 阶段一取舍

阶段一提供固定 node kinds、Tool Registry port 和显式 grant，不提供动态插件 ABI、任意脚本节点或默认 ToolNode。出现第三个稳定的独立业务 executor 后，再评估注册式 node extension；不能提前让 core 类型和错误模型依赖插件系统。
