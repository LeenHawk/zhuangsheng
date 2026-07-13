# SillyTavern 预设与正则兼容

## 目标

庄生允许用户导入 SillyTavern 预设和正则脚本，并把可执行语义转换为庄生的版本化配置。兼容层不是第二套 runtime：新 Run 仍只固定 `GraphRevision`、`ContextPresetVersion`、Channel revision 和标准 LLM IR。

首个兼容版本覆盖当前 SillyTavern 样本中的：

- OpenAI Chat Completion preset：`prompts`、`prompt_order`、生成参数和 `extensions.regex_scripts`；
- master preset 中的 `context`、`instruct`、`sysprompt`、`reasoning`、text completion section；
- 独立或角色卡内嵌的 `regex_scripts` 数组；
- 正则 placement、depth、disabled、prompt/display ephemerality、edit flag、macro substitution、trim strings 和捕获组 replacement。

不在庄生中存在的 STscript slash-command runtime 只在前端 preview 中标记 inactive，不发布到后端；导入不能伪装为已经执行，也不承诺该字段可往返导出。

## 权威对象

```text
Browser / Tauri WebView
  SillyTavern JSON
    -> @zhuangsheng/sillytavern-compat
    -> local detect / preview / regex test / import / export
    -> generic ContextAssemblySpec + generation + provider extensions
    -> existing preset / role-play API clients

Backend
  generic ContextPresetVersion + GraphRevision
    -> format-neutral TextTransformRule runtime
```

`packages/sillytavern-compat` 是唯一认识酒馆格式、字段名和迁移规则的模块。core、storage、Axum、Tauri adapter 和通用 api-client 不定义 `SillyTavern*` 类型，也不提供酒馆专用 route/command。

preview 在前端计算原始文档的 format kind 和 canonical content hash用于当次确认；发布只传输并持久化规范化后的 `ContextPresetVersion` / `GraphRevision`，不上传可能夹带连接字段或 secret 的原始 JSON。export 在前端从已加载的通用版本投影重建规范化酒馆文档，因此只保证已支持语义往返，不承诺字节级复刻。执行时不得回读原始 JSON推导 prompt。

正则转换是 `ContextAssemblySpec` 的版本化组成部分。它随 preset content hash、execution snapshot 和 assembly digest 固定；规则顺序不可由数据库返回顺序或 UI 排序隐式决定。

## 预设映射

OpenAI preset 的 `prompt_order` 选择 `character_id = 100001`（含 persona）的顺序，缺失时回退第一项。enabled entry 决定参与 assembly 的 prompt；`prompts` 中 marker引用 canonical Role Play item，文本 prompt转换为 literal item。

内建 marker映射：

| SillyTavern identifier | 庄生语义 |
| --- | --- |
| `charDescription`、`charPersonality` | `character` |
| `personaDescription` | `persona` |
| `scenario` | `world` |
| `worldInfoBefore`、`worldInfoAfter` | `lore`，保留相对顺序 |
| `dialogueExamples` | `examples` |
| `chatHistory` | `history` |
| `main`、`nsfw`、`jailbreak` 和 custom prompt | versioned literal instruction |

导入到已有 ContextPreset 时 marker只重排/启停匹配的 canonical item，未知内容不覆盖。新建导入缺少角色、世界或绑定来源时 preview必须提示 unresolved，不生成假文本。

通用生成参数映射到 `GenerationOptionsIr`：temperature、top-p、max output tokens、stop和有效 seed。top-k、min-p、frequency/presence/repetition penalty、reasoning、prefill等标准 IR暂未表达的字段进入显式 provider extension或 locked warning，不能静默丢弃。连接地址、proxy password、API key不随预设导入。

## 正则语义

前端导入器把 SillyTavern 的 global -> character -> preset 来源优先级编译为通用 numeric `priority`，并保留每个来源内的数组顺序。后端运行时只按 `(priority, order)` 排序，不认识酒馆 scope。导入单个 preset时其 embedded rules使用 preset priority；角色卡规则使用 character priority。

placement映射：

```text
1 USER_INPUT    -> user/history text
2 AI_OUTPUT     -> assistant/history text和最终回复
3 SLASH_COMMAND -> preserved inactive（庄生无 STscript）
5 WORLD_INFO    -> world/lore candidates
6 REASONING     -> reasoning transcript/display
0 legacy MD     -> 按酒馆 migration变为 display + prompt
4 legacy sendAs -> 按酒馆 migration变为 slash-command inactive
```

