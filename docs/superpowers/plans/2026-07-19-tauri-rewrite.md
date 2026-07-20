# Hummingbird Tauri Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在当前仓库交付一个不依赖 Python 的 Hummingbird Tauri 2 桌面应用，兼容旧版 AWT 模板生成能力，并通过私有源码仓库到公开发布仓库的 GitHub Actions 提供跨平台安装包和签名在线更新。

**Architecture:** React、TypeScript、Mantine 只负责桌面视图和任务状态；Rust 模块负责配置迁移、文件解析、命名表、AI 分块请求、CSV 规则、后台任务和更新边界。GitHub Actions 从私有 `tyrival/Hummingbird` 构建，在公开 `tyrival/Hummingbird-Releases` 发布安装包、更新包、签名、`latest.json` 和校验文件。

**Tech Stack:** Tauri 2、Rust stable、React、TypeScript、Vite、Mantine、Tabler Icons、Vitest、Testing Library、reqwest、calamine、zip、quick-xml、pdf-extract、csv、encoding_rs、tokio-util、tauri-plugin-dialog、tauri-plugin-opener、tauri-plugin-process、tauri-plugin-updater。

## Global Constraints

- 当前窗口标题和应用名统一为 `Hummingbird`，bundle identifier 为 `com.tyrival.hummingbird`。
- AWT 输入上限为 50 MB；默认单块最大 30000 字符，允许配置 8000 到 60000；跨块上下文最多 3000 字符。
- 支持 `.pdf`、`.docx`、`.xls`、`.xlsx`、`.csv`；`.doc` 只返回明确的另存为 DOCX 提示。
- CSV 固定 12 列，使用 UTF-8 BOM 保存；不能截断整份说明书。
- `透传命令识别` 本期只有空白工作区，不实现解析器。
- macOS 发布 Apple Silicon 和 Intel DMG，使用 ad-hoc 签名，不使用 Apple Developer ID，不公证。
- Windows 发布未签名 x64 NSIS EXE；Linux 发布 x64 DEB 和 AppImage。
- 源码仓库保持私有；公开发布仓库固定为 `tyrival/Hummingbird-Releases`。
- 更新私钥和 `RELEASES_REPO_TOKEN` 只存在于 GitHub Secrets，不进入源码、日志、测试或 Release。
- 未经用户明确授权，不执行本地编译、打包、Git commit 或 push。本计划中的运行命令在获得编译授权后执行；授权前只做源文件静态检查。

---

## Planned File Structure

