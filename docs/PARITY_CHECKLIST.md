# Hummingbird 功能兼容性核对

核对日期：2026-07-20

本文把旧版 `Hummingbird-old` 的可验证行为映射到当前 Tauri 2 实现。迁移目标是功能兼容，不要求复刻旧版 Flet 界面；新版界面使用 React、Mantine 和 Tauri 原生能力。

## 旧版测试映射

| 旧版测试文件 | 旧用例数 | 新版覆盖位置 | 结论 |
| --- | ---: | --- | --- |
| `test_chunked_ai_extraction.py` | 9 | `src-tauri/src/chunking.rs`、`ai.rs`、`task.rs`、`register_csv.rs` 的分块、上下文、逐块调用、合并与冲突测试 | 已覆盖，并增加 Unicode 无损拆分、上下文超限二分、取消和异常清理 |
| `test_circuit_group_inference.py` | 13 | `prompt.rs`、`chunking.rs`、`register_csv.rs` 的提示词、范围标题、回路前缀、中文数字、分组与名称清洗测试 | 已覆盖 |
| `test_config_encoding.py` | 2 | `settings.rs` 的 UTF-8、UTF-8 BOM、UTF-16、GB18030、CP1252 解码与迁移测试 | 已覆盖并扩展 |
| `test_flet_runtime.py` | 2 | Flet 客户端不再存在；由 `App.test.tsx`、`lib.rs`、`capabilities/default.json` 和 CI 的 Tauri bundle 矩阵替代 | 已按新运行时替代，不再需要 Python/Flet sidecar |
| `test_last_input_directory.py` | 6 | `settings.rs`、`commands.rs` 的迁移、选择前校验、校验通过后保存父目录、设置原子持久化测试 | 已覆盖并增加路径授权与符号链接边界 |
| `test_macos_release_config.py` | 1 | `tauri.conf.json` 的文件选择权限和 ad-hoc 签名配置、`commands.rs` capability 测试、实际 `.app` codesign 验证 | 已按 Tauri 替代并实际打包验证 |
| `test_naming_csv_source.py` | 5 | `naming.rs` 的编码、表头重排、重复释义合并、CSV 优先、Markdown 回退和 bundle 资源测试 | 已覆盖并扩展 |
| `test_platform_actions.py` | 2 | `commands.rs` 的目录限定打开测试、Tauri opener 边界、React 输出目录交互测试 | 已覆盖；不再拼接平台 shell 命令 |
| `test_pyinstaller_paths.py` | 2 | PyInstaller/PyMuPDF 二进制收集不再适用；由 Cargo 依赖、Tauri bundle、实际 `.app` 资源与 GitHub Actions artifact 检查替代 | 已按新打包技术栈替代 |
| `test_spreadsheet_extraction.py` | 6 | `extraction/spreadsheet.rs` 和 `extraction/mod.rs` 的 XLS/XLSX/CSV、隐藏 sheet、空单元格、编码、分隔符和扩展名测试 | 已覆盖并扩展日期、时间、布尔值及损坏文件测试 |
| `test_windows_release_config.py` | 13 | `tauri.conf.json`、icons、resources、`ci.yml`、`release.yml`、`check-workflows.sh`、`commands.rs` 与文档提取测试 | 已按 Tauri/NSIS 替代；Windows x64 由 GitHub Actions 实机 runner 构建 |

旧版共 61 个测试用例。新版不按文件数量一一照抄，而是把其行为契约拆到 Rust 核心、React 交互、权限边界和跨平台 workflow 中。Flet、PyInstaller 专用行为均有明确替代项，没有静默删除。

## 已确认需求映射

