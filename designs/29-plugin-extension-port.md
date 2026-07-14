# Git 插件与 UI 扩展端口

## 定位

插件端口首先服务 Agentic Role Play 前端扩展。用户可以安装第三方 renderer，决定故事消息如何呈现；插件不能借“兼容酒馆”之名取得 Graph Runtime、数据库、prompt、secret 或任意宿主 DOM 权限。

SillyTavern 兼容包可以作为普通插件实现，但庄生核心不会写死 SillyTavern manifest、事件名、正则阶段或全局对象。插件 API 是产品中立的，当前 API version 为 `1`。

当前纵向闭环支持：

- 从 HTTPS Git URL 和可选 ref 检查插件；
- 使用 Secret Store 中的凭据拉取私有仓库，URL 不携带 token；
- 展示 commit、tree hash、manifest hash、完整权限和新增权限；
- 用户确认后 side-by-side 激活版本；
- 启停、手动检查更新、更新策略和历史版本回滚；
- Web HTTP 与 Tauri command 使用同一 `PluginPackageService`；
- 消息正文及流式候选使用 sandbox renderer，失败回退原生 UI；
- 用户在当前设备选择原生、自动最高优先级或指定 renderer。

`ui_panel`、`ui_theme` 和 `ui_artifact_render` 已作为 manifest 权限名保留，但 API v1 尚未向插件提供对应宿主能力。声明权限不会凭空获得能力。

## 边界

```text
core application
  Plugin manifest / command / view / service traits
             ^
storage     plugin-host             UI extension host
候选和版本   Git、文件、hash、激活     sandbox、UiNode 校验、React renderer
             ^                         ^
       Axum / Tauri adapter       Web / desktop composition
```

- core 只定义领域 DTO、校验和 service trait，不依赖 Git、文件系统、Axum、Tauri 或 React。
- storage 保存候选、安装、版本、策略和 command receipt，不执行插件代码。
- Rust `plugin-host` 负责外部 Git 与 package 目录；它不是 Graph Runtime 的一部分。
- TypeScript `ui-extension-host` 是唯一执行 UI 插件代码的地方。
- 插件不能注册 Graph node、直接写 Memory、拦截 LLM tool loop 或修改 event log。未来若需要这类能力，必须设计独立、可审计的后端 capability。

## 仓库格式

插件仓库必须提交构建产物。庄生不会运行 `npm install`、`pnpm install`、build script、Git hook 或 submodule。

```text
manifest.json
dist/plugin.js
```

最小 manifest：

```json
{
  "apiVersion": 1,
  "id": "example.story-renderer",
  "name": "Story Renderer",
  "version": "1.0.0",
  "description": "Render role-play messages",
  "minimumHostVersion": null,
  "entrypoints": { "uiWorker": "dist/plugin.js" },
  "permissions": [
    "ui_message_read_display",
    "ui_message_decorate"
  ],
  "renderers": [{
    "id": "story-message",
    "slot": "conversation_message_body",
    "priority": 10,
    "roles": ["assistant"]
  }],
  "dependencies": [],
  "settingsSchema": null
}
```

ID 使用小写 ASCII 字母、数字、点、横线和下划线。entrypoint 必须是仓库内的 `.js` 或 `.mjs` 相对路径。包不能包含 symlink、特殊文件或父目录跳转。

限制：最多 2,000 个文件、包总计 10 MiB、单文件 2 MiB、UI entrypoint 1 MiB。entrypoint 必须是无需外部 import 的单文件 ESM bundle。

## Renderer API

插件只导出一个异步或同步函数：

```js
export async function render(request) {
  return [{
    type: "paragraph",
    children: [
      { type: "badge", text: request.message.role, tone: "accent" },
      { type: "text", text: ` ${request.message.text}` }
    ]
  }];
}
```

输入为 display projection，不是原始 provider response：

```ts
interface PluginRenderRequest {
  rendererId: string
  slot: "conversation_message_body"
  message: {
    id: string
    role: "user" | "assistant"
    source: string
    text: string
    reasoning: string | null
    streaming: boolean
  }
  mode: "user" | "expert"
  platform: "web" | "desktop" | "mobile"
}
```

