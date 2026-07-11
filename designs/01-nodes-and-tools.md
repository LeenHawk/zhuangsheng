# 节点与工具边界

## LLMNode 的职责

`LLMNode` 代表一次 LLM 驱动的语义阶段，而不只是一次简单的 `prompt -> text` 调用。

它可以负责：

- 组装 prompt 和 messages
- 调用模型
- 执行 tool-calling loop
- 解释工具结果
- 生成结构化输出
- 记录节点内部 trace

示例结构：

```ts
type LLMNode = {
  id: string
  model: string
  prompt: PromptSpec
  inputSchema?: Schema
  outputSchema?: Schema
  tools?: ToolGrant[]
  memory?: MemoryBinding
  maxToolCalls?: number
}
```

## 工具不是默认图节点

工具默认是 `LLMNode` 的 capability，而不是图节点。

例如：

```text
ResearchLLM[tools: search, browser]
  -> DraftLLM
  -> CriticLLM
  -> Output
```

这里 `search` 和 `browser` 是 `ResearchLLM` 内部的工具，而不是图上的节点。

这样做的好处是图不会退化成一堆底层 API 调用编排，仍然保持语义阶段的清晰度。

## 什么时候工具应该提升为节点

默认规则：

```text
如果工具服务于 LLM 的推理过程，保留在 LLMNode 内部。

如果工具本身是一个独立语义阶段，并且有自己的生命周期，就提升为 graph node。
```

例如：

```text
AnalyzeRequirements
  -> DeployService
  -> VerifyDeployment
```

`DeployService` 虽然技术上可以被看作工具，但它有独立权限、重试、审计和恢复需求，因此适合作为图节点。

## RouterNode

`RouterNode` 是必要的图节点。

它负责条件控制流，而不是底层工具调用。

示例：

```text
Input
  -> ContextAssembly
  -> MainLLM
  -> Router
      -> Output
      -> MemoryReflectionLLM
      -> AskUserOutput
```

路由依据可以是结构化节点输出、运行时状态或显式条件。

## 节点内部 trace

虽然工具调用不默认出现在图上，但它不能变成不可观察的黑盒。

`LLMNode` 内部应该产生细粒度 trace：

```text
LLMNode: Research
  model call #1
  tool call: web_search
  model call #2
  tool call: fetch_url
  model call #3
  output: {...}
```

图保持干净，但调试、审计和回放能力不能丢。
