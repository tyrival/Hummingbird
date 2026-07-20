# Hummingbird

Hummingbird 是一个使用 Tauri 2、Rust、React 和 Mantine 重写的桌面工具。当前版本提供 AWT 模板生成，并为“透传命令识别”保留工作区。应用本身不依赖 Python。

## 主要功能

- 从设备说明书提取文本，调用 OpenAI 兼容接口识别寄存器定义。
- 清洗、合并并排序 AI 返回的寄存器记录，生成固定 12 列 AWT CSV。
- 支持命名表校验、回路名称规范化、冲突处理、任务进度显示和任务取消。
- 启动时检查新版本，也可以在界面中手动检查、下载并安装更新。

## 输入与输出

支持的输入格式是 `.pdf`、`.docx`、`.xls`、`.xlsx` 和 `.csv`，单个文件最大 50 MiB。旧式二进制 `.doc` 不支持，请先用 Word 或 LibreOffice 另存为 `.docx`。扫描版或纯图片 PDF 没有可提取文本时，需要先执行 OCR。

说明书没有总字符数限制。程序按结构自适应分块，默认每块最多 12000 个字符，并携带最多 1500 个字符的上文；分块上限可在设置中调整为 8000–60000。若服务端仍报告上下文过大，程序会继续拆分当前块，而不是截断整份说明书。

生成文件保存在设置的输出目录中，文件名带时间戳与随机后缀。CSV 固定包含 12 列，并以带 BOM 的 UTF-8 编码写入，便于 Excel 正确识别中文。

## 首次使用与配置

打开“设置”，填写 OpenAI 兼容服务的 Base URL、API Key 和模型名称，再按需要调整超时、最大输出 token、输出目录及分块大小。API Key 只保存在本机应用配置目录的 `settings.json` 中，不会写入仓库、日志或更新包；请勿在截图、Issue 或构建日志中公开它。

首次启动新版时，如果还没有 `settings.json`，程序会从旧版的已知应用配置位置读取一次 `config.txt`，迁移可识别字段并写入新版配置。旧文件不会被修改；之后以新版配置为准。

## 安装说明

### macOS

发布包使用 ad-hoc 签名，没有 Apple Developer ID，也不经过 Apple 公证。第一次打开时，macOS 可能提示无法验证开发者：

1. 将 Hummingbird 拖入“应用程序”。
2. 在 Finder 中按住 Control 点击 Hummingbird，选择“打开”，再确认“打开”。
3. 如果仍被阻止，前往“系统设置 → 隐私与安全性”，在对应提示处选择“仍要打开”。

请只从项目的公开 Releases 仓库下载，并在需要时核对发布页提供的校验和。

### Windows

Windows x64 安装包是未购买代码签名证书的 NSIS EXE。SmartScreen 可能显示“Windows 已保护你的电脑”；确认文件来自 Hummingbird 官方发布页后，可选择“更多信息 → 仍要运行”。

### Linux

- Debian/Ubuntu 可安装 x64 `.deb`；建议使用 `sudo apt install ./Hummingbird_*.deb`，让 APT 一并处理 WebKitGTK、GTK 3 等运行依赖（常见包名包括 `libwebkit2gtk-4.1-0`、`libgtk-3-0`，具体名称随发行版而异）。应用内能够检查更新，但 DEB 更新需前往发布页下载新版并手动安装。
- AppImage 支持应用内更新。首次运行前执行 `chmod +x Hummingbird_*.AppImage`，然后双击或从终端启动。不同发行版可能还需要安装 FUSE/WebKitGTK 运行库。

## 在线更新与双仓库发布

源码仓库可以保持私有。安装包和更新清单发布到公开的 [`tyrival/Hummingbird-Releases`](https://github.com/tyrival/Hummingbird-Releases) 仓库；客户端只访问该公开仓库的 `latest.json` 和 Release 文件，因此检查更新不要求源码开源。

更新包由 Tauri updater 使用 Ed25519 私钥签名，客户端用内置公钥验证。签名私钥及其密码、跨仓库发布令牌必须仅配置为私有源码仓库的 GitHub Actions Secrets（名称分别为 `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 和 `RELEASES_REPO_TOKEN`），不得写入代码或 Release。macOS、Windows 和 AppImage 可在应用内下载并安装；Linux DEB 会打开公开发布页，由用户手动更新。

## 本地开发

需要 Node.js、Rust stable，以及当前平台的 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
npm install
npm run test:run
npm run typecheck
npm run lint
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run tauri -- dev
```

构建当前平台的安装包：

```bash
# 普通本地构建不需要 updater 私钥
npm run tauri -- build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

如需在本机验证正式 updater artifacts，先通过环境变量提供 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，再执行 `npm run tauri -- build`。不要把私钥写进命令历史、`.env` 或仓库。正式发布前还需将 updater 公钥写入 `src-tauri/tauri.conf.json`，并通过 GitHub Secrets 注入私钥。

## 发布新版本

私有源码仓库需要在 **Settings → Secrets and variables → Actions** 配置：

- `RELEASES_REPO_TOKEN`：Fine-grained PAT，仅授予公开仓库 `tyrival/Hummingbird-Releases` 的 Contents 读写权限。
- `TAURI_SIGNING_PRIVATE_KEY`：Tauri signer 生成的 Ed25519 私钥全文。
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：生成私钥时使用的密码。

生成密钥时务必用 `--write-keys` 写入仓库外的绝对路径，避免私钥被打印到终端日志：

```bash
npm run tauri -- signer generate --write-keys /绝对/仓库外路径/hummingbird.key
```

同时把生成的 `.pub` 公钥写入 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。私钥请在密码管理器或离线介质中另存一份；GitHub Secrets 不能读取回原值。

首次配置后，先在 GitHub Actions 手动运行 **Release**。手动运行只构建并校验临时 artifacts，不会创建公开 Release。确认四个平台产物无误后，用发布助手创建正式版本：

```bash
# 只检查，不改文件、不提交、不推送
./scripts/release.sh 0.2.0 --dry-run

# 同步全部版本文件、运行检查，确认后提交、打标签并原子推送
./scripts/release.sh 0.2.0
```

脚本要求当前分支是干净且与 `origin/main` 完全一致的 `main`。它会同步 `package.json`、`package-lock.json`、`Cargo.toml`、`Cargo.lock` 和 `tauri.conf.json`，运行前端与 Rust 全套检查，最后要求输入完整标签确认。`main` 与 `v0.2.0` 标签会在一次原子 push 中送到源码仓库。

`v*` 标签触发 `.github/workflows/release.yml`，分别生成 macOS arm64 DMG、macOS x64 DMG、Windows x64 NSIS EXE、Linux x64 DEB 和 AppImage，同时生成更新签名、`latest.json` 与 `SHA256SUMS`，再发布到公开的 `Hummingbird-Releases`。源码仓库不会创建安装包 Release。

发布完成后至少执行以下验收：

1. 在公开 Release 页面确认 5 个安装包、对应 updater 包/签名、`latest.json` 和 `SHA256SUMS` 均存在。
2. 下载当前平台安装包并核对 SHA-256。
3. 安装上一版本，执行“检查更新”，确认能够发现并安装新版本；DEB 按页面提示手动更新。
4. 如果工作流失败，不要复用已公开的版本号；修复后提升 patch 版本重新发布。
