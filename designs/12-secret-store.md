# Secret Store 设计

## 阶段一范围

阶段一是单用户、本地优先的静态 credential store，保存 LLM channel 使用的 API key/token。

进入阶段一：

- 主密码解锁；
- 本地加密持久化；
- provider client just-in-time auth 注入；
- 自动锁定、显式锁定和安全错误；
- secret metadata 的最小审计。

不进入阶段一：多用户/tenant、团队共享、KMS、OAuth refresh、远程同步、per-tool secret grant。因为没有 per-tool grant，阶段一 custom tool 一律不能访问 Secret Store。

Graph definition、channel、memory、state、artifact、event、trace、LLM IR 和 Context Assembly 都只能保存 `SecretRef`，不能保存 secret 明文。

## Threat Model

阶段一保护目标：攻击者获得应用数据库、secret 文件、WAL、普通日志或备份后，不能在不知道主密码的情况下恢复 secret。

阶段一不承诺防御：

- 已控制的运行中进程、调试器或恶意内核；
- 主密码被键盘记录、屏幕录制或用户主动泄漏；
- secret 已发送给目标 provider 后的 provider 侧泄漏；
- 未加固操作系统上的 swap、core dump 或休眠镜像取证。

实现仍应缩短明文生命周期、关闭应用 core dump（平台允许时）、避免复制，并在锁定时清零内存 key。主密码强度不足应提示，但不能记录主密码或派生物。

## SecretRef 与明文类型

```ts
type SecretRef = {
  scheme: "secret"
  id: string
}

type ApiKeyRef = SecretRef
```

序列化展示可以使用 `secret:<id>`，解析时必须校验 scheme、长度和字符集，不能把 ref 当文件路径、URL 或数据库表达式。

Rust 明文类型应采用专用包装：

```rust
pub struct SecretValue(Zeroizing<Vec<u8>>);
```

`SecretValue` 不实现 `Serialize`、`Deserialize`、`Debug`、`Display` 或无条件 `Clone`。只提供受控 byte/header 注入接口，并在 drop 时清零。任何包含 `SecretValue` 的请求 builder 也不得派生 `Debug`。

`SecretRef` 不是 secret，可以进入配置和安全日志；secret 的 name/description 仍按 private metadata 处理。

## Store Header 与 Key Hierarchy

主密码不直接加密每条记录。阶段一使用两层 key：

```text
master password + store salt
  -> Argon2id
  -> key-encryption key (KEK)

random store data key
  -> 加密每条 SecretRecord

KEK
  -> AEAD wrap store data key
```

```ts
type SecretStoreHeader = {
  magic: "zhuangsheng-secret-store"
  formatVersion: 1
  storeId: string
  kdf: {
    algorithm: "argon2id"
    version: number
    salt: string
    memoryKiB: number
    iterations: number
    parallelism: number
  }
  keyWrap: {
    algorithm: "xchacha20-poly1305"
    nonce: string
    wrappedDataKey: string
  }
  activeKeyVersion: number
  createdAt: string
  updatedAt: string
}
```

salt、nonce、ciphertext 使用明确的 base64url 编码。KDF 参数随 store 保存；创建时按当前设备校准，目标约 250–750 ms，并设置实现定义的最低内存/迭代下限，不能接受文件中恶意降低到不安全值或提高到资源耗尽的值。

解锁时 Argon2id 派生 KEK，并用 header AAD 解包随机 store data key。AEAD tag 验证失败统一返回 `UnlockFailed`，不区分错误密码与 header 被篡改。KEK 解包后立即清零。

修改主密码时只需用新 KDF/KEK 原子重包 store data key；不逐条暴露或重写 secret。header 与 records 必须作为一个可恢复事务/文件替换提交。

## Store 初始化

`InitializeSecretStoreCommand` 只在 header 不存在时成功。Application service 预生成 store ID、随机 data key、KDF salt/参数、wrap nonce 和随机 unlock session，再以“header 仍不存在”为 CAS 条件，在一个 storage transaction 中写 header、`store_created` audit、data-key-derived secret command receipt 和非敏感结果 `{ storeId, formatVersion, sessionId }`。任一写入失败都不能留下半个 store；已经存在 header 时返回 `already_initialized`，绝不能覆盖或重新生成 data key。

事务提交后、响应返回前，当前进程安装预生成的 unlocked session；因此正常初始化后无需再次输入主密码。若进程恰在 commit 后、安装或返回前退出，持久化 store 仍完整但启动后为 locked，调用方以新的 idempotency key 执行普通 unlock。初始化重放遵循下文 session-bound receipt 规则，不能借重放创建第二个 session。主密码、KEK、data key 和普通 request digest 均不落盘。

