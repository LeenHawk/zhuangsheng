# Secret Store 设计

## 定位

阶段一按单用户、本地优先设计。

多用户、多租户权限、KMS 和团队级 secret 管理都不进入阶段一。

Secret Store 负责保存 API key、token 等敏感值。Graph、memory patch、event log 和 LLMNode 都不能保存 secret 明文。

## 基本模型

```text
master password
  -> KDF 派生 encryption key
  -> 解密本地 secret store
  -> 根据 apiKeyRef 取出短生命周期 secret value
```

主密码不存储。

主密码只在解锁时使用，不能写入数据库、日志、event、state 或 graph definition。

## SecretRef

Channel 只保存引用：

```ts
type ApiKeyRef = string
```

示例：

```json
{
  "apiKeyRef": "secret:gproxy-main"
}
```

`apiKeyRef` 只是引用，不包含 secret 明文，也不表达 auth kind。

认证 shape 仍由 `operationKey` 和对应 adapter 决定。

## 存储记录

本地 secret store 可以使用 SQLite 表或独立加密文件。

建议逻辑结构：

```ts
type SecretRecord = {
  id: string
  name?: string
  encryptedValue: string
  nonce: string
  salt: string
  kdf: "argon2id"
  createdAt: string
  updatedAt: string
}
```

阶段一不需要 user id、tenant id、sharing policy 或 team scope。

## 加密

不要直接用主密码加密 secret。

应该使用：

```text
master password + salt -> Argon2id -> encryption key
```

加密算法建议：

```text
XChaCha20-Poly1305
```

如果实现环境更容易获得 AES-GCM，也可以用 AES-256-GCM。

## 解锁与缓存

应用启动后 secret store 默认 locked。

用户输入 master password 后：

```text
derive key
verify decrypt
mark unlocked in current process
```

可以在内存中短暂缓存派生出的 encryption key，避免每次调用都要求输入主密码。

锁定应用、超时或进程退出时清空内存缓存。

## Resolver 边界

Core runtime 不直接读取 secret store。

推荐边界：

```text
LLMNode -> Channel apiKeyRef -> SecretResolver -> secret value
```

`SecretResolver` 属于 adapter/storage 边界。

Core runtime 只知道有 `apiKeyRef`，不关心它来自主密码、本机 keychain 还是未来 KMS。

## 禁止事项

- secret 明文不能进入 graph definition
- secret 明文不能进入 event log
- secret 明文不能进入 memory patch
- secret 明文不能进入 node output
- secret 明文不能作为 LLM context
- master password 不能持久化

## 后续扩展

暂缓：

- 多用户 secret
- tenant scope
- team sharing
- KMS unwrap
- OAuth token refresh
- secret rotation policy
- per-tool secret permission policy

这些能力会引入权限模型和审计模型，等单用户 runtime 稳定后再设计。
