# Hummingbird Tauri 重写设计

## 背景

当前 GitHub 仓库只有初始 README。可工作的旧版源码位于同级 `Hummingbird-old` 目录，使用 Python、Flet、PyInstaller，并保留一套更早的 Tkinter/FastAPI 路径。本项目在当前仓库内使用 Tauri 2 完整重写桌面应用，不保留 Python 运行时，同时保持旧版 AWT 模板生成的业务能力和用户流程。

新版落实此前确认但未进入旧版源码的左侧双菜单：`AWT模板生成` 和 `透传命令识别`。本期只迁移 AWT 模板生成功能；透传命令识别保留空白工作区，不创建没有输入输出契约的解析能力。

## 目标

- 使用 React、TypeScript、Mantine 和 Tauri 2 重写桌面界面。
- 使用 Rust 重写文档解析、AI 请求、分块、CSV 清洗合并、命名参考、配置与输出逻辑。
- 保持旧版 AWT 模板生成的功能和处理结果兼容，并用迁移后的测试样例验证。
- 首次启动时自动迁移旧版 `config.txt`，且不修改或删除旧配置。
- 支持 GitHub Actions 持续验证及基于 `v*` 标签的自动发布。
- 发布 macOS Apple Silicon DMG、macOS Intel DMG、Windows x64 EXE、Linux x64 DEB 和 Linux x64 AppImage。
- macOS 不依赖 Apple Developer ID，不公证；使用 ad-hoc 签名。Windows 安装包不签名。
- 使用独立公开发布仓库提供在线版本检查和用户确认后的一键更新，源码仓库保持私有。

## 非目标

- 不保留 Flet、Tkinter、FastAPI、PyInstaller 或 Python sidecar。
- 不在本期实现透传命令识别业务。
- 不实现 Apple 公证、Mac App Store、Microsoft Store 或 Windows 代码签名。
- 不针对特定设备型号硬编码寄存器规则。
- 不新增 OCR；扫描版 PDF 没有可搜索文本时给出明确错误。

## 技术选型

- 桌面框架：Tauri 2。
- 前端：React、TypeScript、Vite。
- 组件库：Mantine。
- 图标：Tabler Icons。
- 后端：Rust，所有核心模块可脱离窗口独立测试。
- Tauri 能力：原生文件/目录对话框、受限文件访问、打开目录、Rust 命令与事件。
- 在线更新：`tauri-plugin-updater`、`tauri-plugin-process`、GitHub Releases 静态 `latest.json` 和 Tauri Ed25519 更新签名。

选择 Mantine 是为了使用成熟的表单、Modal、Notification、Progress、ScrollArea 和深浅主题能力，同时保持比默认 Material Design 更中性的桌面工具视觉。React 只管理视图和任务状态，业务规则不进入前端。

## 总体架构

```text
React UI
  |- 左侧导航
  |- AWT 模板生成工作区
  |- 透传命令识别空白工作区
  |- 设置对话框
  `- 日志、进度、完成与退出确认
          | Tauri commands / events
          v
Rust 应用层
  |- extraction: PDF、DOCX、XLS、XLSX、CSV 文本提取
  |- ai: OpenAI 兼容接口、分块请求、取消与进度事件
  |- register_csv: 清洗、排序、去重、地址占用冲突、回路归组
  |- naming: 正式电参量 CSV 与 Markdown 回退
  |- settings: 配置存储和旧版 config.txt 首次迁移
  |- output: UTF-8 BOM CSV 保存和输出目录管理
  `- updater: 版本检查、签名验证、下载、安装和重启
```

长时间处理作为 Rust 后台任务运行。Rust 通过事件发送阶段、日志和进度，React 不轮询文件或直接执行网络请求。停止操作使用协作式取消：不再开始后续分块、不再保存取消后的结果，并尝试终止正在进行的 HTTP 请求；第三方服务是否立即停止计算不作为保证。

## 界面与交互

主窗口标题为 `Hummingbird`，默认约 860 x 620，支持缩放并跟随系统浅色或深色外观。视觉采用紧凑、专业的桌面工程工具风格，不像素级复刻 Flet。

### 导航

