# Agentic Role Play 产品 UI

## 产品表面

产品首先是面向角色扮演、互动叙事和长期陪伴场景的 Agentic Role Play 应用。用户创建或选择角色、世界与故事，在连续会话中体验候选回复、记忆、工具行为和剧情分支。异步 Graph Runtime 是实现这些能力的基础，不要求普通用户理解节点、attempt、effect 或 schema。

```text
用户价值：角色持续存在、世界保持一致、故事可以分支、行动可以恢复。
技术支撑：GraphRun、Context、Memory、Tool、Event、Artifact、Branch。
```

默认界面使用 Role Play 语言；专家界面才使用 runtime 术语。Conversation 是用户看到的故事时间线，WorkingContext ancestry 才是其历史权威。

## 目标用户

### 普通用户

- 想快速选择角色并开始故事；
- 调整文风、回复长度、视角和常用模型；
- 使用 regenerate、候选切换、记忆和剧情回溯；
- 不希望接触 Graph、JSON Schema 或 token budget 细节。

### 高级创作者

- 制作角色卡、世界资料、开场和可复用故事模板；
- 理解 Context 顺序、长期记忆、工具权限和模型差异；
- 希望调试角色为什么作出某个行动，但不一定编写节点。

### Agent 专家

- 编辑 Graph、Router、协调节点、ContextPreset 和 ToolBinding；
- 查看 Run/Trace、NodeAttempt、Effect、StatePatch 和版本 diff；
- 处理恢复、冲突、未知副作用和兼容性问题。

高级创作者仍可停留在用户模式的展开设置中；不能把“会调 prompt”强迫等同于专家模式。

## 双模式模型

```ts
type UiExperienceMode = "user" | "expert"
```

模式是当前 principal 的 UI preference，可以按设备保存；permission 仍由服务端决定。切换模式不创建 revision，不改变故事，不终止 run，也不授予敏感数据访问权。

### 用户模式

主导航：

```text
首页
故事
资料库
  角色
  Persona
  世界与 Lore
  创作模板
记忆
设置
```

用户模式保留候选、分支、记忆、审批和恢复提示，因为它们是 Role Play 核心能力；只隐藏实现细节。

### 专家模式

在用户导航基础上增加：

```text
Agent Studio
  Graph
  Context
  Tools 与权限
运行
  Runs
  Trace
  Wait / Effect
状态
  Branch / Commit
  Memory proposals
  Artifact / Event
模型与 Channel
```

专家模式不是单独站点。故事页可以打开“检查本次运行”，Run 页也可以返回产生它的 Turn/Candidate。

## 同一 Source of Truth

用户表单是 canonical 领域配置的受限编辑器：

| 用户概念 | 权威领域对象 |
| --- | --- |
| 故事 | Conversation + active WorkingContext branch |
| 用户消息 | ConversationContext `/messages` append commit |
| 回复候选 | Turn Candidate + sibling GraphRun |
| 剧情回溯 | Conversation selection + branch/head CAS |
| 角色、Persona、世界资料 | versioned ContextPreset items、ArtifactRef、Memory binding |
| 角色长期记忆 | LongTermMemory record/proposal |
| Agent 模板 | Graph draft/applied revision |
| 模型 | LLMNode model ref + Channel revision |
| 文风和上下文策略 | ContextAssemblySpec + LLM generation config |
| 能力确认 | ToolGrant、Wait、Effect policy |

UI 不能保存另一份会参与执行的 `simpleSettings`。表单读取服务器生成的映射 view，保存时携带 expected draft token/head并调用 canonical Graph/ContextPreset application command；成功后以返回的 revision/version重新渲染。Role Play facade可以组合验证和字段映射，但不能把friendly DTO落成另一张执行配置表。

## User-mode Compatibility Profile

用户模式只能编辑可证明无损映射的 Role Play 配置。Phase-one compatible template满足：

- run input 为 `conversation_message_v1`；
- reply output 是 required/single `AssistantReplyPayloadV1`；
- 有唯一可识别的主生成 LLMNode；
- 该节点引用一个可编辑 ContextPreset或可映射的graph-inline ContextAssemblySpec；
- preset item ID/profile 能识别 character、persona、world/lore、history、summary 和 style；
- 常用 generation/model/tool permission 字段没有多个冲突 owner；
- 未包含用户模式无法保留的自定义节点配置。

Server/application service 返回 compatibility view，而不是让浏览器遍历任意 Graph 猜语义：

```ts
type RolePlayCompatibilityView =
  | { mode: "editable"; profileVersion: 1; editableFields: string[] }
  | { mode: "partial"; profileVersion: 1; editableFields: string[]; lockedReasons: string[] }
  | { mode: "expert_only"; reasons: string[] }
```

`partial` 中只允许保存列出的字段；未知 node/item 原样保留。`expert_only` 仍可用于 Conversation 运行，只是用户设置页显示只读摘要和“在专家模式编辑”。不提供“转换为简单模式”这种有损操作；需要简化时显式从模板创建新 draft。

## 信息架构

### 首页

- 最近故事和未完成生成；
- 需要用户处理的 approval、human input、secret unlock 或 effect 协调；
- 常用角色/模板和“开始新故事”；
- 本地模式的存储、锁定与离线状态。

### 故事

- 故事列表以 Conversation 为权威；
- 卡片展示角色/模板摘要、最后可见消息、active branch和运行状态；
- 不按所有 branch 的最新 timestamp 伪造 preview；
- 删除/归档要区分 Conversation retention 与底层共享 Memory/Artifact。

