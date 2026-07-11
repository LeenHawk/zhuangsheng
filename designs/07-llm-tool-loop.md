# LLMNode Tool Loop 与流式聚合

## ModelCall 与 LLMNodeRun

需要区分两个层次：

```text
ModelCall
一次原始 provider API request / response。

LLMNodeRun
一次图节点执行，可以包含多个 ModelCall 和多个工具执行步骤。
```

一次 `LLMNodeRun` 可以是：

```text
model call #1
  -> text delta / preamble
  -> tool call A
  -> tool call B

runtime dispatch tools

model call #2
  -> final answer

node completed
```

图调度层看到的是一个 `LLMNode`，trace 层可以看到多次 model call 和多次 tool call。

## Custom Tools 与 Hosted Tools

工具分两类：

```text
Custom tools
由本项目 runtime 执行，例如本地工具、memory tools、业务工具。

Hosted / built-in tools
由 provider 或上游平台执行，例如 provider 托管的 web search、file search、code execution。
```

Custom tools 的通用流程：

```text
provider 返回 tool call
runtime 执行工具
runtime 把 tool result 回填给 provider
provider 继续生成
```

Hosted tools 可能在一次 provider request 内由上游平台执行完成。对于本项目来说，它们作为 provider response / stream 中的 trace 或 output 事件处理，不进入本地 tool dispatcher。

## Provider 差异

OpenAI Responses / Chat Completions：

```text
一次 response 可能返回零个、一个或多个 function/tool call。
custom tool 执行后，runtime 再发下一次 model request。
Responses API 可以通过 previous_response_id 或显式 transcript 延续上下文。
```

Claude Messages：

```text
response 可以包含 text block 和一个或多个 tool_use block。
stop_reason = tool_use 时，runtime 执行工具并把 tool_result 发回下一次 Messages request。
```

Gemini GenerateContent：

```text
支持单轮多个 function call，也支持串行 / compositional function calling。
custom function 仍由 runtime 执行，并把结果作为后续请求内容交回模型。
```

统一语义：

```text
三家都支持 agent loop。
差别主要在工具由谁执行，以及 loop 是 runtime、SDK 还是 provider 平台托管。
```

## 状态机

LLMNode executor 可以按以下状态机执行：

```text
Start
  -> BuildRequestIr
  -> ModelCallStreaming
  -> AccumulateResponse
  -> HasToolCalls?
      -> DispatchTools
      -> AppendToolResults
      -> BuildNextRequestIr
      -> ModelCallStreaming
  -> FinalizeOutput
  -> NodeCompleted
```

限制由 `LlmNodeLimits` 控制：

```ts
type LlmNodeLimits = {
  maxModelCalls?: number
  maxToolCalls?: number
  timeoutMs?: number
  maxInputTokens?: number
  maxOutputTokens?: number
}
```

runtime 不能依赖模型自觉停止。达到限制后，节点应该失败、等待人工介入，或按配置返回 partial result。

## Codex 参考规则

`samples/codex` 的 turn loop 可以作为实现参考，但本项目不引入 Codex 代码依赖。

Codex 的核心结构类似：

```text
RegularTask
  -> run_turn
    -> run_sampling_request
      -> try_run_sampling_request
        -> stream events
        -> handle_output_item_done
        -> ToolCallRuntime
          -> ToolRouter
            -> ToolRegistry
```

关键规则：

```text
一次 turn / node run 可以包含多次 sampling request。
模型如果返回 tool call，runtime 执行工具，并把工具输出写回 history。
下一次 sampling request 从更新后的 history 构造 prompt。
模型如果只返回 assistant message，则 turn / node run 完成。
```

这对应本项目：

```text
ModelCall #1
  -> tool call
  -> dispatch tool
  -> append tool result to IR messages

ModelCall #2
  -> final answer
  -> NodeCompleted
```

## Streaming Tool Calls

流式响应可能包含自然语言 delta，也可能包含 tool call delta。

```text
text_delta
tool_call_delta
tool_call_completed
completed
```

`StreamFinalizer` 需要同时聚合文本和工具调用：