- 左侧使用低对比度表面色，顶部显示应用图标和名称。
- 两个纵向圆角菜单：`AWT模板生成`、`透传命令识别`。
- 当前项使用克制的蓝色强调，避免夸张阴影和动画。
- 透传命令识别页面为空白内容区，不显示虚构能力或未定义的输入组件。

### AWT 模板生成工作区

- 显示标题、格式说明和文件选择卡片。
- 支持点击选择以及把单个文件拖入窗口。
- 显示所选文件绝对路径、格式与大小。
- `开始提取` 在任务期间切换为 `停止`。
- 使用阶段文字和细进度条表达状态；由于单个 AI 请求没有可靠百分比，界面不展示虚假精确百分比。
- 日志包含时间戳、普通、成功、警告和错误级别，最多保留 500 条。
- 底部显示当前输出目录，提供 `打开输出目录` 和 `设置`。
- 完成后显示记录数和保存路径，并提供打开目录操作。
- 处理中停止或关闭窗口时必须二次确认。
- 应用启动后延迟检查新版本，不阻塞主界面；检查失败不打扰正常工作。
- 有新版本时显示版本号、发布日期和发布说明，只有用户点击后才下载并安装。
- 更新下载显示真实字节进度，安装完成后由用户确认是否立即重启。
- AWT 提取任务运行期间禁止安装更新，避免任务或输出中断。

### 设置

设置使用尺寸适中的 Mantine Modal，包含：

- API 地址；
- API 密钥，默认遮盖且允许临时显示；
- 模型名称；
- 请求超时秒数；
- 最大输出 Token；
- 输出目录；
- 高级设置中的单块最大字符数。

设置页同时显示当前应用版本并提供 `检查更新`。该操作由用户主动触发，因此检查失败时显示明确错误；后台自动检查失败只记录诊断信息。

超时、最大输出 Token 和单块最大字符数保存前必须校验为合法正整数。单块最大字符数允许 `8000` 到 `60000`。`LAST_INPUT_DIR` 是隐藏配置，不显示在设置中。

## 输入格式与校验

- 文件上限为 50 MB。
- 可选择扩展名：`.pdf`、`.docx`、`.doc`、`.xls`、`.xlsx`、`.csv`。
- PDF 提取每页可搜索文字；无文字时提示可能为扫描版，不在本期执行 OCR。
- DOCX 提取正文段落与表格内容。
- XLS 和 XLSX 读取全部工作表，包括隐藏工作表，保留内部空单元格位置与工作表标题。
- CSV 支持 UTF-8 BOM、UTF-8、GB18030 及兼容回退编码，并检测逗号、分号和制表符等常见分隔符。
- 旧版允许选择 `.doc`，但实际用 DOCX 解析器读取二进制 DOC，通常失败。新版仍允许选择该扩展名，但明确提示用户另存为 DOCX，不把这个旧缺陷继续伪装为可用能力。
- 文件通过格式和大小校验后才更新最近输入目录。取消选择或校验失败不更新。

## 文本分块

整份文档不设置总字符数上限，所有提取出的有效内容均进入处理链路。

- 默认单块最大 `30000` 字符。
- 相邻块附带最多 `3000` 字符的上一块末尾上下文。
- 优先按工作表、章节、表格和回路范围边界切分。
- 工作表标题和仍有效的回路范围标题在续块开头重复。
- 超长单行按容量安全拆分，不丢失字符。
- 续块上下文只用于理解跨块表格，提示模型不得重复输出上一块记录。
- 如果服务明确返回上下文超限，当前块自动二分并重试，已成功的其他块不重跑。
- 默认逐块请求，避免并发轰炸用户配置的接口，并保持日志、取消和结果顺序可理解。

## AI 接口

使用用户配置的 OpenAI Chat Completions 兼容接口。请求包括固定 system prompt、完整电参量命名参考、当前文档块，以及非首块所需的上一块上下文。

AI 任务逐块产生阶段和日志事件。普通网络失败最多重试两次，采用指数退避。认证失败、余额不足、请求参数错误和确定性的格式错误不自动重试。空响应区分普通空内容与推理 Token 耗尽，并给出可操作的中文提示。

