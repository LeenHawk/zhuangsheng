# UI 视觉与组件系统

## 设计方向

视觉需要同时支持两种体验：用户模式沉浸、温暖、以内容为主；专家模式高信息密度、精确、适合长时间观察。两者共享 token 和组件，不创建两套主题。

```text
Role Play surface   叙事内容优先、控制克制、角色与世界有辨识度
Expert surface      结构优先、状态明确、数据可比较、操作可追溯
```

避免把产品做成霓虹“AI 仪表盘”、传统后台管理模板或装饰过度的游戏 HUD。角色主题可以影响封面、头像和局部 accent，不能改变错误、审批、权限等语义色。

## Token 层级

使用 CSS variables + Tailwind semantic utilities。组件只能消费语义 token，不能散落具体色值。

```text
primitive     neutral/brand色阶、spacing、radius、font、shadow
semantic      background/surface/text/border/accent/status/focus
component     message/node/panel/timeline等少量局部token
```

Light、Dark 和 High Contrast 使用相同 token 名。用户自定义主题只能覆盖允许的 brand/cover/accent token；status、focus、contrast和安全遮罩不能被覆盖。

## 色彩语义

| Token | 用途 |
| --- | --- |
| `bg-canvas` | 页面和 Graph 画布底色 |
| `bg-surface` | 卡片、message、panel |
| `bg-elevated` | popover、dialog、sheet |
| `text-primary` | 正文和主要标签 |
| `text-secondary` | metadata、帮助信息 |
| `border-default` | 普通边界 |
| `accent` | 当前角色/产品主操作 |
| `status-info` | queued、准备、同步 |
| `status-running` | streaming、running |
| `status-success` | ready、completed、applied |
| `status-warning` | waiting、retry、partial、stale |
| `status-danger` | failed、denied、destructive |
| `status-unknown` | outcome unknown、兼容未知 |

状态色必须同时配图标、文本或纹理。`warning` 不是所有非成功状态的兜底；waiting、conflict和unknown使用不同文案与图标。

对比度至少满足 WCAG AA。正文、表单和状态标签在 Light/Dark均用自动化 token contrast test。

## 字体与排版

- UI 字体使用跨平台 sans-serif stack；CJK 和 Latin保持接近的 x-height与字重；
- Role Play 正文可以选择用户字体，但 command、状态、表单和代码仍使用系统 UI 字体；
- code、ID、hash、JSON Pointer使用 monospace；
- 默认正文 15–16px，移动端输入不低于16px以避免浏览器缩放；
- 行宽：叙事正文约 62–76 个 Latin字符或 30–42 个汉字；专家表格不套正文行宽；
- 标题层级不超过四级，Inspector用 section label而不是不断缩小标题。

流式文本沿用正式消息字号和行高，不能用跳动字号制造“AI感”。Reasoning/trace摘要视觉弱于正式角色回复。

## 间距、圆角与阴影

Spacing使用 4px基准的有限 scale。高频专家表格可用 compact密度，但点击目标仍不低于 32px；用户模式主操作目标不低于 40px，移动端不低于44px。

- message/card：中等 radius、低阴影或无阴影；
- dialog/sheet：更高层级 shadow；
- Graph node：小 radius和清晰边界，避免像普通卡片；
- destructive/approval 不通过夸张圆角或动画弱化严肃性。

## Icon 规则

使用单一图标库并包一层语义 Icon component。图标不能单独承担 candidate、branch、approval、secret lock等关键含义；必须有 label或accessible name。

Role Play领域优先使用明确图标：story、character、persona、world、memory、branch。Runtime使用 node、attempt、tool、effect、event、checkpoint。不要用同一“闪光”图标表示所有 AI 功能。

## 基础组件

基础层基于 shadcn/ui + Radix：

```text
Button / IconButton / Link
Input / Textarea / Select / Combobox / Slider
Checkbox / Switch / RadioGroup
Dialog / AlertDialog / Popover / Tooltip / Sheet
Tabs / Accordion / ScrollArea / Separator
Menu / Command / Toast / Progress / Skeleton
Table / VirtualList / ResizablePanel
```

业务组件不能复制基础组件内部 focus、disabled或error逻辑。所有 form control支持 label、description、error、required、pending和read-only。

Switch只用于立即可逆的 boolean preference；发布版本、审批、删除、Memory apply不能伪装成 switch。

## Role Play 组件

### StoryCard

Props只接收 StoryCardViewModel：标题、封面/角色摘要、最后可见消息、active branch摘要、状态和可用 action。组件不查询 run或拼 branch历史。

### MessageBubble

变体：user、assistant、streaming、saved_partial、system_action。Assistant variant展示 Candidate位置和 provenance入口；streaming不能伪造正式 message ID。

### CandidateNavigator

包含 previous/next、当前位置、regenerate和branch action。内部不改变 selection；发出带 expected head的 intent，由feature command处理。

### RoleProfileCard

