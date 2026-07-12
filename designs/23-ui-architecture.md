# 前端与可视化架构

## 定位

前端默认是面向 Agentic Role Play 用户的故事、角色、世界和对话产品；Graph Runtime 是支撑角色持续行动、记忆、工具、分支和恢复的能力底座，不是默认首页。专家模式才直接暴露 Graph、Run、Trace、Context 和 Effect 等内部模型。

两种模式消费同一组 core query/command/event，是不同的信息投影而不是两套产品。前端不是第二个 scheduler：它可以编辑 GraphDraft、展示执行事实和提交领域命令，但不能自行判断 node readiness、retry、queue consumption、branch commit 或 tool effect 结果。

技术基线：TypeScript、pnpm、React、shadcn/ui、Radix UI、Tailwind CSS、React Flow。Web 使用 Axum HTTP/SSE；Tauri desktop/mobile 使用等价 command/event DTO。两者共用领域 UI，只替换 transport 和平台 capability。

阶段一先做 Web 垂直闭环，再接 Tauri shell；不在 Tauri 中默认内嵌 Axum server，也不为 mobile 复制一套领域组件。

## Workspace 边界

```text
apps/
  web/             Web bootstrap、router、auth/session、HTTP/SSE transport wiring
  desktop/         Tauri bootstrap、desktop/mobile config、platform permission wiring
packages/
  ui/              shadcn/Radix primitives、theme、tokens；不含 runtime 业务
  graph-view/      React Flow renderers、layout、selection、overlay；不请求 API
  api-client/      DTO decoder、commands/queries、transport、event projection
  domain-ui/       roleplay、library、settings、editor、runs、trace、memory、artifacts、secrets
```

`domain-ui` 是 Web 与 Tauri 共用的单一领域包，内部按 feature 目录拆分，不为每个页面创建 package。项目只有一个 shell 时可先放在 app 内；第二个 shell 接入前整体提取，避免提前拆成大量 workspace package。

`ui` 不依赖 api-client；`graph-view` 只依赖稳定 view model 和 `ui`；`domain-ui` 组合 graph-view/api-client。apps 只负责路由、依赖注入、错误边界和平台适配。

## 用户模式与专家模式

`user | expert` 是非敏感 UI preference，不是授权角色。默认 user；切换模式只改变导航、术语、信息密度和可编辑表单，不改变服务端 permission、正在运行的 GraphRun 或持久化配置。

```text
用户模式：故事、角色/世界资料、对话、候选、记忆、分支、常用设置
专家模式：以上全部 + Agent Graph、Run/Trace、Context、Tool/Effect、版本状态
```

用户模式表单必须编译到 canonical GraphDraft、ContextPreset、MemoryBinding、Channel/Model ref 和 policy command；专家模式直接编辑同一资源。不能建立一份“简单设置 JSON”作为第二 source of truth。专家修改超出 user-mode compatibility profile 后，用户模式显示“自定义高级配置”和只读摘要，只允许仍可无损映射的字段；不能用简单表单保存动作覆盖未知节点或 ContextItem。

模式、信息架构和映射规则见 `24-agentic-role-play-ui.md`，页面交互见 `25-ui-screen-specs.md`，视觉与组件规则见 `26-ui-design-system.md`。

## 可替换 Transport

```ts
interface RuntimeTransport {
  query<T>(request: QueryRequest<T>, signal?: AbortSignal): Promise<T>
  command<T>(request: CommandRequest<T>, signal?: AbortSignal): Promise<T>
  subscribeRun(runId: string, afterDurableSeq?: number): RunSubscription
  openArtifact(ref: ArtifactRef): Promise<ArtifactStreamHandle>
}
```

- `HttpSseTransport` 使用 JSON command/query、SSE durable cursor；WebSocket control 只是可选优化。
- `TauriTransport` 使用 serde 等价 commands 和 event channel，并保留相同 cursor 语义。
- 两种 transport 的 durable source 都是 server/core 的单一数据库 cursor drain；notifier/event callback 只作 wake hint，不能直接成为 durable frame。Ephemeral callback 使用独立 live overlay channel，不改变 durable cursor。
- 平台文件选择、secret initialize/unlock、下载目录等通过单独 `PlatformCapabilities` 注入，不进入领域组件。
- api-client 生成 idempotency key、携带 expected control epoch/head，并把 `409` 解码为 typed conflict；组件不手拼 header 或 endpoint。

所有外部 DTO 先按 `schemaVersion` 解码为 normalized client event/view，再进入 feature。未知 critical schema 停止该 run 投影并提示升级；不能把 server event 原样散落到 React 组件中处理。

## 四层前端状态

```text
server query projection     RunView、revision、wait、candidate、commit 等权威快照
durable event projection    按 run durableSeq 可重放的本地 reducer
ephemeral live overlay      token/reasoning/tool args 等可丢观察数据
local interaction state     draft、selection、panel、filter、未提交表单
```

四层使用不同 store/slice。durable reducer 是唯一 event-to-UI 映射入口；组件只订阅 `RunMonitorViewModel` 等稳定 selector。local editor state 和 run projection 不能共用一个巨大不透明对象。