surface规则与酒馆一致：

- `promptOnly`：只改变发送给模型的临时文本；
- `markdownOnly`：只改变 UI display projection；
- 两者都为 false：改变 canonical user/assistant reply文本，并由后续 prompt自然读取；
- 两者都为 true：prompt和display都应用，canonical文本不改。

`minDepth/maxDepth` 使用 0 表示最新消息。只有消息、world info和reasoning候选有 depth；system utility prompt不借用伪 depth。

前端导入器接受酒馆 `/pattern/flags` 格式，并在发布前把酒馆专用 `{{match}}` 编译为通用 `$0`。规范化 replacement 支持 `$0`、`$1..$99`、`$<name>`和显式宏；宏只来自版本化 `textTransformMacros` / 当次通用执行上下文，不能读取全局 secret/state。未知酒馆宏按兼容策略保留并产生 warning。

## 安全与确定性

- 每个 preset最多 256 条正则；pattern/replacement各 64 KiB；单次输入 4 MiB；输出 8 MiB；
- regex engine必须有 backtrack/step limit，limit耗尽返回 typed failure；
- invalid pattern在 preview中定位到 rule id，发布 fail closed；
- 规则执行结果、规范化规则版本和surface进入 provenance/trace；原始 source hash只用于当次前端确认，不进入后端执行状态；
- display-only转换不写 ConversationContext；prompt-only转换不写消息；
- secret、header、connection字段永不进入导入预览或 artifact导出。

## 前端工作流与 UI

Web 和 Desktop 复用同一个前端 compatibility package。detect、preview、regex test 和 export 都在浏览器/Tauri WebView 内执行；apply 只编排现有的 `create/publish ContextPreset` 与 `create RolePlay template` 通用调用。不存在 `/v1/compatibility/sillytavern/*`，也不存在 `preview_sillytavern_import` 等 Tauri command。

导入先显示识别类型、prompt顺序、生成参数、正则数量、inactive/locked字段和目标 preset diff；用户确认后才发布 canonical revision。原始 JSON、proxy、credential 和自定义连接字段不会进入通用 command body。

`apply import` 可选目标 Channel。选择后，同一个前端幂等工作流会发布或选定 `ContextPresetVersion`，再创建固定导入 generation/provider extensions 的 Role Play `GraphRevision`；generation-only 文件必须与一个已发布的 ContextPreset组合，不能生成空角色。

`test regex` 复用前端 preview 的规范化规则，并接收显式 target/surface/depth 和宏上下文。它使用浏览器 JavaScript `RegExp` 提供酒馆迁移前的交互试跑；真正发布仍必须通过后端通用 pattern validator，运行时以格式无关的 Rust transform engine 为权威。两者差异必须 fail closed 或显示 warning，不能靠酒馆专用后端分支补齐。

export 以已加载的 `ContextPresetVersion` 为基础；调用方若同时提供通用 generation/provider extensions，可一并输出。前端根据 numeric priority 重建 preset/global/character 文档，避免把规则错误降级成同一 scope；headers、连接字段和非白名单 provider option永不导出。

用户模式提供“导入酒馆预设”向导和规则启停/测试；专家模式显示原始字段映射、pattern flags、placement、depth、provenance和失败原因。任何 invalid/inactive规则都不能被绿色“已兼容”状态掩盖。

## 验收

- 使用 `samples/SillyTavern/default/content/presets` fixture验证格式识别与映射；
- 使用酒馆 regex engine等价 fixture验证 flags、捕获组、trim、depth、surface和顺序；
- preset发布后修改原始文件不影响已创建 Run；
- prompt-only/display-only不污染持久消息；canonical output规则在 candidate commit前只执行一次；
- Web 与 Desktop 使用同一个 compatibility package，对同一 JSON 产生相同 canonical spec/hash；
- 后端 crate、HTTP route、Tauri command 和通用 api-client 中不存在酒馆专用类型或 dispatcher；
- import -> export -> import保留所有已支持的 active 字段；inactive字段只在当前导入 preview 中审计，不冒充可持久化往返。
