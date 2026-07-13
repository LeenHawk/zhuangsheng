# SillyTavern 预设与正则兼容

## 目标

庄生允许用户导入 SillyTavern 预设和正则脚本，并把可执行语义转换为庄生的版本化配置。兼容层不是第二套 runtime：新 Run 仍只固定 `GraphRevision`、`ContextPresetVersion`、Channel revision 和标准 LLM IR。

首个兼容版本覆盖当前 SillyTavern 样本中的：

- OpenAI Chat Completion preset：`prompts`、`prompt_order`、生成参数和 `extensions.regex_scripts`；
- master preset 中的 `context`、`instruct`、`sysprompt`、`reasoning`、text completion section；
- 独立或角色卡内嵌的 `regex_scripts` 数组；
- 正则 placement、depth、disabled、prompt/display ephemerality、edit flag、macro substitution、trim strings 和捕获组 replacement。

不在庄生中存在的 STscript slash-command runtime 会被完整保留并标记 inactive；导入不能伪装为已经执行。

## 权威对象

```text
SillyTavern JSON
  -> parse + detect + validate
  -> import preview (mapping / warnings / inactive features)
  -> ContextAssemblySpec + GenerationOptionsIr + provider extensions
  -> publish ContextPresetVersion + GraphRevision
```

原始文档的 canonical bytes、format kind 和 content hash作为来源 metadata 保留，便于审计和重新导出。执行时不得回读原始 JSON推导 prompt。

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

规则执行顺序保持 SillyTavern 的 global -> scoped -> preset来源优先级，以及每个来源内的数组顺序。导入单个 preset时其 embedded rules作为 preset scope；角色卡规则作为 scoped scope。

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

find pattern接受酒馆 `/pattern/flags` 格式，replacement支持 `{{match}}`、`$0`、`$1..$99`、`$<name>`和宏。宏查找只从显式 `SillyTavernMacroContext`读取；未知宏按兼容策略保留并产生 warning，不能读取全局 secret/state。

## 安全与确定性

- 每个 preset最多 256 条正则；pattern/replacement各 64 KiB；单次输入 4 MiB；输出 8 MiB；
- regex engine必须有 backtrack/step limit，limit耗尽返回 typed failure；
- invalid pattern在 preview中定位到 rule id，发布 fail closed；
- 规则执行结果、规则版本、surface和source hash进入 provenance/trace；
- display-only转换不写 ConversationContext；prompt-only转换不写消息；
- secret、header、connection字段永不进入导入预览或 artifact导出。

## API 与 UI

Web 和 Tauri提供同构命令：detect/preview、test regex、apply import、export。导入先显示：识别类型、prompt顺序、生成参数、正则数量、inactive/locked字段和目标 preset diff；用户确认后才发布 canonical revision。

用户模式提供“导入酒馆预设”向导和规则启停/测试；专家模式显示原始字段映射、pattern flags、placement、depth、provenance和失败原因。任何 invalid/inactive规则都不能被绿色“已兼容”状态掩盖。

## 验收

- 使用 `samples/SillyTavern/default/content/presets` fixture验证格式识别与映射；
- 使用酒馆 regex engine等价 fixture验证 flags、捕获组、trim、depth、surface和顺序；
- preset发布后修改原始文件不影响已创建 Run；
- prompt-only/display-only不污染持久消息；canonical output规则在 candidate commit前只执行一次；
- HTTP 与 Tauri preview/apply产生相同 canonical spec/hash；
- import -> export -> import保留所有已支持字段，inactive字段仍可审计。