取消是正常终止状态，不显示为红色错误。取消后不保存部分 CSV。

## CSV 业务规则

固定表头为：

```csv
id,group,data_name,unit,reg_add,reg_type,endian,dcm,k,fun_num,calc,style
```

结果处理保持旧版规则：

- 移除 Markdown 代码围栏、重复表头和兼容旧输出的中文解释行。
- 使用标准 CSV 解析器处理引号、逗号和换行字段。
- 校验 12 列结构；损坏的记录形成可见警告，不让整份输出静默失真。
- `group` 必须是大于等于 1 的整数，非法值回退为 1。
- `CH6`、`CH 6`、`Channel 6`、`第六路`、`第6回路`、`6号回路`、`回路6` 等明确前缀用于校正 group，并从 data_name 移除回路编号。
- 范围表达不能单独当作具体回路；公共参数不凭地址强制归组。
- 回路编号与标准参量匹配相互独立。去除明确回路前缀后，匹配到正式名称则使用标准编码，未匹配则使用一次 `DOC_` 前缀。
- 合并跨块结果时移除精确重复记录，按寄存器地址排序并重新生成连续 id。
- 同一地址允许多个 bit 字段。
- 对多寄存器类型执行地址占用检查；同地址单寄存器记录与多寄存器记录冲突时保持旧版优先规则。
- 证据相同但内容冲突时保留先出现记录并产生警告，不输出两条互相矛盾的正确结果。

最终 CSV 使用 UTF-8 BOM，文件名为：

```text
原文件名_yyyyMMdd_HHmmss_随机6位.csv
```

## 电参量命名数据源

运行时优先读取随应用打包的 `t_electric_param.csv`：

- 必须包含 `ParamCode` 和 `ParamName`，列顺序不限。
- 忽略空行、空编码和空含义。
- 相同编码和含义去重；相同编码的不同含义按首次出现顺序用 ` / ` 合并。
- CSV 至少有一条有效记录时，它是完整权威来源，不混入 Markdown。
- CSV 缺失、无法解码、字段无效或没有有效记录时，回退 `naming-convention.md`。
- AI 提示词与返回值名称校验必须消费同一次加载得到的名称集合。

不复制旧版真实 `config.txt`。正式 CSV 和 Markdown 兼容数据可以从旧工程迁移到仓库资源目录。

## 配置与首次迁移

新版在操作系统标准应用配置目录保存 `settings.json`，不依赖安装目录可写。配置包含显式 schema 版本和 `migrationVersion`。

仅当新版配置不存在时检测旧版 `config.txt`：

- macOS 优先检查 `~/Library/Application Support/Hummingbird/config.txt`。
- Windows 和 Linux 检查旧版可执行文件附近及历史应用数据目录。
- 支持 UTF-16、UTF-8 BOM、UTF-8、GB18030 和 CP1252。
- 只导入 `AI_API_BASE_URL`、`AI_API_KEY`、`AI_MODEL`、`AI_REQUEST_TIMEOUT`、`AI_MAX_TOKENS`、`OUTPUT_DIR` 和 `LAST_INPUT_DIR`。
- 导入后写入迁移版本，之后不重复覆盖用户配置。
- 不删除、不修改旧文件；迁移失败不阻止应用启动，而是使用默认配置并记录警告。

为了保持旧版使用方式，API 密钥保存在当前用户的应用配置文件中。密钥不进入日志、错误消息、GitHub Actions、测试快照或仓库。UI 默认遮盖密钥。

默认值：

```text
AI_API_BASE_URL=http://192.168.32.20:3000/v1
AI_API_KEY=
AI_MODEL=deepseek-chat
AI_REQUEST_TIMEOUT=600
AI_MAX_TOKENS=16384
OUTPUT_DIR=output
LAST_INPUT_DIR=
AI_CHUNK_MAX_CHARS=30000
```

相对输出目录解析到应用数据目录。绝对目录按用户选择原样保存。

## 错误模型

Rust 使用稳定的错误类别向前端返回简洁中文信息，同时保留不含密钥的详细上下文：

