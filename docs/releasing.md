# 编译与发布

GitHub Actions 使用原生 x64 和 ARM64 runner 构建 Tauri 应用。发布工作流有两种入口：

- 手动运行 `Release`：构建并保留 Actions artifacts，不修改 GitHub Release。
- 发布一个 GitHub Release：构建全部平台，成功后把所有文件附加到该 Release。

## 交付矩阵

| 平台 | 架构 | 交付内容 |
| --- | --- | --- |
| Windows | x64、ARM64 | 裸 `.exe`、NSIS `-setup.exe`、`.msi` |
| macOS | x64、ARM64 | 裸可执行文件、`.dmg` |
| Linux | x64、ARM64 | 裸二进制、`.deb` |
| Android | x64、ARM64 | 按 ABI 拆分且已签名的 `.apk`，不生成 AAB |

每组产物都附带 `SHA256SUMS-*.txt`。桌面产物目前没有开发者证书签名；macOS 裸二进制仅使用 ad-hoc 签名。正式对外分发前应再配置 Apple Developer ID、公证和 Windows Authenticode，以减少系统安全提示。

## Android 签名

可安装并能持续升级的 release APK 必须始终使用同一份 keystore。先在安全位置创建并备份 keystore：

```bash
keytool -genkeypair -v \
  -keystore zhuangsheng-release.jks \
  -alias zhuangsheng \
  -keyalg RSA -keysize 2048 -validity 10000
```

随后在仓库 `Settings > Secrets and variables > Actions` 配置四个 repository secrets：

- `ANDROID_KEYSTORE_BASE64`：keystore 文件的单行 Base64。
- `ANDROID_KEYSTORE_PASSWORD`：keystore 密码。
- `ANDROID_KEY_ALIAS`：key alias，例如 `zhuangsheng`。
- `ANDROID_KEY_PASSWORD`：key 密码。

Linux 可用以下命令直接写入 Base64 secret：

```bash
base64 -w 0 zhuangsheng-release.jks | gh secret set ANDROID_KEYSTORE_BASE64
```

其余三个值可分别通过 `gh secret set SECRET_NAME` 交互输入。keystore、密码和解码后的文件都不能提交到 Git；丢失 keystore 后，已安装 APK 无法通过后续版本原地升级。

## 发布步骤

1. 同步 `apps/desktop/src-tauri/tauri.conf.json` 和 Cargo package 的版本号。
2. 推送提交并确认 `CI` 通过。
3. 创建 `vX.Y.Z` tag，并以该 tag 发布 GitHub Release。
4. 等待 `Release` workflow 完成，检查 Release 页面上的文件和校验和。

GitHub 的 ARM64 hosted runners 可用于公开仓库。若未来改回私有仓库，它们会消耗私有仓库 Actions 分钟，超出账户额度后可能产生费用。
