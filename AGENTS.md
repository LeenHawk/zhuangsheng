# Agent 工作规范草案

## 项目定位

本项目是一个 agentic 框架，核心模型是以 LLM 调用为主要节点的异步有向图运行时。

设计重点包括：

- 异步图执行
- LLMNode 内部工具调用
- 持久化记忆
- 版本化状态
- 分支和恢复
- 流式事件
- 桌面、移动和 Web 前端可视化

## 技术选型

后端优先使用 Rust。

推荐后端技术栈：

- Rust stable
- Tokio 作为异步运行时
- Axum 作为 Web API adapter，而不是核心 runtime 依赖
- Serde 作为序列化基础
- SeaORM 作为数据库访问层
- SQLite 用于本地优先和早期开发
- PostgreSQL 用于多用户、服务端部署或更强并发需求
- tracing / tracing-subscriber 用于结构化日志和 trace

前端目标包括 desktop、mobile 和 web。

候选形态：

- Tauri：desktop + mobile，本地优先、桌面应用、移动端封装
- Axum Web adapter：Web 控制台、远程访问、多用户服务

核心 runtime 必须独立于 Tauri 和 Axum。

推荐架构：

```text
core runtime
  <- tauri adapter
  <- axum adapter
```

Tauri 和 Axum 都只是外层 adapter。它们不能反向污染 core runtime 的类型、生命周期或错误模型。

前端技术栈：

- TypeScript
- pnpm 作为前端包管理器
- React
- shadcn/ui
- Radix UI
- Tailwind CSS
- React Flow

React Flow 主要用于图编辑、图运行状态可视化、节点 trace 展示和分支状态查看。

## 架构原则

优先保持清晰边界，而不是提前做大而全的抽象。

核心边界：

- Graph definition：图结构、节点定义、边定义
- Runtime：run、scheduler、node instance、join、loop、interrupt、resume
- State：state patch、version、checkpoint、branch、merge
- Memory：memory binding、memory tools、memory manager、versioned memory
- Events：event log、streaming、trace、replay
- Adapters：HTTP、SSE、WebSocket、Tauri commands
- UI：图编辑、运行监控、事件流、memory 查看、状态 diff

不要把所有能力塞进一个全局 context 或巨型 service。

不要让 LLMNode 直接操作底层数据库或底层 memory store。

不要把工具默认建模成图节点。工具通常是 LLMNode 的 capability，只有具备独立生命周期、权限、审计或恢复需求时才提升为图节点。

## 文件大小约束

每个源文件最好控制在 200 行以内。

单个文件绝对不能超过 500 行。

当文件接近 200 行时，应该主动评估是否拆分。

拆分时优先按照职责拆分，而不是机械拆分：

- 类型定义
- runtime 逻辑
- storage 逻辑
- API handler
- UI component
- hook
- utility
- test fixture

不要为了满足行数而制造过多无意义的小文件。

## 目录结构原则

目录应该表达模块边界，而不是技术噪音。

后端建议结构方向：

```text
crates/
  core/
    graph/
    runtime/
    state/
    memory/
    events/
  adapters/
    axum/
    tauri/
  server/
    api/
    transport/
    auth/
  storage/
    sqlite/
    postgres/
```

`core` crate 不应该依赖 `axum`、`tauri`、HTTP 类型或 UI 类型。

`adapters/axum` 和 `adapters/tauri` 负责把外部协议转换成 core runtime 的输入输出。

前端建议结构方向：

```text
apps/
  desktop/
  web/
packages/
  ui/
  graph-view/
  api-client/
```

如果项目早期规模较小，可以先使用更扁平的结构。不要为了看起来专业而过早拆 workspace。

当模块边界稳定后，再逐步拆分为 workspace crate 或 package。

## Rust 代码规范

Rust 代码应优先保证清晰、可维护和错误边界明确。

建议：

- 使用 `Result<T, E>` 表达可恢复错误
- 使用 `thiserror` 定义领域错误
- 在应用入口或 API 边界使用 `anyhow` 可以接受，但核心库不要滥用 `anyhow`
- 公共类型应尽量显式、可序列化、可调试
- 运行时核心类型优先派生 `Debug`, `Clone`, `Serialize`, `Deserialize`，按需派生 `PartialEq`
- 避免过深泛型和 trait 抽象，除非已经有明确的多实现需求
- 避免提前引入宏和复杂类型体操
- 异步任务必须考虑取消、超时、重试和 trace

不建议：

- 为了抽象而抽象
- 过早设计插件系统
- 把数据库模型直接暴露为领域模型
- 在核心 runtime 中混入 UI 或传输层概念
- 使用全局可变状态隐藏依赖

## 前端代码规范

前端应使用现代 React 和 TypeScript。

建议：

- UI 基础组件使用 shadcn/ui 和 Radix UI
- 样式使用 Tailwind CSS
- 图编辑和图状态展示使用 React Flow
- 业务组件和通用 UI 组件分离
- 图节点 UI、事件流 UI、memory UI、state diff UI 分模块维护
- 组件 props 保持明确，不传递巨大不透明对象
- 网络 API 访问集中在 api client 层
- 对高频事件流做节流、合并或虚拟化

不建议：