## AEAD Record

```ts
type SecretRecord = {
  id: string
  name?: string
  kind: "api_key" | "token"
  keyVersion: number
  algorithm: "xchacha20-poly1305"
  nonce: string
  ciphertext: string
  createdAt: string
  updatedAt: string
}
```

每次 create/update 生成新的 192-bit random nonce，绝不能在同一 data key 下复用。`ciphertext` 包含 authentication tag。

Record AAD 至少包含：

```text
formatVersion + storeId + record id + kind + keyVersion
```

这样攻击者不能在不同 store/record 之间替换 ciphertext。metadata 更新若进入 AAD，必须与 ciphertext 同事务重新加密。

Wrapped store data key 的 AAD 使用 canonical 编码的 `magic + formatVersion + storeId + KDF algorithm/version/salt/params + wrap algorithm + activeKeyVersion`。修改主密码会生成新 KDF salt/参数、nonce 和 wrapped ciphertext，并原子替换 header。

阶段一只有一个 store data key，`activeKeyVersion` 和所有 record `keyVersion` 恒为 1；rotation/keyring 延后。字段保留是为了格式演进，读取到其他 version 返回 `unsupported_format`，不能猜测旧 key。

Provider opaque continuation 等 internal-sensitive runtime object 使用 HKDF-SHA256 从 store data key 派生 purpose/object-bound subkey，再以 XChaCha20-Poly1305 加密。阶段一每个 provider EffectAttempt 只预留一个 object，purpose 固定为 `provider_opaque_bundle_v1`；salt=`storeId` bytes，info=`zhuangsheng/internal-sensitive/v1\0 + effectAttemptId + \0 + reservedObjectId + \0 + purpose`。Format version=1、KDF version=1、store/effect attempt/object ID/purpose/key version/algorithm 进入 AAD。它不进入 SecretRecord 列表或普通 Artifact/Object dedup，但解密同样要求当前 unlock session。

明文 container 是 canonical `InternalSensitiveBundleV1`：包含 schemaVersion/effectAttemptId/modelCallId 和有界 `entryKey -> {adapterKey, operationKey, semanticSlot, opaqueBytes}` map。Adapter 按 normalized response 顺序为 top-level continuation 与每个 hosted/reasoning item 分配唯一 entryKey；entryKey/semanticSlot 位于密文内并受 AEAD 认证，外部 ref 不能用任意 selector 读取其他 entry。整个 bundle 只加密落盘一次，一把 lease 不需要在 lock 后再派生第二把 object key。

因此即使某个本地/credential-free channel 不需要 API key，只要它会产生必须恢复的 opaque continuation，也必须先初始化并解锁 Secret Store；不能退化为明文持久化。

阶段一 canonical AEAD 是 XChaCha20-Poly1305，不按运行平台静默改成另一算法。未来算法迁移通过 `formatVersion` / `algorithm` 显式执行。

## 存储与文件边界

Secret Store 可以使用独立文件或 SQLite 表，但 header、record、nonce 和 ciphertext 之外不得写入 key material。

- 文件权限按平台收紧为当前用户可读写；创建时禁止宽权限窗口。
- SQLite WAL、journal、临时表和备份只能出现 ciphertext。
- 备份必须同时包含匹配的 header 与 records；不完整恢复返回 `CorruptStore`。
- 删除记录后，普通数据库不能保证物理擦除；阶段一承诺逻辑删除和后续 vacuum/compact，不承诺闪存安全擦除。
- UI 不默认把 secret 放入剪贴板；显式 reveal/export 需要重新确认并不进入 event log。

## Unlock Session

应用启动后 store 默认 locked。成功解锁后只在当前进程保存 `Zeroizing<StoreDataKey>`、随机 128-bit session ID 和本地 generation，不缓存主密码。

锁定条件：

- 用户显式锁定；
- 配置的 idle timeout；
- OS/app lock signal（平台支持时）；
- 进程退出。

锁定会使 session ID 失效并提高本地 generation；data key 的清零与 in-flight encryption lease 规则见下节。已经交给 OS HTTP stack 的请求无法撤回；后续 model call 必须重新解锁，旧 resolver/auth lease 不得跨 session 使用。

并发 unlock 尝试需要本地速率限制。错误信息不能泄漏 record 是否存在、密码接近程度或 KDF 中间状态。

