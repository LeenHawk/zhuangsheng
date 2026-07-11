# UI 页面与交互规格

## 全局 Shell

### Desktop / Web

```text
┌──────────────────────────────────────────────────────────────┐
│ Product / Story breadcrumb      sync/lock     mode   account │
├──────────────┬───────────────────────────────────────────────┤
│ primary nav  │ route content                                   │
│              │                                                 │
│ contextual   │                                                 │
│ status       │                                                 │
└──────────────┴───────────────────────────────────────────────┘
```

- 顶栏始终显示当前故事/模板作用域、连接状态、Secret lock 和 user/expert 模式；
- 模式切换保留当前资源语义位置，例如 Story Turn ↔ origin Run，而不是跳回首页；
- 左侧导航在 user 模式使用 Role Play 术语，expert 模式增加 Studio/Run/State；
- command pending 只锁定相关 action，不用全屏 spinner阻塞可读历史；
- durable stale/offline 以顶栏状态和局部 banner表达，不清空已有 projection。

### Mobile

- 底部导航：故事、资料库、记忆、设置；
- 当前故事使用全屏单路由，候选和设置进入 bottom sheet；
- expert 模式只提供 node list、run summary、timeline和只读 diff，完整 Graph 编辑提示使用宽屏；
- 键盘弹出时 composer稳定贴底，不能把候选/approval按钮遮住。

## 首页

首页目标是“继续故事或处理阻塞”，不是展示 runtime 指标。

```text
继续故事             最近角色/模板
[Story cards]         [Library cards]

需要处理
[approval] [unlock] [conflict] [unknown effect]
```

Story card包含标题、主要角色、active branch摘要、最后一条可见消息、最近活动和状态。Running card订阅轻量 projection；页面不可为每张卡建立完整 token stream。

Empty state提供三条路径：从内建模板开始、导入角色/世界资料、进入专家模式创建 Agent Graph。首次运行缺少 Channel 时在流程内引导配置，不把用户丢到通用设置首页。

## 新故事向导

采用可返回的四步向导，所有选择先留在本地 draft；最终提交时才创建 Conversation/首个 Turn。

1. 角色与模板：选择 compatible applied revision，显示能力和内容边界。
2. 用户 Persona 与世界：选择已有 preset content或新建；明确本故事可见范围。
3. 模型与能力：模型、常用生成档位、工具/网络审批摘要；Secret locked时在此解锁。
4. 开场检查：角色、世界、Context preview摘要、首条消息和预计能力。

最终 action：

```text
确保所需 draft/version 已发布
-> CreateConversation并写default run profile
-> 使用返回 root head SubmitConversationTurn
-> 跳转 Story route并订阅 Candidate
```

响应丢失使用原 idempotency key恢复；返回上一步不重复发布资源。Apply/permission error定位到对应步骤和字段。

## Story / Conversation 页面

### Desktop 布局

```text
┌──────────── story title / branch / run status ───────────────┐
│                                                              │
│                    virtualized message stream                │
│            user message                                      │
│                        assistant candidate                    │
│                 tool/action/approval inline card              │
│                                                              │
├──────── candidate controls / context notices ────────────────┤
│ attach   composer                                  send/stop  │
└──────────────────────────────────────────────────────────────┘
 right drawer: Story settings / Memory / Branch / Run summary
```

消息流只按 active ancestry显示。历史分支消息不混入时间排序；branch badge打开历史面板。

### Message

- User message提交成功后显示 durable 状态，不显示虚假的逐字发送；
- Assistant 正式消息携带 Candidate位置、模型/角色摘要和可选 origin Run入口；
- ephemeral stream使用视觉上不同的“生成中”容器，断线清空后以恢复提示替代；
- saved partial 明确标为“保存的未完成回复”，不与正常 Candidate混淆；
- Markdown 禁 HTML；Artifact、引用和动作结果使用专用 renderer。

### Composer