```text
text deltas
tool call name
tool call arguments deltas
usage
finish reason
```

工具参数未完整前不能执行工具。

通常 custom tool 执行后的继续生成需要下一次 provider request，而不是在同一个 HTTP stream 中继续。

Codex 的做法也是：流式 `ToolCallInputDelta` 只交给 argument diff consumer 做增量消费或 UI 事件，真正执行工具发生在完整 output item done 之后。

本项目也应遵守：

```text
tool_call_delta
  -> 聚合 / 展示 / trace

tool_call_completed 或 output item done
  -> dispatch tool
```

## 流式转非流式

LLMNode 可以边执行边发流式事件，但对下游节点来说，通常要等到该节点完成后拿到完整 `NodeResult`。

需要一个 stream finalizer：

```text
provider stream
  -> LlmStreamEventIr
  -> runtime stream events for UI
  -> StreamFinalizer
  -> final LlmResponseIr
  -> LlmNodeOutput
  -> NodeResult.completed
```

默认语义：

```text
streaming 只影响观察层，不改变图调度层。
```

下游节点和 Router 仍然基于 finalized output 执行。

## Preamble 与 Final Output

模型可能先输出一段可见文本，再请求工具。

```text
我先查一下资料。
<tool_call search>
```

这段 pre-tool text 可以流给 UI 或 trace，但默认不一定进入最终 `LlmNodeOutput`。

推荐默认策略：

```text
streaming text delta 可以展示给用户或 trace。
最终 LlmNodeOutput 默认取最后 final answer turn。
完整过程通过 trace 查看。
```

如有需要，可以增加输出策略：

```ts
type ToolLoopOutputPolicy = {
  includePreToolTextInFinalOutput?: boolean
}
```

## 多工具调用

一次 model call 可能返回多个 tool call。

runtime 应支持：

```text
parallel tool calls
同一轮返回多个工具调用，可并发或按配置限流执行。

serial tool calls
模型根据上一轮工具结果继续请求下一个工具。
```

执行策略：

```ts
type ToolExecutionPolicy = {
  parallel?: boolean
  maxConcurrentTools?: number
}
```

第一版可以先支持多个 tool call，并用小并发或顺序执行。所有 tool result 都 append 后，再进入下一次 model call。

Codex 的工具并发是 per-tool capability：工具声明是否支持并行，不支持并行的工具会被互斥执行。

本项目后续可以采用类似能力：

```ts
type ToolBinding = {
  name: string
  supportsParallel?: boolean
}
```

第一版可以先全局顺序执行或有限并发，后续再扩展到 per-tool 并发能力。

## Tool I/O

工具不应该只是 `JSON args -> JSON result`。

工具输入可以包含结构化参数和材料：

```ts
type ToolCallInput = {
  args: unknown
  materials?: ToolMaterialRef[]
}
```

工具输出可以分成多份 typed parts：

```ts
type ToolCallOutput = {
  parts: ToolOutputPart[]
}
```

典型输出部分：

```text
llm_result
给 LLM 继续推理用，通常应该短。

artifact
大内容持久化，不塞进上下文。

state_patch
影响 run state。

memory_proposal
提出记忆变更，但不直接落库。

user_message
需要直接展示给用户。

evidence
提供引用链。
```

只有 `llm_result` 部分回填给模型。

其他部分由 runtime 分发：

```text
artifact -> artifact memory
memory_patch -> memory manager
memory_proposal -> memory manager
user_message -> UI event
debug -> trace
```

这能保证 LLMNode 内部 tool loop 可控，同时不会把大对象或副作用结果全部塞回上下文。

Codex 的回填路径是：

```text
tool result
  -> ResponseInputItem
  -> ResponseItem
  -> working memory conversation
  -> next sampling request
```

本项目对应路径：

```text
ToolCallOutput.llm_result
  -> tool message / IR message
  -> next LlmRequestIr
```

工具失败不一定要让整个节点直接失败。可以按工具策略转换成 model-visible tool result，让模型有机会解释、重试或选择其他路径。
