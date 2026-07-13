# 庄生

庄生是一个面向 Agentic Role Play 的本地优先异步图运行时。核心以 Rust、Tokio、SQLite 实现；Web 使用 Axum + SSE，desktop/mobile shell 使用 Tauri commands 与同一组 core application service ports。

## 架构

```text
React domain UI
  ├─ Web transport    -> Axum adapter  -> core ports
  └─ Tauri transport  -> Tauri adapter -> core ports

core runtime <- SQLite storage
             <- provider / tool executors
```

Core 不依赖 Axum、Tauri 或 UI 类型。设计基线和取舍位于 [`designs/`](designs/)。

## 环境

- Rust stable
- Node.js 与 Corepack
- pnpm 9（由根 `package.json` 固定）
- Web/服务端开发无需 Tauri 系统库
- Linux Tauri 构建需要 Tauri v2 官方 prerequisites，包括 `pkg-config`、WebKitGTK、GLib、GTK 和 DBus development packages

安装前端依赖：

```bash
corepack pnpm install
```

## Web 开发

启动服务端：

```bash
DATABASE_URL='sqlite://zhuangsheng.db?mode=rwc' \
BIND_ADDR='127.0.0.1:3000' \
cargo run -p zhuangsheng-server
```

另一个终端启动 Web：

```bash
VITE_API_BASE_URL='http://127.0.0.1:3000' corepack pnpm dev
```

默认访问 Vite 输出的本地地址。若前后端跨 origin 运行，需要在部署层配置同源代理或明确的 CORS 策略。

## Tauri 本地开发

Tauri 使用应用数据目录中的 SQLite，不会内嵌 Axum server。先启动前端开发服务器：

```bash
corepack pnpm --filter @zhuangsheng/desktop dev
```

再启动本地 shell：

```bash
cargo run --manifest-path apps/desktop/src-tauri/Cargo.toml
```

本地 shell 包含 scheduler、LLM executor、Conversation projector、durable event cursor、Story/Wait/Memory/Context/Artifact 和设置 surface。

## 验证

```bash
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
corepack pnpm -r typecheck
corepack pnpm test
corepack pnpm build
corepack pnpm --filter @zhuangsheng/desktop build
```

在没有 Linux WebKitGTK development packages 的环境中，可以验证不含 WRY 的 Rust contract：

```bash
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --no-default-features
```

完整 Tauri/WRY bundle 仍必须在安装了对应平台系统依赖的机器上验收。
Linux 上可用仓库固定的 Tauri CLI 生成调试安装包：

```bash
corepack pnpm --filter @zhuangsheng/desktop exec tauri build --debug --bundles deb --ci
```