### 资料库

资料库是对 ContextPreset、Artifact 和可授权 Memory content 的 Role Play 投影。角色、Persona、世界和模板按 profile分类展示，不复制内容。导入先进入 draft/preview，明确显示字段映射、未知字段、资源引用和安全风险，确认后才发布 version。

### 记忆

普通用户看到“角色记住了什么”、来源、置信说明、适用范围和状态；编辑产生 MemoryChangeProposal，不直接改 projection。高级展开显示 evidence、expected head 和 version；专家模式可进入完整 proposal/commit view。

### 设置

设置按作用域分层，当前作用域始终显示在标题和保存按钮旁：

```text
应用设置       当前设备/账号的显示与默认行为
模型与连接     Channel、Secret、默认模型
创作模板设置   角色、世界、Context、能力与默认生成参数
当前故事设置   ConversationRunProfile，指定后续 Turn默认GraphRevision/reply key
专家设置       Graph、schema、limits、trace、raw capture
```

不同作用域不能在一个表单中一次“全部保存”。当前故事若要修改共享角色/世界，必须明确选择“更新共享模板”或“复制为故事专用模板”，随后更新ConversationRunProfile；不能悄悄改所有引用者。未保存离开需提示；server revision 已变化时显示字段级 diff，不能 last-write-wins。

## 设置分组与映射

### 应用设置

- 语言、主题、字体、字号、密度、reduced motion；
- 默认 UI 模式和启动页；
- 流式显示、自动滚动、候选手势和通知；
- 本地下载、备份和诊断偏好；
- 不包含会改变 GraphRun 语义的字段。

### 模型与连接

- Channel 名称、base URL、SecretRef、operation 和 model catalog；
- 默认模型只是新配置的选择，不改历史 run；
- Secret 明文使用专用 write-only flow；
- capability unknown/override 在用户模式显示解释和确认，不伪装为已支持。

### 角色与世界

- 角色身份、描述、说话风格、目标、边界、示例对话和头像；
- 用户 Persona、称呼、关系和可透露信息；
- 世界规则、地点、Lore 条目、触发条件和优先级；
- 开场、叙事视角、语言和内容边界；
- 保存到 versioned preset/artifact，不把图片 data URL塞进 draft JSON。

### 生成与上下文

- 模型、回复长度、温度等当前 shape明确支持的 generation 字段；
- history、summary、world info、memory 的启用、顺序和预算；
- 用户模式用“短/平衡/长”和解释性预算摘要，专家模式显示 exact token/overflow；
- Context preview 显示最终顺序、来源和裁剪动作，但敏感内容默认遮罩。

### 能力与安全

- 允许的工具类别、网络/文件能力和每次/总量限制；
- `always ask | ask for risky | use granted` 只是 policy preset，最终仍编译为 grant/approval规则；
- 高风险或 unknown effect不能被“少打扰”开关绕过；
- Adult/sensitive 内容设置与系统权限、模型 provider policy分别展示，不能合并成一个假开关。

## 核心用户旅程

### 首次使用

```text
初始化 Secret Store（可跳过本地无凭据模板）
-> 添加/选择 Channel 和模型
-> 选择角色/模板或创建草稿
-> 检查角色、世界、权限摘要
-> CreateConversation（写入default run profile）
-> 提交首个 Turn
```

每步可返回修改且不重复创建资源；使用 idempotency receipt恢复丢失响应。

### 日常对话

```text
打开 active branch
-> 输入消息
-> 展示生成/行动摘要
-> 接受候选或 regenerate
-> 必要时审批工具/输入
-> 继续、切换候选或查看记忆
```

Composer 提交后立即显示已持久化 user message；assistant streaming 是未提交 overlay，直到 Candidate ready才成为正式消息。

### 分支与回溯

用户从候选或历史 Turn选择“从这里继续”时，先显示将移动 active branch、后续旧消息仍保留和当前设置来源。确认后提交 selection/fork命令；UI不复制消息或本地重放旧 Turn。

### 专家诊断

从任意 assistant message进入其 `originRunId`，定位 GraphRevision、NodeInstance、ModelCall、ToolCall、read set和 commit。退出诊断后返回同一故事/Turn，不丢失滚动和候选位置。

## 产品状态与文案

用户模式把 runtime 状态翻译为可行动语言：

| Runtime | 用户模式 |
| --- | --- |
| queued/ready | 正在准备 |
| running | 角色正在回应/行动 |
| waiting approval | 需要你的确认 |
| waiting human response | 角色需要更多信息 |
| secret_store_unlocked | 需要解锁模型连接 |
| retry backoff | 暂时失败，准备重试 |
| interrupted | 已暂停，可继续 |
| projection_conflicted | 回复已生成，但故事已在别处分叉 |
| outcome_unknown | 外部操作结果无法确认，需要处理 |

不能把 failed统一写成“生成失败”。错误必须说明用户能否重试、继续旧分支、切换模型、解锁或进入专家诊断，同时保留 traceId。

## 明确边界

- 用户模式不承诺编辑任意专家 Graph；
- 角色/世界资料不会自动获得 tool、secret 或网络权限；
- UI mode 不改变 server authorization；
- 当前故事的默认选择由ConversationRunProfile持久化，实际历史仍由每个 Candidate 固定的 GraphRevision/NodeInstance snapshot解释；profile更新只影响后续 run；
- Conversation/RP 投影不能污染 core runtime 类型；
- 完整页面和组件规格见 `25-ui-screen-specs.md` 与 `26-ui-design-system.md`。