| 需求 | 实现 | 验证 |
| --- | --- | --- |
| Tauri 2、React、TypeScript、Mantine | `src-tauri/`、`src/`、`package.json` | React shell 测试、TypeScript、ESLint、Vite production build、Cargo check |
| AWT 模板生成 | `AwtWorkspace.tsx`、`task.rs`、`register_csv.rs`、`output.rs` | 前端任务生命周期测试和 Rust 端到端任务测试 |
| 透传命令识别本期保持空白 | `PassthroughWorkspace.tsx` | `App.test.tsx` 验证空语义工作区 |
| PDF、DOCX、XLS、XLSX、CSV | `extraction/` | 格式夹具、编码、隐藏 sheet、空行、损坏/加密文件和扫描 PDF 测试 |
| `.doc` 给出另存为 DOCX 提示 | `extraction/mod.rs` | `legacy_doc_returns_explicit_save_as_docx_guidance` |
| 单文件最大 50 MiB | `MAX_INPUT_BYTES` | 边界值 50 MiB 接受、超 1 字节拒绝 |
| 说明书总长度不截断 | `chunking.rs`、`task.rs` | 多块逐一处理、长行 Unicode 无损、上下文超限二分测试 |
| 默认 30000 字符、上下文 3000，范围 8000–60000 | `settings.rs`、`chunking.rs`、`SettingsModal.tsx` | Rust 默认/校验测试与 React 表单测试 |
| 固定 12 列 AWT CSV、UTF-8 BOM | `register_csv.rs`、`output.rs` | 固定表头、合并/排序/冲突和 BOM 输出测试 |
| 旧配置一次迁移 | `settings.rs` | 多编码、已迁移不重复、原文件不修改、原子保存测试 |
| API key 不进入错误、日志或仓库 | `error.rs`、`task.rs`、`check-no-secrets.sh` | 序列化/日志脱敏测试、scanner 自测和仓库扫描 |
| 推理模型 token 耗尽诊断 | `ai.rs`、`error.rs` | 兼容 `reasoning_content`/`reasoning`、`finish_reason`、completion/output token 统计；推理耗尽、普通 length 和普通空响应测试 |
| 默认相对输出目录首次打开 | `commands.rs` | 在 app data 内安全创建；未授权绝对路径和符号链接仍拒绝 |
| 单任务、取消与安全退出 | `task.rs`、`commands.rs`、`App.tsx`、capability | 竞争、取消、清理、退出 gate、崩溃监督与 React 生命周期测试；窗口权限精确限制为 `core:window:allow-destroy` |
| 在线检查与签名更新 | `updater.rs`、`UpdateModal.tsx`、Tauri updater 配置 | 版本判定、下载绑定、one-shot payload、安装互斥、前端交互测试；真实公钥已内置 |
| macOS arm64/x64 DMG，ad-hoc、无公证 | `tauri.conf.json`、workflows | 本机 arm64 `.app`/DMG 实际构建、codesign 与 hdiutil 验证；x64 由 Actions runner 验证 |
| Windows x64 NSIS EXE | workflow matrix | workflow 静态检查；实际产物由 Windows Actions runner 验证 |
| Linux x64 DEB 与 AppImage | workflow matrix | workflow 静态检查；实际产物由 Ubuntu Actions runner 验证 |
| 私有源码仓库、公开二进制仓库 | `release.yml`、`prepare-release-assets.sh` | PAT 仅出现在最终 publish job 的静态断言 |
| 自动发布新版本 | `scripts/release.sh` | dry-run、同版本首发、commit/tag/push 故障回滚夹具 |

## 本轮本地证据

- 前端：8 个测试文件、47 个测试通过；lint、typecheck 和 Vite production build 通过。
- Rust：144 个测试通过；`cargo fmt --check`、Clippy `-D warnings`、Cargo check 通过。
- 发布静态检查：secret scanner、自测、workflow 检查和 release helper 回滚夹具通过。
- macOS arm64：Mach-O 架构正确，`.app` 使用 ad-hoc 签名且 `codesign --verify --deep --strict` 通过。
- bundle 资源：命名 CSV 和 Markdown 与源码 SHA-256 完全一致。
- DMG：`hdiutil verify` 返回 `VALID`。
- updater：真实公钥已写入配置，签名 `.app.tar.gz` 与 `.sig` 已生成；私钥位于仓库之外。
- 运行：此前 bundle 已启动并保持稳定，系统日志无崩溃，WebKit 主窗口可见。最终审查随后补充了最小 `core:window:allow-destroy` 权限；安全退出逻辑与 capability 精确断言已通过自动化测试，当前代码重新构建后的原生窗口关闭由最终本机验收确认。

## 必须在 GitHub 完成的验证

本地 macOS 不能替代 GitHub 托管的 Intel macOS、Windows 和 Linux runner。合并并推送 `main` 后先检查 CI 四个平台 bundle；推送首个 `v*` 标签后再按 `docs/RELEASE_CHECKLIST.md` 核对公开仓库资产、校验和、`latest.json` 以及至少一次从旧版本到新版本的签名更新。