- 文件：不存在、超过上限、格式不支持、为空或无可提取文字；
- 解析：具体格式、页或工作表失败；
- 配置：URL、正整数范围或输出目录不合法；
- 网络：连接、超时、TLS、HTTP 状态码；
- AI：认证、余额、上下文超限、空响应、输出格式；
- 保存：目录不可写、磁盘或文件系统错误；
- 更新：清单不可达、版本无效、签名验证失败、下载中断、安装或重启失败；
- 取消：正常终止，不作为错误。

前端 Notification 只显示摘要，日志区显示可操作细节。任何日志和错误在输出前执行密钥脱敏。

## 测试策略

### Rust 单元测试

- PDF、DOCX、XLS、XLSX、CSV 文本规范化边界；
- CSV 编码和分隔符识别；
- 文本分块、标题继承、上下文、超长行和自动二分；
- 电参量 CSV 加载、去重、含义合并及 Markdown 回退；
- 回路范围识别、具体回路前缀解析和 data_name 清理；
- CSV 代码围栏、中文解释、列数、group、DOC_、排序、去重和地址占用规则；
- 配置编码、合法性、目录解析和一次性旧版迁移；
- 网络重试分类、取消与错误脱敏。
- 更新清单解析、SemVer 比较、平台架构选择、签名失败和活动任务拦截。

### 兼容金样

把旧版 Python 测试的输入输出迁移为 Rust 测试和固定 fixture，覆盖：

- 全工作表及隐藏工作表；
- UTF-8 BOM 与 GB18030 CSV；
- 分块和跨块上下文；
- 寄存器地址排序及冲突；
- 多 bit 字段；
- `CH`、`Channel`、中文数字和回路前缀；
- 正式命名 CSV 与 Markdown 回退；
- 最近目录与配置保留。

### React 测试

- 两项导航及空白第二工作区；
- 选择、拖放、格式和大小状态；
- 设置字段和范围校验；
- 任务阶段、日志上限、停止与关闭确认；
- 完成提示和输出目录操作；
- 深浅主题令牌。
- 后台静默检查、手动检查、新版本对话框、下载进度和重启确认。

### Tauri 边界测试

- 命令参数和返回 DTO；
- 后台任务事件协议；
- 每次只允许一个活动提取任务；
- 文件路径权限和打开目录行为；
- Rust 错误到前端错误类型的映射。
- updater 命令权限、状态事件和提取任务互斥。

不依赖脆弱的原生系统对话框截图断言。

## 在线更新

### 仓库边界

- `tyrival/Hummingbird` 是私有源码仓库，包含源码、测试和 GitHub Actions 定义。
- `tyrival/Hummingbird-Releases` 是公开二进制发布仓库，只包含 GitHub Release、安装包、更新专用产物、`.sig`、`latest.json`、`SHA256SUMS` 和发布说明。
- 发布仓库不包含源码、用户 AI 密钥、Tauri 更新私钥或发布用 GitHub Token。
- 客户端不内置 GitHub 私有仓库访问令牌。

应用使用以下公开 HTTPS 地址检查更新：

```text
https://github.com/tyrival/Hummingbird-Releases/releases/latest/download/latest.json
```

### 更新签名

Tauri updater 的 Ed25519 签名与 Apple Developer ID、macOS 代码签名及公证相互独立。在线更新签名不可关闭：