输出是 closed union `UiNode[]`，包括 text、badge、HTTPS link、paragraph、heading、quote、code、divider 和 stack。没有 HTML、React component、CSS、DOM selector、event handler 或任意 URL resource。宿主限制输出深度 8、节点 256、文本 100,000 字符。

## 执行隔离

每个插件版本使用独立的 hidden iframe：

```text
React host
  -> postMessage
opaque-origin iframe (sandbox="allow-scripts", connect-src 'none')
  -> Dedicated Worker
plugin ESM render(request)
  -> structured clone UiNode[]
host validates -> React renders
```

iframe 没有 `allow-same-origin`、popup、top navigation 或 form 权限。CSP 禁止网络、图片、媒体、字体、frame 和 object。插件在 Dedicated Worker 中执行，因此同步死循环不会占住宿主 UI 主线程；单次 render 默认 1.5 秒，超时会终止整个 sandbox。任何 load error、render error、非法 node 或超时都返回原生消息显示。

这是执行隔离和能力最小化，不是对恶意 native code 的证明。插件仍可能消耗其 worker 的 CPU/内存；安装页必须显示来源、固定 commit、hash 与权限，用户承担信任第三方代码的决定。

## 安装与更新状态机

```text
HTTPS Git URL
  -> staging shallow fetch
  -> 去除 .git
  -> manifest / 文件类型 / 大小 / hash 校验
  -> candidate（固定 commit + tree hash）
  -> 用户确认全部权限
  -> versions/<plugin>/<version-id> side-by-side
  -> storage transaction CAS 激活
```

活动目录不执行 `git pull`。文件先移动到新的不可变 version 目录，数据库用 `expectedActiveVersionId` 做 CAS；旧版本保留，可通过 rollback command 原子切回。entrypoint 每次读取都会重新验证 package tree 与 manifest hash。

候选以 `pluginId + resolvedCommit` 复用，自动扫描不会为同一新增权限版本反复创建 staging 副本。已安装过的 commit 由历史版本识别，不重复安装。

更新策略：

- `manual`：只在用户操作时检查；
- `notify`：允许检查并形成候选，但不自动激活；
- `automatic`：后台检查；仅当 `addedPermissions` 为空时自动激活；
- 任意新增权限都必须回到安装页重新确认。

自动更新失败继续使用当前版本。激活 receipt、configure 和 rollback 都有 idempotency key；并发更新由 active version CAS 拒绝。

## 传输接口

HTTP：

```text
GET  /v1/plugins
POST /v1/plugins
POST /v1/plugins/candidates/{candidateId}/activate
POST /v1/plugins/{pluginId}/configure
POST /v1/plugins/{pluginId}/check-update
POST /v1/plugins/{pluginId}/rollback
GET  /v1/plugins/{pluginId}/entrypoint
```

Tauri 暴露等价 command，并同时进入 exact JSON dispatcher。桌面端 package root 位于 app data 的 `plugins/`；服务端默认使用 `PLUGIN_DIR=plugins`。两者都调用同一 package service，不在 Tauri 中启动 Axum。

Git transport 使用编译进 `plugin-host` 的 `gix`，HTTPS 后端为 reqwest + rustls，不启动 `git`/`git.exe`，也不要求用户安装 Git。仓库使用 `open::Options::isolated()`，不会读取 system/global Git config、credential helper、交互 prompt、template 或 hook；认证只接受 Secret Store 显式注入的账号。

拉取只接受 branch/tag 名或默认 HEAD，使用 depth 1，随后记录实际 resolved commit。API 不接受裸 40 位 object ID，避免依赖服务端是否允许 `want <sha>`；需要固定已知版本时应发布 tag。Android/iOS 因此不再依赖系统 Git，但仍必须完成各 target 的 rustls 平台证书、文件权限、包体和真机网络验证，不能仅凭内嵌实现宣称移动端已交付。

## 暂缓能力

- 插件设置 schema 表单与加密设置值；
- panel、theme、artifact renderer slot；
- 经用户授权的外部链接打开 action；
- 后端插件/WASI tool；
- 插件签名、发布者信任库和撤销列表；
- Android/iOS 真机 Git transport 与包体验证；
- 从 registry 安装及差分下载。

扩展这些能力时继续遵守：manifest 权限只声明，宿主 capability 才授予；前端插件与后端 capability 分开审批；插件数据不能成为 core runtime 的隐式全局 context。