Initialize/unlock 还使用每 store（header 尚不存在时为 singleton）的进程内线性化 mutex，覆盖“验证输入/决定 receipt → 注册不可用的 `installing` session → 提交 header/receipt → 切换为 `active` → 发送 system delivery/返回响应”。Resolver 只接受 active，不能看见 installing。相同 scope/idempotency key 的并发请求等待前一请求离开临界区后，仍用自己的 secret bytes 重算 HMAC再决定返回、冲突或失败，不能盲目共享 leader 结果；因此同 key 不同密码不会被误当成功。Commit 失败会移除 installing；commit 后若进程退出，registry 消失且下次同 key按 expired 处理。Mutex 必须持有到 active/failed 的最终内存状态，消除 receipt 已提交但并发重放误判 session 失效的窗口。

## Secret-bearing Command Idempotency

Secret Store 初始化、secret create/update、主密码变更和 unlock 不使用普通 SHA-256 request digest；否则数据库/WAL 会成为 API key 或低熵主密码的离线 verifier。成功初始化/解锁/写入后，使用 store data key 经 HKDF-SHA256 派生专用 `SecretReceiptKey`：salt=`storeId`，info=`zhuangsheng/secret-command-receipt/v1`；该 key 仅用于 HMAC-SHA256，不用于加密/解密，用完立即清零。

Receipt 只保存 HMAC，input 是长度前缀的 canonical bytes：`domain/version + scope + idempotencyKey + commandKind + canonical non-secret fields + exact secret bytes`；所有字段有显式长度，不用字符串拼接。Secret write 与 receipt HMAC/result metadata 在同一数据库事务落盘。Initialize/unlock 只在 wrap/unwrap 与 AEAD 验证成功、data key 已在内存时计算 receipt；失败/locked 尝试不持久化任何与密码有关的 digest/HMAC，只走不含输入的 rate-limit/audit metadata。

重放成功命令时必须先在上述线性化边界重算 HMAC；匹配才考虑返回已存非敏感 result，不匹配返回 `idempotency_conflict`。Initialize/unlock receipt 额外绑定首次成功时的随机 `sessionId + processGeneration`：只有该 session 仍在当前进程 registry 中 active 且未超时，同 key 重放才返回原 session ID；lock、idle expiry 或进程退出后，同 key 即返回 `idempotency_key_expired`，不得安装新 session、解决 wait 或返回失效 ID，调用方必须生成新 key 再 unlock。Lock 会在同一进程 best-effort 标记相关 receipt expired；崩溃无需回写，缺少匹配的内存 active session 本身就是权威失效判据。

Secret write/password-change 的非 session result 仍按普通安全 retention 返回；其 receipt 不能被 unlock expiry 连带失效。攻击者只获得 DB/HMAC 而没有 data key 时不能离线验证候选 secret；若 data key 已泄露，Secret Store 机密性本身已不在威胁模型内。不带 secret bytes 的 lock/纯 metadata command 仍可使用普通 application receipt。

## In-flight Sensitive Write Lease

Provider call 可能在发送后返回多个必须加密持久化的 opaque entries。发送前先持久化 EffectAttempt并预留其唯一随机 bundle object ID，再在 unlocked session 内派生仅绑定该最终 `(effectAttemptId, reservedObjectId, provider_opaque_bundle_v1)` 的短期 `SensitiveWriteLease`：

```text
store data key
  -> HKDF-SHA256(固定 salt/info，包含 effectAttemptId/objectId/purpose)
  -> Zeroizing per-effect encryption key
```

Lease 内存中持有上述最终 object 的 `Zeroizing` AEAD key，只能加密并落盘一个该 effect 的 bundle，不是可继续派生任意 object key 的 seed。它不能解密 SecretRecord、其他 object 或发起新认证；它不持久化，deadline 不超过 effect deadline加短暂 finalization grace。

显式/自动 lock 立即禁止新 resolver/auth/write lease、使 session ID 失效并清零 store data key，但已经 `started` 的 effect 可以保留自己的 purpose-bound encryption key直到 response 被原子加密落盘、effect terminal 或 lease deadline，随后清零。因此 UI 可以显示 locked，而不会让 send→response 之间的结果只能明文落盘。

Lock/cancel 后返回的 response 仍先用有效 lease加密保存，再按 run epoch/fencing 决定是否只作隔离 audit，不能推进旧 run。进程退出或 lease 到期导致无法安全保存 continuation时，effect 进入 reconcile/`outcome_unknown`；non-idempotent hosted/tool call 不能当普通 transport failure重发。