- 支持文本、已授权 ArtifactRef和显式 user action；
- Enter/Shift+Enter 行为可配置且在移动端不依赖快捷键；
- send 前固定当前 expected head与 run spec，command pending时禁止重复提交；
- running 时主 action变为“暂停”，hard cancel放入二级确认菜单；
- 无 WaitRecord 的新输入永远创建新 Turn；等待表单不能被普通 composer替代。

### Candidate 控件

```text
←  2 / 4  →      regenerate      从此处继续      更多
```

- 按钮与 swipe 同时存在，键盘可访问；
- queued/running/failed candidate保留位置和状态，不只展示 ready；
- regenerate 创建 sibling run，不覆盖当前候选；
- 选择 ready candidate提交 selection CAS；
- 若已有后续 Turn，确认框展示将移动 active branch以及旧历史仍保留；
- conflict刷新权威 selection/head并提供比较，而不是本地强制切换。

### Inline Action Card

用户模式把 wait/tool/effect转换为有限卡片：

- Approval：工具名称、目的、数据范围、风险、approve/reject；
- Human response：由 response schema生成表单；
- Secret unlock：打开专用 write-only modal；
- Retry/interrupt：说明是否自动继续和 wall-clock deadline；
- Unknown effect：展示安全摘要和服务端允许的结论，危险操作引导专家或协调者。

卡片提交后保留不可变 decision摘要，不用 loading结束后直接消失。

## Story Settings Drawer

Drawer 使用分组导航，并固定显示作用域：`模板默认` 或 `当前故事后续回复`。

`当前故事后续回复` 的模型/Graph选择写ConversationRunProfile。角色、世界或Context内容来自共享模板时，编辑前必须选择更新共享模板或复制为故事专用资源；复制成功并Apply后再CAS更新profile。取消、冲突或部分失败不能留下只在浏览器生效的override。

### 角色

- 名称、头像、身份、性格、目标、说话风格；
- 内容边界和角色主动性；
- 示例对话单独列表，可排序且有 token 影响提示；
- 长文本采用 autosave local draft，但只有 Save/Publish产生 server version。

### Persona

- 用户称呼、简介、关系、偏好和允许透露的信息；
- preview明确哪些字段会进入模型 context；
- 敏感字段默认折叠，模板分享时默认排除。

### 世界与 Lore

- 条目列表 + 详情双栏；移动端 list/detail路由；
- 每项显示 enabled、触发方式、优先级、预算和来源；
- 用户模式提供“总是/相关时/关闭”，专家模式显示 exact selector；
- bulk import先显示 mapping/duplicate/unknown diagnostics。

### 风格与生成

- 叙事视角、语言、回复长度档位、创作倾向；
- 只显示当前 operation/model支持的参数；
- 参数旁显示“影响后续回复”，历史 Candidate不变；
- 用户档位映射到 exact值后，摘要可展开查看，不做隐式模型猜测。

### Context

- 用户模式显示可排序来源卡片：角色、Persona、世界、记忆、历史、摘要；
- 每卡显示 enabled、优先级、预计 token和 overflow结果；
- Preview 调用服务端并标注 metadata-only/local或remote count；
- 专家入口打开完整 ContextPreset editor，不在 Drawer 嵌套复杂 JSON。

### 能力与安全

- 工具类别、资源范围、网络/文件能力和审批档位；
- “始终询问”与 grant存在与否分开显示；
- 权限变更生成新配置/overlay，只影响允许的后续行为；
- Secret、成人内容和 provider policy分别呈现。

### 保存行为

- Footer显示 unsaved、base revision和影响范围；
- Save前服务器返回 normalized diff/diagnostics；
- editable profile发布 preset/graph version，partial profile只提交允许字段；
- revision conflict展示 base/current/yours，不自动覆盖；
- expert-only显示只读摘要和“打开专家模式”。

## 资料库

角色、Persona、世界和模板共享搜索/标签/导入框架，但使用不同卡片和编辑表单。列表只显示可授权 metadata；内容预览受 permission和sensitivity限制。

详情页包含：当前 published version、draft状态、使用它的故事、Artifact/Memory引用和版本历史。删除默认是归档/停止新使用；已被历史 Run snapshot引用的 version仍可审计。