客户端只持久化 graph draft、模式/布局等非敏感 UI preference、durable cursor 和可校验 projection cache。当前故事、角色、世界、prompt、tool arguments、memory content、raw response、secret form 和 ephemeral delta 默认不写 localStorage/IndexedDB/Tauri store。

## Durable Cursor 与 Live Delta

订阅流程：

1. 读取同一 `runId + principal + endpoint` 的已校验 projection/cache cursor，或加载权威 RunView/event history。
2. 从 `afterDurableSeq` 订阅；按 durableSeq 去重并拒绝倒退。Sequence 允许空洞，不能用 `seq + 1` 猜丢事件。
3. reducer 成功应用并持久化 projection 后才推进本地 cursor。
4. retention 过期、decoder 不兼容或 reducer invariant 失败时丢弃 cache，重新查询权威 projection/history。
5. 断线立即清空 ephemeral overlay，指数退避重连；terminal durable event/ref 恢复最终文本和 tool transcript。

Ephemeral delta 按 `callId/itemId + liveOrdinal` 去重，只更新 live overlay。每个 animation frame 合并文本，reasoning/tool argument 使用 50–100 ms 节流；缓冲有硬上限，压力下可以 coalesce/drop，绝不能阻塞 durable event。`llm.call.completed`、`node.completed/failed` 会原子替换并清空对应 live overlay。

## Graph Draft、Apply 与 Editor

Graph editor 是专家模式 surface，只编辑可暂时不完整的 `GraphDraft`。用户模式的角色/世界/生成设置通过 versioned mapping command更新可识别的 draft/preset。客户端即时校验 ID、悬空端口、明显 schema/edge 错误并显示 advisory diagnostics；Apply 必须调用服务端完成默认值补齐、canonical validation 和不可变 `GraphRevision` 创建。

Fresh workspace 的 empty state 通过公开 create command 建立 Graph/Channel/ContextPreset，使用 api-client 生成的 idempotency key，并以服务端返回的 ID/draft token/head 为权威；不能在本地先伪造资源后让 update 路由碰运气。新 Conversation 同样先调用 CreateConversation，使用返回的 root head 提交首个 Turn。

只有收到 revision id/content hash 才显示 applied。warning 与 error 绑定 node/port/edge/path；error 阻止 Apply，warning 需可检查但不由 UI 猜测修复。Run 创建器只能选择 applied revision；编辑 draft 不改变正在监控的 run。

React Flow 代码按职责拆分：

- node/edge renderers 只渲染 view model；
- layout 是可替换纯函数，不改 graph 语义；
- selection/editor commands 操作 draft；
- run overlay 操作只读 projection；
- edge 只表示 output-to-input，selector 编辑在 consumer port，条件编辑在 RouterNode。

Applied revision 默认只读；“从 revision 创建 draft”产生新草稿。自动布局、拖拽位置等纯展示 metadata 不应误触运行语义 revision，除非未来 graph schema 明确纳入。

## Run Monitor 与 Graph Overlay

Run Monitor 是专家模式的完整 surface；用户模式只在故事页展示生成中、等待确认、失败与可恢复操作等安全摘要。完整 Run 页固定加载该 run 的 graph revision，并展示 status、controlEpoch、cursor/connection、input/output refs 和 context branch。UI 不把 draft 或“最新 revision”套在旧 run 上。

Node overlay 展示 durable 状态、activation/attempt 计数、running/waiting/retry/failure、当前 model/tool 摘要；颜色之外必须有图标与文本。Edge overlay 只展示 durable enqueue/consume/stranded 和有界流量计数，不据此模拟 readiness 或未来调度。

Timeline 按 durableSeq 排序，trace tree 使用 causation/correlation 组织，不按 timestamp 猜父子关系。LLM model calls、tool calls、approval、effect `outcome_unknown`、artifact refs 和 retries 都可展开；raw/prompt/arguments 默认遮罩。

控制面：interrupt/resume/cancel 使用当前 expectedEpoch；human/approval wait form 从 response schema 生成，提交 delivery/idempotency key；过期 wait、epoch conflict 或 terminal run 刷新权威 view，不做 last-write-wins。`secret_store_unlocked` 只能打开专用 sensitive unlock flow，主密码绝不作为 Wait response/event；`outcome_unknown` 只呈现服务端允许的协调命令和 evidence 输入。

## Conversation、Context 与持久化领域 UI