## Provider-client Resolver 边界

Core runtime 和 `LLMNode` executor 不解析 secret：

```text
core request plan
  { operationKey, channel, model, credential }
    -> ProviderClient boundary
      -> secret: SecretResolver.resolve(apiKeyRef)
         -> shape-specific CredentialInjector
      -> none: inject nothing
      -> HTTP request
      -> immediately drop SecretValue
```

`SecretResolver` 与 credential injector 位于 adapter/provider-client 边界。它们返回/消费短生命周期 `SecretValue`，不能把明文回传给 core、shape-neutral IR 或 runtime event。

认证位置由当前标准 `OperationKey` adapter 决定，例如 bearer header、`x-api-key` 或 `x-goog-api-key`。Channel 不保存 auth kind，也不能用 `extraHeaders` 覆盖认证字段。

如果某个标准 shape 只能把 credential 放在 query 中，client 必须在内存中最后拼接、禁用最终 URL tracing，并确保 redirect 不把 credential 转发到不同 origin。阶段一优先使用标准 header 形式。

## URL、Header 与网络限制

Channel 创建/更新时必须验证：

- `baseUrl` 不含 userinfo；
- 不含名为 key、token、secret、signature、credential 等敏感 query；
- 默认使用 HTTPS；仅显式开发模式允许 loopback HTTP；
- redirect 默认关闭，或只允许同 origin 且重新执行 auth policy；
- credential injector 只向已验证的最终 origin 注入。

Provider extension 只能加入 allowlist 的非敏感 header。Authorization、Cookie、Proxy-Authorization、各类 API key/token header 和 transport framing header 必须拒绝，规则与 `07-llm-ir.md` 保持一致。

## Locked 与错误语义

```ts
type SecretStoreError =
  | { type: "not_initialized" }
  | { type: "already_initialized" }
  | { type: "locked" }
  | { type: "not_found"; secretRef: SecretRef }
  | { type: "unlock_failed" }
  | { type: "idempotency_key_expired" }
  | { type: "corrupt_store" }
  | { type: "unsupported_format" }
```

GraphRun 内需要 credential 时若 store locked，runtime 返回可持久化 `waiting(secret_store_unlocked)`，而不是伪装成 provider auth failure。解锁后恢复同一 NodeInstance，并从未发送的 model call 继续。Run 外 preview/model discovery 返回 typed `locked` application error并在解锁后幂等重试，因为它们没有合法 WaitRecord owner。

成功 initialize/unlock 建立不可复用的随机 session ID，并只在 receipt 已提交且 session 已切为 active 后，由 application service 用系统 delivery `unlock:<sessionId>` 幂等解决当前 principal 的 open waits；持久化 response 只有该非敏感 ID。主密码/data key 不进入 WaitRecord。若调度前 store 再次锁定或进程退出，provider client 检测 session 失效并让 executor再次等待。

`not_found` 是配置错误，默认 failed；provider 已返回的 401/403 是远端 auth error，不能自动触发解锁。进入等待会结束当前 NodeAttempt；resume attempt 获得新的 execution deadline，但 GraphRun wall-clock deadline继续包含锁定等待时间。

## 日志、Event 与 Raw Capture

允许审计的操作只有：store created/unlocked/locked、secret ref created/updated/deleted/resolved、调用方 node/channel id 和结果类别。禁止记录：

- master password、KEK、data key、SecretValue；
- auth/request headers；
- 带 credential 的 URL；
- provider client 的 debug request；
- resolver lease 或解密 buffer；
- 可能回显 credential 的原始错误正文。

HTTP/tracing middleware 必须在序列化字段进入 subscriber 前脱敏，不能依赖 UI 隐藏。`LlmApiError` 只保留长度受限的安全 message/code/status/retryable；raw capture 永不包含 request header。

## Context、Tools 与未来扩展

Context Assembly、template、memory tool 和 preview 都没有 `SecretResolver`。发现 SecretValue 或已知 credential fingerprint 时必须 fail closed，而不是只在展示层遮罩。

阶段一 custom tool 即使已获 `ToolGrant` 也不能请求 `SecretRef` 或 resolver handle。Hosted tool 只使用 provider client 已注入的 channel credential，不获得其明文。

未来确需工具调用第三方 credential 时，再设计独立 `SecretGrant`：绑定 tool id/version、secret ref、用途、目标 origin、审批、审计和 rotation。不能把 resolver 作为全局 service 放进 tool context。