- 大型页面组件承载所有逻辑
- 过度使用全局状态
- 把服务端 event 原样散落到所有组件中处理
- 为简单状态引入复杂状态管理库
- 盲目复制 shadcn 示例后不整理结构

React Flow 相关代码应特别注意拆分：

- node renderers
- edge renderers
- layout logic
- selection state
- run status overlay
- event-to-graph-state mapping

## 测试原则

不要过度测试。

测试应该覆盖核心行为和容易回归的边界，而不是追求覆盖率数字。

优先测试：

- graph scheduling
- router decision
- join policy
- state patch application
- branch fork 和 merge
- interrupt 和 resume
- memory patch validation
- event ordering 和 reconnect

可以少测或不测：

- 简单 getter/setter
- 纯展示组件的细枝末节
- 第三方库本身行为
- 尚未稳定的实验性 UI

测试阶段性补齐即可。早期不要为了测试架构牺牲设计速度。

## 重构策略

允许先实现，再在阶段边界重构。

适合重构的时机：

- 一个模块文件接近 200 行且职责开始混杂
- 同类逻辑出现第三次重复
- 领域概念稳定下来
- API 边界开始被多个调用方依赖
- 测试开始难写，说明边界可能不清晰
- runtime 行为已经通过基础场景验证

不应该在概念还不稳定时做大规模抽象。

## 文档规范

设计文档放在 `designs/`。

文档应优先记录：

- 为什么这样设计
- 关键边界
- 取舍
- 暂缓实现的能力
- 未来可能扩展方向

不要让文档变成代码注释的重复。

当实现偏离设计时，要么更新设计文档，要么在实现中清楚标记原因。

## 开发习惯

每次修改前先理解现有代码和文档，不要凭空假设架构。

实现时优先做最小正确改动。

新增依赖前要确认：

- 是否真的需要
- 是否有维护风险
- 是否会影响 Tauri/mobile 兼容性
- 是否能被更简单的本地代码替代

不要盲目复制模板、示例项目或大型 boilerplate。

如果复制第三方示例代码，必须整理命名、结构和边界，让它符合本项目。

## API 与传输

核心 runtime 不绑定 Tauri，也不绑定 Axum。

Web 服务 adapter 可以使用 Axum。

桌面和移动 adapter 可以使用 Tauri commands。

二者都应该调用同一个 core runtime API。

建议传输方式：

- HTTP JSON：普通 CRUD、run 创建、状态查询
- SSE：只读事件流
- WebSocket：双向控制，例如 interrupt、resume、user input
- Tauri commands：桌面和移动端本地调用

同一核心 runtime 不应该绑定到某一种传输方式、应用壳或 UI 环境。

传输层只负责协议适配，不能承载核心业务逻辑。

不要在 Tauri 应用中默认内嵌 Axum server，除非明确需要让 desktop/mobile 和 web 共用完全相同的 HTTP/SSE/WebSocket client。

设计：

```text
Tauri UI -> Tauri adapter -> core runtime
Web UI -> Axum adapter -> core runtime
```

## 数据持久化

持久化设计应支持本地优先和服务端部署。

建议：

- event log 是 runtime 恢复和 trace 的基础
- state 使用 patch 和 version
- 大对象使用 content hash 去重
- 当前状态使用 materialized view 提高读取速度
- 历史版本通过 checkpoint + delta 恢复
- memory edit 使用 proposal / patch / validation 流程

不要为每个版本复制完整状态。

## 性能与可靠性

早期不要过度优化，但核心路径要避免明显错误。

需要注意：

- LLM token event 不能无限制持久化
- 高频事件需要 compact、coalesce 或采样
- active node 需要 lease 或 idempotency key
- 外部副作用需要可恢复策略
- long-running run 需要 checkpoint
- branch 不应该复制整份 state

## 安全与权限

即使早期是本地应用，也要保留权限边界。

建议：

- LLMNode 只能访问显式授予的 tools 和 memory scopes
- memory edit 通过 memory manager 校验
- 长期记忆变更需要 reason 和 evidenceRefs
- 高风险操作需要人工确认或策略确认
- 不在日志中记录 secret
- API key 和 token 不进入 event log 或 state patch

## Agent 行为要求

Agent 在本项目中工作时应遵守：

- 先读相关文档和现有代码，再修改
- 保持改动小而明确
- 不引入无关格式化
- 不重写用户未要求重写的代码
- 不删除或覆盖用户改动
- 不盲目新增抽象
- 不盲目新增依赖
- 不把文件写到 500 行以上
- 文件接近 200 行时主动考虑拆分
- 实现后运行与改动相关的最小验证
- 如果发现设计和实现冲突，先记录或询问，不要强行扩大范围

## 当前优先级

当前阶段优先沉淀设计和最小 runtime。

推荐优先级：

```text
1. 明确 graph / node / edge 类型
2. 明确 GraphRun / NodeInstance
3. 实现基础 async scheduler
4. 实现 event log
5. 实现 state patch
6. 实现 LLMNode executor 抽象
7. 实现 RouterNode
8. 实现 memory binding
9. 实现 streaming API
10. 再考虑 Tauri/Web UI 可视化
```

UI 不应早于核心 runtime 太多，否则容易围绕假数据和临时接口堆积复杂度。