- 更新公钥写入 `tauri.conf.json`，可以公开。
- 更新私钥只保存为私有源码仓库的 `TAURI_SIGNING_PRIVATE_KEY` GitHub Secret。
- 私钥密码保存为 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` GitHub Secret。
- 私钥必须在仓库外做离线恢复备份；丢失后，已安装版本无法信任使用新密钥签署的后续更新。
- GitHub Actions 使用私钥生成更新产物及 `.sig`，但不把私钥写入文件、日志或 Release。

### 更新行为

- 启动后延迟数秒执行一次后台检查，每次启动最多自动检查一次。
- 设置页允许用户随时手动检查。
- 只有 SemVer 高于当前版本才提示更新，不实现强制升级或远程降级。
- 用户确认后下载并验证完整更新包，签名验证通过才安装。
- 更新下载失败允许重试，不影响当前安装。
- 安装前再次确认没有活动提取任务；安装完成后提示重启。
- macOS updater 使用签名的 `.app.tar.gz`，Windows 使用签名的 NSIS 更新包。
- Tauri 官方 Linux updater 使用 AppImage，因此 Linux 同时发布 DEB 和 AppImage：DEB 用于传统安装和手动升级，AppImage 用于应用内一键更新。通过 DEB 安装时，只提示新版本并打开公开 Release 下载页，不尝试用 AppImage 覆盖 DEB 安装。

## GitHub Actions

### 持续集成

`.github/workflows/ci.yml` 在 `main`、Pull Request 和手动触发时运行：

- 前端格式检查、类型检查和测试；
- Rust 格式检查、静态检查和测试；
- macOS ARM、macOS Intel 目标、Windows x64 和 Ubuntu x64 打包验证；
- 安装包保存为 Actions Artifacts，不创建 Release。

### 正式发布

`.github/workflows/release.yml` 在私有源码仓库推送 `v*` 标签或手动触发时运行，在公开 `tyrival/Hummingbird-Releases` 创建同版本 GitHub Release 并上传：

```text
Hummingbird_<version>_macos_aarch64.dmg
Hummingbird_<version>_macos_x86_64.dmg
Hummingbird_<version>_windows_x64-setup.exe
Hummingbird_<version>_linux_amd64.deb
Hummingbird_<version>_linux_amd64.AppImage
SHA256SUMS
latest.json
```

- macOS 使用 ad-hoc 签名，不使用 Apple Developer ID，不公证。发布说明包含首次打开方式。
- Windows 使用 Tauri 的 EXE 安装包且不签名。发布说明提示可能出现 SmartScreen 警告。
- Linux DEB 声明所需 WebKit/GTK 运行依赖；AppImage 作为在线更新载体同时发布。
- 同一 Release 还包含 macOS `.app.tar.gz`、Windows NSIS 和 Linux AppImage 的更新签名 `.sig`；面向终端用户的下载列表可通过发布说明区分普通安装包与更新内部产物。
- 私有源码仓库使用一个仅能写入 `Hummingbird-Releases` Releases 的细粒度 GitHub Token Secret；普通 CI 保持只读权限。
- `latest.json` 包含版本、发布说明、RFC 3339 日期以及各操作系统和架构的更新 URL 与 `.sig` 内容。
- 工作流不注入用户 AI 密钥。

Tauri 官方 GitHub Actions 指南支持按 macOS ARM、macOS Intel、Windows 和 Linux 矩阵构建，并建议在没有 Apple 证书时使用 ad-hoc 签名：<https://v2.tauri.app/distribute/pipelines/github/>。

## 验收标准

- 应用在代码层面不依赖 Python、Flet、Tkinter、FastAPI 或 PyInstaller。
- AWT 模板生成具备旧版所有有效输入、设置、日志、取消、保存和打开目录流程。
- 旧版业务测试所覆盖的 Rust 金样全部通过。
- 长说明书不因总长度被截断，默认 30000 字符分块，超限可自动二分。
- 透传命令识别入口存在且内容为空，不包含未定义实现。
- 首次启动可迁移旧配置，之后不重复覆盖；旧文件保持不变。
- 后台检查不阻塞启动；手动检查、新版本提示、签名下载、安装和重启流程可用。
- 更新私钥不进入仓库或 Release，签名无效的更新绝不安装。
- DEB 安装只提示并打开 Release 页面；Linux AppImage、macOS 和 Windows 支持应用内更新。
- `main` 工作流验证三平台，`v*` 标签从私有源码仓库向公开发布仓库发布两个 DMG、一个 EXE、一个 DEB、一个 AppImage、更新专用产物、`latest.json` 和校验文件。
- 未签名和未公证状态在发布说明中如实披露。

## 开发约束

- 未经用户明确授权，不在本地执行编译或打包命令。
- 未经用户明确授权，不执行 Git commit 或 push。
- 实施阶段优先运行不触发编译的源码静态检查；完整 Rust、前端测试和安装包验证需要另行获得编译授权，或由远端 GitHub Actions 在代码进入远端后执行。
