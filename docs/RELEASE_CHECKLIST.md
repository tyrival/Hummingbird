# Hummingbird 发布检查清单

本文适用于私有源码仓库 `tyrival/Hummingbird` 向公开二进制仓库 `tyrival/Hummingbird-Releases` 发布新版本。

## 一次性准备

- [ ] 公开发布仓库 `tyrival/Hummingbird-Releases` 已创建，默认分支是 `main`。
- [ ] 私有源码仓库已配置 `RELEASES_REPO_TOKEN`，仅授予发布仓库 Contents 读写权限。
- [ ] 私有源码仓库已配置 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
- [ ] `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey` 是真实公钥，不含 placeholder。
- [ ] updater 私钥和密码有仓库外备份；私钥文件、密码、PAT 不存在于 Git、Release、Issue 或构建日志。

如需重新生成 updater 密钥，必须把输出写到仓库之外，避免 CLI 将私钥打印到终端：

```bash
npm run tauri -- signer generate --write-keys /绝对/仓库外路径/hummingbird.key
```

命令会交互询问密码，并生成私钥及对应 `.pub` 文件。只把 `.pub` 文件内容写入 Tauri 配置；私钥全文和密码分别写入 GitHub Secrets。轮换密钥会使旧安装包无法验证新更新，因此正式发布后仅在私钥泄露或明确迁移方案下轮换。

## 本地发布前验证

在仓库根目录运行：

```bash
npm ci
./scripts/check-no-secrets.sh
./scripts/check-no-secrets.sh --self-test
./scripts/check-workflows.sh
bash scripts/tests/test-release-script.sh
npm run lint
npm run typecheck
npm run test:run
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
cargo test --manifest-path src-tauri/Cargo.toml
git diff --check
```

普通本地安装包构建不需要私钥：

```bash
npm run tauri -- build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

macOS 本地额外检查：

```bash
codesign --verify --deep --strict --verbose=2 \
  src-tauri/target/release/bundle/macos/Hummingbird.app
hdiutil verify src-tauri/target/release/bundle/dmg/Hummingbird_*.dmg
```

## 合并主分支后先验证 CI

- [ ] 推送 `main`，等待 `.github/workflows/ci.yml` 的 `Quality checks` 通过。
- [ ] 下载并确认四个 CI artifacts 均非空：macOS arm64、macOS x64、Windows x64、Linux x64。
- [ ] CI 阶段只构建测试安装包，不需要 updater 私钥，也不会发布 Release。
- [ ] 若任何平台失败，修复后重新通过 CI，不要开始正式发布。

## 使用发布助手

发布助手要求当前分支为干净的 `main`，并且与 `origin/main` 完全一致。

```bash
# 先在隔离 worktree 验证，不修改、提交或推送当前工作区
./scripts/release.sh 0.1.0 --dry-run

# 正式首发；输入完整标签 v0.1.0 二次确认
./scripts/release.sh 0.1.0
```

后续版本例如：

```bash
./scripts/release.sh 0.1.1 --dry-run
./scripts/release.sh 0.1.1
```

脚本会同步所有版本文件、执行完整检查、创建 annotated tag，并通过一次原子 push 推送 `main` 和标签。若目标版本已经是 `main` 当前版本（例如首次发布 `0.1.0`），脚本不会制造空提交，只给当前提交打标签。对新版本则会创建 `release: vX.Y.Z` 版本提交。

## GitHub Actions 发布验收

`v*` 标签触发 `.github/workflows/release.yml`。等待 Validate、四平台 Build 和 Publish 全部成功，然后在 `tyrival/Hummingbird-Releases` 检查：

- [ ] `Hummingbird_<version>_macos_aarch64.dmg`
- [ ] `Hummingbird_<version>_macos_x86_64.dmg`
- [ ] macOS arm64/x64 的 `.app.tar.gz` 与各自 `.sig`
- [ ] `Hummingbird_<version>_windows_x86_64-setup.exe` 与 `.sig`
- [ ] `Hummingbird_<version>_linux_x86_64.deb`
- [ ] `Hummingbird_<version>_linux_x86_64.AppImage` 与 `.sig`
- [ ] `latest.json`
- [ ] `SHA256SUMS`

下载公开资产后验证：

```bash
shasum -a 256 -c SHA256SUMS
```

同时检查 `latest.json`：

- [ ] `version` 等于标签去掉 `v` 后的版本。
- [ ] `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`linux-x86_64` 四个平台齐全。
- [ ] URL 全部指向同一标签下的公开 `Hummingbird-Releases` 资产。
- [ ] 每个平台签名非空，且没有私钥、密码或 PAT。

## 更新路径验收

- [ ] 安装上一正式版本，打开“检查更新”。
- [ ] 确认显示的新版本、说明和发布时间与 `latest.json` 一致。
- [ ] macOS、Windows 或 AppImage 至少选择一个平台完成下载、签名校验、安装和重启。
- [ ] 更新后“关于/版本信息”显示目标版本，再次检查时提示已是最新版本。
- [ ] Linux DEB 按界面提示打开公开发布页并手动安装，不宣称支持应用内替换。

## 失败处理

- 正式 Release 已公开后不要覆盖同名资产，也不要复用相同版本号；修复后提升 patch 版本重新发布。
- workflow 在 Publish 前失败时，先修复源码或 workflow，再使用新的版本号重新打标签。
- 若怀疑 updater 私钥泄露，停止发布，删除泄露材料，轮换密钥与 GitHub Secrets，并评估旧客户端迁移；不要把旧私钥或密码粘贴到 Issue/日志中。
- macOS 未使用 Apple Developer ID 或公证，首次安装出现 Gatekeeper 提示属于预期；Windows 未签名安装包出现 SmartScreen 提示也属于预期。