```text
.
|- .github/workflows/ci.yml                 # main/PR 三平台测试与打包验证
|- .github/workflows/release.yml            # v* 双仓发布、签名、latest.json、校验和
|- package.json                              # React/Tauri 脚本和前端依赖
|- vite.config.ts                            # Vite 与 Vitest 配置
|- tsconfig.json
|- index.html
|- src/
|  |- main.tsx                               # React 入口和 Mantine Provider
|  |- App.tsx                                # 应用壳、导航、更新入口
|  |- theme.ts                               # Hummingbird 设计令牌
|  |- app.css                                # 窗口基础布局和拖放覆盖层
|  |- api/types.ts                           # 与 Rust DTO 对齐的 TypeScript 类型
|  |- api/tauri.ts                           # invoke/listen 适配层
|  |- components/AppSidebar.tsx
|  |- components/LogPanel.tsx
|  |- components/SettingsModal.tsx
|  |- components/UpdateModal.tsx
|  |- features/awt/AwtWorkspace.tsx          # AWT 页面组合
|  |- features/awt/useExtractionTask.ts      # 单任务状态机和事件订阅
|  |- features/passthrough/PassthroughWorkspace.tsx
|  `- test/setup.ts
|- src-tauri/
|  |- Cargo.toml
|  |- build.rs
|  |- tauri.conf.json
|  |- capabilities/default.json
|  |- icons/*                                # Tauri 各平台图标
|  |- resources/t_electric_param.csv
|  |- resources/naming-convention.md
|  |- src/main.rs                            # 桌面入口
|  |- src/lib.rs                             # 插件注册和命令注册
|  |- src/error.rs                           # AppError 和安全序列化
|  |- src/settings.rs                        # Settings、校验、持久化、旧配置迁移
|  |- src/naming.rs                          # CSV/Markdown 命名数据源
|  |- src/extraction/mod.rs                  # 格式路由和输入校验
|  |- src/extraction/pdf.rs
|  |- src/extraction/docx.rs
|  |- src/extraction/spreadsheet.rs
|  |- src/chunking.rs                        # 结构感知分块
|  |- src/register_csv.rs                    # 清洗、回路、排序、冲突
|  |- src/prompt.rs                          # 固定 system prompt
|  |- src/ai.rs                              # OpenAI 兼容请求和重试
|  |- src/output.rs                          # UTF-8 BOM 输出
|  |- src/task.rs                            # 后台任务、取消、事件 DTO
|  |- src/commands.rs                        # Tauri commands
|  `- src/updater.rs                         # 检查、下载、安装、重启边界
|- tests/fixtures/                           # 从旧 Python 测试提取的金样
|- scripts/check-no-secrets.sh               # 仓库敏感内容静态检查
`- README.md                                 # 使用、配置、构建、未签名提示
```

---

### Task 1: Scaffold the Tauri, React, and Mantine application shell

**Files:**
- Create: `package.json`, `vite.config.ts`, `tsconfig.json`, `tsconfig.node.json`, `index.html`
- Create: `src/main.tsx`, `src/App.tsx`, `src/theme.ts`, `src/app.css`, `src/test/setup.ts`
- Create: `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Test: `src/App.test.tsx`

**Interfaces:**
- Produces: a Tauri 2 application named `Hummingbird`; React entrypoint `App(): JSX.Element`; Mantine theme `hummingbirdTheme`.
- Consumes: no earlier tasks.

- [ ] **Step 1: Add the failing application-shell test**

```tsx
render(<App />);
expect(screen.getByText('AWT模板生成')).toBeInTheDocument();
expect(screen.getByText('透传命令识别')).toBeInTheDocument();
expect(screen.getByRole('heading', { name: 'AWT模板生成' })).toBeInTheDocument();
```

- [ ] **Step 2: Define package scripts and pinned major dependencies**

Use scripts `dev`, `build`, `test`, `test:run`, `lint`, `typecheck`, `tauri`; add React 19, Mantine 8, Tabler Icons, Tauri 2, Vite, Vitest, Testing Library and ESLint. Configure Vitest with `jsdom` and `src/test/setup.ts`.

- [ ] **Step 3: Configure the Tauri application**

Set `productName`, `identifier`, version `0.1.0`, window title `Hummingbird`, dimensions `860 x 620`, minimum `720 x 520`, bundled resources, `createUpdaterArtifacts: true`, CSP, and bundle targets `dmg`, `nsis`, `deb`, `appimage`.

- [ ] **Step 4: Implement the Mantine application shell**

`main.tsx` wraps `App` in `MantineProvider` and `Notifications`. `App.tsx` owns only `activeWorkspace: 'awt' | 'passthrough'` and renders a fixed sidebar plus one workspace. The passthrough workspace must render an empty semantic `<main aria-label="透传命令识别" />`.

- [ ] **Step 5: Run verification after compile authorization**

Run: `npm run test:run -- src/App.test.tsx && npm run typecheck`

Expected: the shell test passes and TypeScript reports no errors. Before authorization, inspect scripts/imports with `rg` and run `git diff --check` only.

---

### Task 2: Define shared DTOs, errors, and safe logging

**Files:**
- Create: `src/api/types.ts`, `src/api/tauri.ts`
- Create: `src-tauri/src/error.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: inline Rust tests in `src-tauri/src/error.rs`; `src/api/tauri.test.ts`

**Interfaces:**
- Produces Rust: `AppError { code: ErrorCode, message: String, detail: Option<String> }`, `redact_secrets(text, secrets) -> String`.
- Produces TypeScript: `AppErrorDto`, `TaskEvent`, `TaskStatus`, `SettingsDto`, `UpdateInfoDto`.
- All later commands return `Result<T, AppError>` and serialize `code`, `message`, `detail`.

- [ ] **Step 1: Test stable error serialization and secret redaction**

Cover API keys in URLs, bearer headers, raw configured keys, empty keys and ordinary text. Assert serialized errors never include a known secret.

- [ ] **Step 2: Implement stable error codes**

Define exact codes: `file_not_found`, `file_too_large`, `unsupported_format`, `no_extractable_text`, `parse_failed`, `invalid_settings`, `network_failed`, `authentication_failed`, `context_too_large`, `empty_ai_response`, `invalid_ai_csv`, `save_failed`, `task_active`, `no_active_task`, `cancelled`, `update_failed`, `update_blocked`.

- [ ] **Step 3: Implement the TypeScript invoke adapter**

Expose typed functions rather than raw `invoke` calls. Normalize unknown rejected values into `AppErrorDto` so UI components do not parse Rust strings.

- [ ] **Step 4: Verify after authorization**

Run: `cargo test error --manifest-path src-tauri/Cargo.toml && npm run test:run -- src/api/tauri.test.ts`

Expected: serialization and adapter tests pass.

---

### Task 3: Implement settings storage and one-time legacy migration

**Files:**
- Create: `src-tauri/src/settings.rs`
- Create: `tests/fixtures/config/utf8-config.txt`, `tests/fixtures/config/utf16-config.txt`, `tests/fixtures/config/gb18030-config.txt`
- Modify: `src-tauri/src/lib.rs`
- Test: inline tests in `src-tauri/src/settings.rs`

**Interfaces:**
- Produces: `Settings`, `SettingsStore::load_or_migrate(app: &AppHandle)`, `SettingsStore::save(settings)`, `Settings::validate()`.
- Exact defaults: base URL `http://192.168.32.20:3000/v1`, model `deepseek-chat`, timeout `600`, max tokens `16384`, output `output`, chunk max `30000`, context chars `3000`.
- Consumed by Tasks 4, 7, 8, 10 and 11.

- [ ] **Step 1: Write migration and validation tests**

Cover UTF-16/UTF-8 BOM/UTF-8/GB18030/CP1252 decoding, known-key filtering, preservation of Chinese paths, invalid numeric fields, chunk bounds, relative output resolution, existing `settings.json`, migration warnings and non-modification of the legacy file.

- [ ] **Step 2: Implement versioned settings**

Use `schemaVersion: 1` and `migrationVersion: 1`. Serialize atomically through a same-directory temporary file and rename. Set file permissions conservatively where supported. Never include `apiKey` in `Debug` output.

- [ ] **Step 3: Implement legacy candidate discovery**

macOS checks `~/Library/Application Support/Hummingbird/config.txt`; Windows/Linux check executable-adjacent and historical app-config candidates. Stop on the first readable candidate. Do not scan broad user directories.

- [ ] **Step 4: Verify after authorization**

Run: `cargo test settings --manifest-path src-tauri/Cargo.toml`

Expected: all migration fixtures and validation cases pass.

---

### Task 4: Port the electric-parameter naming source

**Files:**
- Create: `src-tauri/src/naming.rs`
- Copy: `/Users/tyrival/workspace/Hummingbird-old/t_electric_param.csv` to `src-tauri/resources/t_electric_param.csv`
- Copy: `/Users/tyrival/workspace/Hummingbird-old/naming-convention.md` to `src-tauri/resources/naming-convention.md`
- Create: `tests/fixtures/naming/*.csv`, `tests/fixtures/naming/fallback.md`
- Test: inline tests in `src-tauri/src/naming.rs`

**Interfaces:**
- Produces: `NamingCatalog { entries: Vec<NamingEntry>, names: HashSet<String>, reference: String }` and `load_naming_catalog(resources: &ResourcePaths) -> Result<NamingCatalog, AppError>`.
- Consumed by `prompt::build_system_prompt` and `register_csv::sanitize_csv`.

- [ ] **Step 1: Port old naming-source tests as fixtures**

Assert GB18030 and UTF-8 BOM decoding, reordered headers, empty rows, duplicate meanings, order preservation, valid CSV authority, invalid CSV Markdown fallback and a current-resource smoke assertion for known codes.

- [ ] **Step 2: Implement CSV and Markdown readers**

Use the standard CSV parser. Key by exact `ParamCode`, merge distinct meanings with ` / ` in first-seen order, and derive the case-insensitive validation set once.

- [ ] **Step 3: Verify source parity after authorization**

Run: `cargo test naming --manifest-path src-tauri/Cargo.toml`

Expected: all legacy-derived cases pass and resources load from both development and bundled paths.

---

### Task 5: Port CSV sanitation, circuit normalization, merge, and output rules

**Files:**
- Create: `src-tauri/src/register_csv.rs`, `src-tauri/src/output.rs`
- Create: `tests/fixtures/register_csv/*.input.csv`, `tests/fixtures/register_csv/*.expected.csv`
- Test: inline tests in both modules

**Interfaces:**
- Produces: `CSV_HEADER`, `parse_circuit_parameter_name`, `is_circuit_range_heading`, `sanitize_csv`, `merge_csv_results`, `save_csv`.
- Signatures:

```rust
pub fn sanitize_csv(input: &str, catalog: &NamingCatalog) -> Result<SanitizedCsv, AppError>;
pub fn merge_csv_results(results: &[SanitizedCsv]) -> MergedCsv;
pub fn save_csv(output_dir: &Path, original: &Path, csv: &str, now: DateTime<Local>) -> Result<PathBuf, AppError>;
```

- [ ] **Step 1: Convert all old circuit and chunk-merge assertions into fixtures**

Include concrete/range prefixes, Chinese numerals, public parameters, invalid groups, duplicate headers, Markdown fences, quoted fields, exact duplicates, address sorting, multi-register occupancy and same-address bit records.

- [ ] **Step 2: Implement circuit parsing independently of address inference**

Only explicit concrete prefixes modify group. Remove one existing `DOC_`, strip the prefix and separators, then add `DOC_` once only when the cleaned name is absent from the naming catalog. Ambiguous or empty cleaned names produce warnings and remain unchanged.

- [ ] **Step 3: Implement deterministic merge rules**

Preserve first-seen order as the conflict tie-breaker, sort retained rows by numeric `reg_add`, allow multiple bit rows at the same address, resolve single/multi-register conflicts with the old rule, and regenerate ids from 1.

- [ ] **Step 4: Implement UTF-8 BOM output**

Sanitize the stem for platform-invalid filename characters, use local timestamp plus six lowercase hex characters, create the output directory, and write `EF BB BF` followed by CSV bytes.

- [ ] **Step 5: Verify after authorization**

Run: `cargo test register_csv --manifest-path src-tauri/Cargo.toml && cargo test output --manifest-path src-tauri/Cargo.toml`

Expected: every old assertion has an equivalent passing Rust case.

---

### Task 6: Implement PDF, DOCX, XLS, XLSX, and CSV text extraction

**Files:**
- Create: `src-tauri/src/extraction/mod.rs`, `pdf.rs`, `docx.rs`, `spreadsheet.rs`
- Create: `tests/fixtures/documents/README.md` and small generated-safe fixtures
- Test: inline tests per extraction module

**Interfaces:**
- Produces:

```rust
pub enum DocumentKind { Pdf, Docx, Xls, Xlsx, Csv, LegacyDoc }
pub fn validate_input(path: &Path, metadata: &Metadata) -> Result<DocumentKind, AppError>;
pub fn extract_document(path: &Path, kind: DocumentKind) -> Result<String, AppError>;
```

- [ ] **Step 1: Add format and size validation tests**

Cover case-insensitive extensions, 50 MB boundary, missing/empty file, unsupported ODS and `.doc` guidance.

- [ ] **Step 2: Implement spreadsheet parity**

Use calamine for XLS/XLSX and the CSV reader for CSV. Emit exact headings `=== Sheet: NAME ===` and `=== CSV ===`, include hidden sheets where the parser exposes them, preserve internal empty cells as tab separators, remove trailing empty cells/rows and keep blank row separation.

- [ ] **Step 3: Implement DOCX ZIP/XML reading**

Parse `word/document.xml` with `zip` and `quick-xml`, preserving paragraph boundaries and table cell/row order. Reject encrypted/corrupt packages with `parse_failed`.

- [ ] **Step 4: Implement PDF text extraction**

Extract page text, join non-empty pages with blank lines, close resources deterministically and return `no_extractable_text` for scanned/image-only documents.

- [ ] **Step 5: Verify after authorization**

Run: `cargo test extraction --manifest-path src-tauri/Cargo.toml`

Expected: legacy spreadsheet strings match exactly; format-specific fixtures pass.

---

### Task 7: Implement structure-aware chunking and prompt construction

**Files:**
- Create: `src-tauri/src/chunking.rs`, `src-tauri/src/prompt.rs`
- Create: `tests/fixtures/chunking/*.txt`
- Test: inline tests

**Interfaces:**
- Produces:

```rust
pub struct ChunkPolicy { pub max_chars: usize, pub context_chars: usize }
pub struct DocumentChunk { pub index: usize, pub text: String, pub prior_context: Option<String> }
pub fn split_document_text(text: &str, policy: ChunkPolicy) -> Vec<DocumentChunk>;
pub fn bisect_chunk(chunk: &DocumentChunk, policy: ChunkPolicy) -> Result<[DocumentChunk; 2], AppError>;
pub fn build_system_prompt(catalog: &NamingCatalog) -> String;
```

- [ ] **Step 1: Port old split and prompt tests**

Cover sheet-title repetition, circuit-range-title repetition, single rows over capacity, no character loss, 30000/3000 defaults and all legacy prompt phrases governing groups, names, register types, endian, decimal positions and continuation rows.

- [ ] **Step 2: Implement boundary-first splitting**

Prefer blank-line section boundaries, sheet headings and range headings; then line boundaries; only then split a single overlong line by Unicode scalar boundaries. Count characters consistently, not UTF-8 bytes.

- [ ] **Step 3: Implement deterministic bisection**

On context overflow, split only the failing chunk into two smaller chunks, preserve inherited headings/context, and prevent endless bisection below the 8000-character floor by returning `context_too_large`.

- [ ] **Step 4: Verify after authorization**

Run: `cargo test chunking --manifest-path src-tauri/Cargo.toml && cargo test prompt --manifest-path src-tauri/Cargo.toml`

Expected: no fixture content is lost and prompt parity tests pass.

---

### Task 8: Implement the OpenAI-compatible client and extraction pipeline

**Files:**
- Create: `src-tauri/src/ai.rs`, `src-tauri/src/task.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: inline tests using a local mock HTTP server crate under dev-dependencies

**Interfaces:**
- Produces `AiClient::extract_chunk`, `ExtractionTaskManager::start`, `cancel`, `status`.
- Events:

```rust
pub enum TaskEvent {
  Stage { task_id: Uuid, stage: TaskStage },
  Log { task_id: Uuid, level: LogLevel, message: String },
  Progress { task_id: Uuid, completed_chunks: usize, total_chunks: usize },
  Completed { task_id: Uuid, output_path: PathBuf, record_count: usize },
  Cancelled { task_id: Uuid },
  Failed { task_id: Uuid, error: AppError },
}
```

- [ ] **Step 1: Test request shape and error classification**

Assert `/chat/completions` resolution, bearer header, configured model/temperature/max_tokens, system/user messages, no key leakage, empty response, 401/403, 429/5xx retry, timeout, cancellation and context-overflow response classification.

- [ ] **Step 2: Implement sequential chunk processing**

Emit a progress event after each successful chunk. Retry transient network/429/5xx failures at most twice with cancellable exponential delays. Do not retry authentication, invalid request or stable CSV-format failures.

- [ ] **Step 3: Implement context-overflow bisection**

Replace the failing chunk in the task queue with two deterministic child chunks. Preserve completed results and recompute the total chunk count event.

- [ ] **Step 4: Enforce one active extraction task**

Store one task handle and cancellation token in managed state. Reject a second start with `task_active`. Cancellation prevents saving partial output.

- [ ] **Step 5: Verify after authorization**

Run: `cargo test ai --manifest-path src-tauri/Cargo.toml && cargo test task --manifest-path src-tauri/Cargo.toml`

Expected: mocked end-to-end chunk processing, retry and cancellation tests pass.

---

### Task 9: Expose narrow Tauri commands and permissions

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/capabilities/default.json`
- Test: command-level Rust tests

**Interfaces:**
- Produces commands: `get_settings`, `save_settings`, `select_input_file`, `select_output_directory`, `start_extraction`, `cancel_extraction`, `get_task_status`, `open_output_directory`, `get_app_version`.
- Frontend never receives unrestricted shell or arbitrary write permissions.

- [ ] **Step 1: Test command validation**

Assert settings validation occurs before persistence, selected file validation occurs before `LAST_INPUT_DIR` update, output open is directory-only, and task commands preserve stable error DTOs.

- [ ] **Step 2: Register dialog and opener plugins with least privilege**

Grant only the main window the dialog, opener, process and updater permissions it needs. Keep arbitrary shell execution disabled. File parsing remains Rust-side after explicit native selection.

- [ ] **Step 3: Verify after authorization**

Run: `cargo test commands --manifest-path src-tauri/Cargo.toml`

Expected: command validation tests pass and capability JSON contains no broad shell permission.

---

### Task 10: Build the Mantine AWT workspace and settings flow

**Files:**
- Create: `src/components/AppSidebar.tsx`, `LogPanel.tsx`, `SettingsModal.tsx`
- Create: `src/features/awt/AwtWorkspace.tsx`, `useExtractionTask.ts`
- Create: `src/features/passthrough/PassthroughWorkspace.tsx`
- Modify: `src/App.tsx`, `src/api/tauri.ts`, `src/api/types.ts`, `src/app.css`
- Test: colocated `*.test.tsx`

**Interfaces:**
- Consumes Task 9 typed API and Task 8 events.
- Produces complete AWT workflow, settings modal and empty passthrough workspace.

- [ ] **Step 1: Test navigation and blank passthrough content**

Click each rounded navigation item, assert selected state and ensure the passthrough workspace contains no parser inputs or speculative copy.

- [ ] **Step 2: Test file selection, drag/drop, and processing state**

Mock typed API calls. Assert path/size rendering, invalid-file notification, start-to-stop button transition, stage text, chunk progress, 500-log eviction and terminal result states.

- [ ] **Step 3: Test confirmation flows**

Assert stop and window close request confirmation only while processing. Assert completed output offers `打开目录` and cancelled tasks do not show red errors.

- [ ] **Step 4: Test and implement settings validation**

Use Mantine form controls; validate positive timeout/tokens, chunk range 8000-60000, required base URL/model and output directory. Preserve hidden `lastInputDir` and never render the current API key in non-password text.

- [ ] **Step 5: Implement system theme and desktop styling**

Use Mantine color-scheme manager, compact spacing, blue-gray tokens, subtle surfaces, keyboard-visible focus states and responsive collapse behavior below the minimum content width.

- [ ] **Step 6: Verify after authorization**

Run: `npm run test:run && npm run typecheck && npm run lint`

Expected: UI tests, types and lint pass.

---

### Task 11: Add signed online update UI and backend

**Files:**
- Create: `src-tauri/src/updater.rs`, `src/components/UpdateModal.tsx`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/src/commands.rs`, `src-tauri/tauri.conf.json`, `src/App.tsx`, `src/api/tauri.ts`, `src/api/types.ts`
- Test: updater Rust tests and `src/components/UpdateModal.test.tsx`

**Interfaces:**
- Produces commands: `check_for_update(manual: bool)`, `download_and_install_update(channel)`, `relaunch_app`.
- Endpoint: `https://github.com/tyrival/Hummingbird-Releases/releases/latest/download/latest.json`.
- Update install is rejected while `ExtractionTaskManager` is active.

- [ ] **Step 1: Test SemVer, silent/manual errors, and active-task blocking**

Background failures produce no modal; manual failures show an error. A newer version produces notes/date; equal/older versions produce no update. Installation during extraction returns `update_blocked`.

- [ ] **Step 2: Configure updater artifacts and public key placeholder workflow**

Set `createUpdaterArtifacts: true`, HTTPS endpoint and updater permissions. The committed `pubkey` must be the generated public key content, never a path. Do not generate or store a private key in the repository.

- [ ] **Step 3: Implement download progress and relaunch**

Forward started/content-length/chunk/finished events to React. Require user confirmation before download and before relaunch. Verify signature before installation through the official plugin.

- [ ] **Step 4: Handle Linux package mode truthfully**

AppImage enables in-app install. A DEB-installed build checks and displays the update but opens the public Release page for manual DEB upgrade instead of replacing itself with AppImage.

- [ ] **Step 5: Generate signing keys only with explicit authorization**

Run after authorization: `npm run tauri signer generate -- -w <explicit-path-outside-repository>`.

Store the printed public key in `tauri.conf.json`. Add private key and password to private-source-repo secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Make a separate offline backup and verify it can be restored before deleting any temporary copy.

- [ ] **Step 6: Verify after authorization**

Run: `cargo test updater --manifest-path src-tauri/Cargo.toml && npm run test:run -- src/components/UpdateModal.test.tsx`

Expected: update state and UI tests pass without contacting GitHub.

---

### Task 12: Migrate icons, bundle resources, and document unsigned installation

**Files:**
- Copy/convert: `/Users/tyrival/workspace/Hummingbird-old/assets/AppIcons/.../1024.png` into Tauri icon outputs under `src-tauri/icons/`
- Modify: `src-tauri/tauri.conf.json`, `README.md`
- Create: `scripts/check-no-secrets.sh`
- Test: source-level shell assertions in `scripts/check-no-secrets.sh`

**Interfaces:**
- Produces branded bundles and user documentation.

- [ ] **Step 1: Configure bundled resources and icons**

Bundle only the naming CSV, Markdown fallback and generated icons. Do not copy `config.txt`, Python files, Flet archives, PyInstaller specs, `default.profraw` or old vendor binaries.

- [ ] **Step 2: Write truthful install documentation**

Document macOS ad-hoc/no-notarization first-open steps, Windows SmartScreen warning, Linux DEB dependencies, AppImage executable permission, settings migration, supported inputs, `.doc` limitation, output encoding and online update behavior.

- [ ] **Step 3: Add static secret checks**

Fail when tracked source contains private-key headers, `TAURI_SIGNING_PRIVATE_KEY=`, `RELEASES_REPO_TOKEN=`, bearer token literals, the old real `config.txt`, or common GitHub token prefixes. Allow documented secret names without values.

- [ ] **Step 4: Verify without compiling**

Run: `bash scripts/check-no-secrets.sh && git diff --check`

Expected: no secret-like values or whitespace errors.

---

### Task 13: Add CI and cross-repository release workflows

**Files:**
- Create: `.github/workflows/ci.yml`, `.github/workflows/release.yml`
- Modify: `README.md`
- Test: static YAML inspection; live execution only after authorized push/tag

**Interfaces:**
- Consumes GitHub Secrets: `RELEASES_REPO_TOKEN`, `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- Produces public release assets and `latest.json` in `tyrival/Hummingbird-Releases`.

- [ ] **Step 1: Implement CI workflow**

Trigger on pull requests, `main`, and `workflow_dispatch`. Run frontend lint/type/test and Rust fmt/clippy/test. Use a matrix for macOS ARM, macOS x86 target, Windows x64 and Ubuntu x64; install official Linux WebKit/GTK dependencies; upload build artifacts without creating a Release.

- [ ] **Step 2: Implement tag release workflow**

Trigger on `v*` and `workflow_dispatch`. Validate tag version equals Tauri/package version before building. Use `tauri-apps/tauri-action` with updater signing secrets and a repository token scoped to `tyrival/Hummingbird-Releases`.

- [ ] **Step 3: Normalize release names and checksums**

Publish exactly two DMGs, one NSIS EXE, one DEB, one AppImage, platform updater bundles/signatures, `latest.json` and `SHA256SUMS`. Generate checksums from explicit artifact paths and fail if an expected platform artifact is missing.

- [ ] **Step 4: Protect secrets and permissions**

Set ordinary CI permissions to `contents: read`. Give release steps only the cross-repo token through their environment. Do not echo environment variables. Keep forked pull requests unable to access release secrets.

- [ ] **Step 5: Static workflow verification before compile authorization**

Run source checks for triggers, matrix targets, repository name, secret names, expected asset extensions and absence of Apple notarization secrets. After authorized push, use the first manual workflow run to validate artifacts without publishing a stable version.

---

### Task 14: Final parity audit and release readiness

**Files:**
- Create: `docs/PARITY_CHECKLIST.md`, `docs/RELEASE_CHECKLIST.md`
- Modify: implementation files only for issues found by the audit

**Interfaces:**
- Produces explicit evidence that every confirmed design requirement maps to code and a test.

- [ ] **Step 1: Map every old test to a Rust or React test**

List each source test file from `Hummingbird-old/tests` and its replacement test. Mark Flet/PyInstaller runtime tests as replaced by Tauri shell, capabilities and workflow checks rather than silently dropping them.

- [ ] **Step 2: Run static-only audit before authorization**

Run `rg` checks for Python/Flet/PyInstaller dependencies, placeholder passthrough behavior, fixed CSV header, 30000/3000 defaults, 50 MB limit, public update URL and forbidden secrets. Run `git diff --check`.

- [ ] **Step 3: Run full verification after explicit compile authorization**

Run:

```text
npm run lint
npm run typecheck
npm run test:run
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

Expected: every command exits 0 and the local platform bundle is produced.

- [ ] **Step 4: Validate GitHub workflows after authorized commit/push**

Confirm CI artifacts for all matrix targets. Create a prerelease tag, verify the public release repository contains every expected asset, validate `SHA256SUMS`, fetch `latest.json`, and test one signed updater path before declaring release readiness.

- [ ] **Step 5: Do not commit without explicit authorization**

If the user later authorizes commits, group them by completed task with clear messages. Until then, leave all work as reviewable workspace changes and report exact verification limits.

---

## Execution Notes

- Tasks 1-3 establish contracts; Tasks 4-8 port the compatibility-critical Rust core; Tasks 9-11 connect the desktop UI and updater; Tasks 12-14 package, publish and audit.
- Do not start Task 10 before the Task 9 command DTOs are stable.
- Do not generate update keys until Task 11 and explicit user authorization; never place the private key under the workspace.
- `RELEASES_REPO_TOKEN` is already configured by the user. Its value must never be requested, read or printed.
- Any crate that cannot faithfully parse a confirmed fixture must be replaced behind the same module interface; UI contracts must not change to accommodate parser internals.