导出必须声明是否包含 Artifact、Memory、SecretRef和私有 Persona。SecretValue永不导出；缺失资源形成显式 manifest diagnostics。

## Memory 页面

用户视图按角色/故事/scope分组显示 active、proposed、obsolete；每条包含摘要、reason、evidence来源和最近更新时间。

- “更正”创建 replace proposal；
- “忘记”创建 tombstone proposal并说明历史审计仍存在；
- approve/reject/apply各自展示状态，不用一个 toggle伪装；
- expert视图增加 commit、policy version、search projection和read-set使用记录。

## Branch 与历史

用户模式采用故事时间线：Turn、candidate和分叉点。选择任一点显示“预览此分支”，只有确认后才改变 active pointer。

专家模式使用 branch tree + commit list + JSON Pointer diff。Merge conflict逐 path显示 base/source/target与选择；用户模式不暴露任意 merge，只提供受支持的“从这里继续”和候选选择。

## Expert Agent Studio

```text
┌ node palette ┬──── React Flow canvas ────┬ inspector ┐
│ search       │ nodes / ports / edges     │ config    │
│ templates    │ diagnostics overlay       │ schema    │
├──────────────┴────────────────────────────┴───────────┤
│ diagnostics / diff / apply result / revision history │
└───────────────────────────────────────────────────────┘
```

- Draft token和Apply状态始终可见；
- Inspector按 node type加载明确表单，不传巨大 config object；
- Context、schema和Router DSL使用专用编辑器；
- user-mode compatible字段带映射标记，破坏兼容前显示影响；
- Applied revision只读，编辑必须创建/切换 draft。

## Expert Run / Trace

Run detail使用 Graph、Timeline、Inspector三视图共享 selection：选中 NodeInstance后过滤 attempts/model/tool/effect/event，但 durable sequence顺序不变。

- Graph overlay只显示权威状态和queue计数；
- Timeline可按 importance/type过滤，不能重排为伪时间线；
- Inspector显示 execution snapshot、read set、checkpoint、output/ref和安全错误；
- raw/prompt默认遮罩并需permission/reveal；
- interrupt/resume/cancel固定 expectedEpoch；conflict刷新RunView。

## 全局设置

页面分为 Appearance、Behavior、Models & Connections、Storage & Backup、Privacy & Diagnostics。

Secret Store状态在 Models & Connections 顶部独立显示。Initialize/unlock/change password使用专用 modal；关闭或失焦清空字段。Channel编辑不把API key混入普通表单，而只选择 SecretRef。

默认模式、stream呈现和candidate手势是UI preference；默认模型/模板影响新选择；任何设置项都必须标注作用域与“是否影响已运行历史”。

## 状态矩阵

每个 route至少设计：loading、empty、ready、command pending、stale、offline、permission denied、not found、conflict和decoder incompatible。

Story额外覆盖 streaming、waiting、interrupted、candidate failed/projection conflicted；Studio覆盖invalid draft/apply diagnostics；Run覆盖retention expired和unknown critical event；Settings覆盖locked、unsupported capability和partial compatibility。

## 键盘与可访问性

- `Cmd/Ctrl+K` 打开资源/命令搜索，不在输入框劫持文本；
- candidate 前后切换有按钮和可配置快捷键；
- Graph canvas所有 node/edge可通过列表替代路径访问；
- modal/sheet遵循 focus trap与恢复；
- 状态不只靠颜色，stream不逐token aria-live；
- reduced motion关闭候选滑动、画布飞行动画和流式光标效果。

## 验收场景

1. 新用户不进入专家模式完成 Channel→角色→故事→首个回复。
2. 用户 regenerate并切换候选后，active ancestry与服务端一致。
3. 已有后续消息时切换旧 Candidate得到明确分支确认。
4. Tool approval、Secret unlock、human input不会被普通 composer误提交。
5. 专家改出 incompatible Graph后，用户设置不覆盖未知配置。
6. 从 assistant message可追到 Run，再无损返回原 Turn/Candidate。
7. 断线重连清除live overlay并从durable cursor恢复正式结果。
8. Mobile不依赖hover、右键、精细拖拽或仅手势操作。