用于 character/persona/world模板摘要，区分 published、draft、imported和incompatible。Avatar失败显示安全 fallback，不加载任意远程 URL。

### ContextSourceCard

展示来源、启用、顺序、预算、overflow和preview action。用户模式显示自然语言，expert variant显示 exact type/binding/path，但共享同一 normalized view model。

### InlineActionCard

变体：approval、human_response、unlock、retry、conflict、unknown_effect。每种只接受服务端允许的 action集合；不能用通用 `{title,buttons}` 让客户端自行构造高风险命令。

## Expert 组件

### GraphNode

Node renderer只接收 node identity、port、selection和run overlay。配置编辑在Inspector；renderer不直接修改draft。状态边框和节点类型标识分离，避免 running颜色覆盖类型。

### RunStatusBadge

使用 closed mapping展示 durable状态；未知值显示 incompatible并停止关键投影，不能落到灰色 `unknown`继续操作。

### EventTimeline

虚拟化，按 durableSeq排列；group/collapse只改变展示，不改变顺序。Ephemeral item视觉分离且无durable ID。

### DiffViewer

支持 JSON Pointer、before/after、provenance和conflict selection。大对象使用摘要/ref，按需授权加载；不在浏览器复制完整历史树。

### SensitiveValue

统一处理 prompt、memory、tool argument、raw response和sensitive artifact。默认摘要/遮罩，Reveal需要permission和短时确认；组件不接收 SecretValue。

## 表单模式

### Draft Form

本地允许暂时不完整；显示 dirty、base token、client diagnostics。Save draft与Publish/Apply是不同 action，不能共用模糊的“保存”。

### Versioned Form

加载时记录 base revision/head；提交携带 expected token。Conflict展示 current/yours和可重放字段，默认不覆盖。成功后用server normalized view替换local draft。

### Sensitive Form

字段不进入普通 form store、URL、analytics或persist cache；modal关闭、提交和失焦按策略清零。Password/API key不提供默认 reveal，粘贴后不回显完整值。

### Destructive Action

AlertDialog说明资源、影响范围、是否保留历史和能否恢复。要求 reason时使用结构化字段；不要用输入资源名作为所有危险操作的仪式性确认。

## 状态与反馈

- Skeleton只用于首次布局加载；已有数据刷新用stale indicator；
- Toast只报告短暂结果，冲突、审批和恢复不能只放Toast；
- command pending显示在发起位置并保留取消/离开规则；
- optimistic UI只用于非语义 preference。Message、selection、approval、branch和revision等待server成功；
- Error提供安全message、retryability、traceId和下一步；不展示SQL/provider raw正文；
- Empty state区分真正无数据、filter无结果、无权限和decoder不兼容。

## Motion

Motion只解释空间和状态：sheet进出、candidate切换、branch展开、panel resize。Token stream不对每个token做位移动画；Graph运行不持续脉冲整张画布。

默认 transition 120–220ms。等待时间未知不用伪进度条，可显示阶段和elapsed。Reduced motion关闭滑动、缩放、闪烁光标与自动画布定位。

## Responsive

```text
< 640px       mobile：单列、bottom nav、sheet
640–1023px    compact：list/detail、可折叠侧栏
>= 1024px     desktop：多栏、resizable panels
>= 1440px     wide：Story drawer或Run inspector可常驻
```

Breakpoint由可用布局空间决定，不用设备型号。Tauri mobile和Web responsive消费同一组件；平台差异通过 capability注入。

Graph在mobile默认切为node list + detail，画布只读预览。复杂edge编辑明确不支持，不用缩小桌面画布假装可用。

## 内容与主题

角色头像、封面和主题图均为ArtifactRef，经权限校验和安全MIME处理。Active content不作为CSS背景直接执行；动画图可遵循reduced motion。

角色accent只用于头像环、封面、当前Story标记和少量highlight。用户消息、系统警告、approval和错误保持全局语义样式，防止主题造成误读。

## 组件边界与测试

- `packages/ui`：基础组件、token和accessibility behavior；
- `packages/domain-ui`：Role Play/Runtime业务组件和view model；
- `packages/graph-view`：node/edge/canvas，不访问transport；
- feature container负责query/command，presentational component不导入api-client。

测试优先覆盖：键盘/focus、状态映射、mode variant、version conflict、candidate intent、SensitiveValue、approval action allowlist、responsive critical actions和contrast。截图测试只覆盖稳定布局/token，不为每个文案建立脆弱快照。

## 验收标准

1. User/Expert使用同一token和领域组件，无重复业务状态机。
2. 普通用户在移动端完成故事、候选、approval和设置，不需要Graph术语。
3. 专家在宽屏完成Graph、Run、Trace和Diff操作，信息密度不破坏可访问性。
4. 角色主题不能覆盖语义状态、安全遮罩和focus可见性。
5. 所有关键状态有文本/图标，Dark/Light/High Contrast均达到目标对比度。
6. 组件不从raw event、数据库entity或SecretValue猜/读取业务状态。