- Conversation 视图按 active branch ancestry 取消息，不按 createdAt 拼接所有 branch。
- 每个 Turn 展示 sibling candidates 的 queued/running/ready/failed 状态；partial stream 明确标为未提交。
- Regenerate 创建新 candidate run，不重复 user message。Swipe/候选按钮提交 selection CAS；切换已有后续消息的旧 Turn 前展示“将移动 active branch”的确认和保留历史说明。
- 手势不是唯一入口：mobile swipe 同时提供可聚焦的上一候选/下一候选按钮与当前位置。
- WorkingContext 展示 commit/branch、JSON Pointer 级 before/after diff、StatePatch provenance 和 conflict，不允许直接改 projection row。
- Memory proposal inbox 展示 reason、evidence、expected head、policy/status，并通过 approve/reject/apply command；Story 中的 `memory_proposal_review` wait 在提交前展示完整 content/reason，要求逐项 approve/reject 并一次提交全部 open blockers，不提供绕过 MemoryManager 的裸 CRUD。
- Context preview 展示 role、provenance、trust、sensitivity、token/action；默认 metadata-only，显式授权后才 reveal 内容。
- Artifact UI 只持有 ArtifactRef/metadata，以流式 reader 上传下载；active content sandbox，显示 classification/retention/hash，staging 未 commit 不进入选择器。
- Secret UI 只列 SecretRef/metadata。Header 不存在时显示专用 initialize flow，成功后消费返回的当前 session；create/update/unlock/initialize 的明文不进入全局 store、日志、错误回显或 analytics。失效的 unlock receipt 要求生成新 idempotency key重试，不能复用旧 session；提交/失焦后清空字段，reveal/copy 默认关闭并要求显式确认。

## 安全呈现

prompt、memory、tool arguments/result、raw provider response、sensitive artifact 默认显示摘要/ref。Reveal 同时检查服务端 permission 和本地短时确认；遮罩不是授权替代，未授权内容根本不应下发。

不使用 `dangerouslySetInnerHTML` 渲染模型/tool/artifact 内容。Markdown 使用禁 HTML、过滤链接协议的 renderer；外链、下载和 Tauri open-shell 需确认。文件名、错误 details、trace label 和搜索高亮都按不可信文本处理。

Secret/API key 不进入 URL、query cache、React devtools-friendly state 或 crash report。错误 UI 只显示 typed code、安全 message、retryable 和 traceId；provider/SQL 原始正文不展示。

## 性能与背压

- timeline、conversation、tool trace、memory records 和 artifact list 使用虚拟化；默认折叠 debug 事件。
- 高频 reducer 批量提交，selector/memo 边界以 run/node/call 为粒度，避免每个 token 重渲整张图。
- React Flow 只接收最小 node/edge overlay diff；布局不随 token event 运行。
- 大 JSON、output 和 artifact 使用分页/ref/stream，不放入组件 props 或 query cache。
- 用户滚离尾部时暂停 auto-follow；恢复时跳到最新 durable event，不积压 DOM。

## Error、可访问性与移动布局

每个 feature 有局部 error boundary、loading/empty/stale/offline 状态；command error 不摧毁仍可查看的 durable projection。断线期间控制按钮默认禁用或明确排队策略，不假装命令已生效。

Radix focus management、键盘导航、语义表单和 ARIA label 是基线。状态不只依赖颜色；delta 不逐 token播报给 screen reader，只在句段或 terminal 时使用节流 aria-live。支持 reduced motion、高对比度和画布键盘选择。

Desktop/Web 宽屏使用 canvas + inspector + timeline 可调整分栏。窄屏/mobile 使用 list/detail、底部 sheet 和单面板路由；graph canvas 可切换为可搜索 node list，所有关键操作不依赖 hover、右键或精细拖拽。

## 测试边界

- api-client：DTO/schema decoder、durable reducer、cursor 去重、断线清除 live overlay、terminal finalization、typed conflict。
- graph-view：draft command、diagnostic mapping、node/edge overlay selector和键盘交互；不测试 React Flow 自身。
- domain component：user/expert mode projection、Role Play compatibility/partial save、wait schema form、candidate/branch warning、mask/reveal、artifact sandbox。
- transport contract：同一 fixture 跑 HTTP/SSE 与 Tauri，注入重复/丢失/乱序 wake hint 和并发 commit callback，验证两者仍只按数据库 cursor 输出严格递增 durable sequence；同时验证 ephemeral 丢失和 reconnect 语义一致。
- 少量 E2E：用户模式 Channel→角色模板→Story→candidate/settings，专家 draft apply→run monitor、断线恢复、interrupt/wait/approval、memory proposal、artifact 与 secret 脱敏。

测试使用 versioned event fixtures 和 fake transport，不复制 scheduler 算法生成“预期状态”；服务端集成 fixture 才是运行语义来源。

## 实现阶段

1. 后端 M1 后：pnpm workspace、设计 token、双模式 shell、api-client、用户模式 empty/setup flow，以及专家 GraphDraft editor。
2. M2–M3 后：durable projection、SSE reconnect、专家 run/trace/wait/control；用户模式只消费稳定的运行摘要。
3. M4 后：Role Play 生成流、Context preview、tool approval、artifact/secret 和模型设置。
4. M5 后：默认用户模式完整闭环，包括 Story、Conversation candidate、memory、branch/history；专家模式增加 WorkingContext diff 与 merge。
5. Web 垂直闭环稳定后：接入 Tauri desktop/mobile transport与平台能力，优先完善移动端用户模式，再提供受限专家观察页；不改变领域组件 API。

每阶段只消费已经稳定的 query/command/event contract。若 UI 需要从 raw event 猜 scheduler 状态，先补服务端 projection/decoder，而不是把推断逻辑塞进页面组件。
