# 透传报文解析 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Hummingbird 中交付“透传报文解析”工作区，用确定性代码解析批量 Modbus、DL/T 645 和 CJ/T 188 报文，并在用户选择说明书或 AWT 模板时增加受控 AI 参量解释。

**Architecture:** Rust 后端先执行协议无关的空白清理与 `&&` 分割，再按平台外层/`FE` 前导路由到独立协议解析器；解析事实、资料映射和 AI 解释使用不同 DTO 层合并。React/Mantine 前端只管理输入、资料选择、任务状态和分层结果展示，不复制协议规则。

**Tech Stack:** React 19、TypeScript、Mantine、Vitest/Testing Library、Tauri 2、Rust 2021、Serde、现有文档提取/AI/CSV/参量目录模块。

## Global Constraints

- 菜单、页面标题和无障碍标签统一为“透传报文解析”。
- 副标题固定为“王大佬帮看下这报文什么意思？”。
- 删除空白并按双 `&&` 分割必须是整个流程第一步，并支持 Modbus、645、188 混合批次。
- 单个 `&` 非法；一个片段失败不得终止其他片段。
- 有 `68 + ASCII序列号 + 68` 外层或首字节非 `FE` 的片段按 Modbus；一个或多个 `FE` 开头的片段在剥离唤醒字节后区分标准 645/188。
- 协议分类、原始字段、地址、值和校验由代码确定，AI 不参与协议归类且不得覆盖代码事实。
- 不上传资料时不调用 AI；上传说明书或人工校准 AWT 模板时才增加解释层。
- 说明书支持 PDF、DOCX、XLS、XLSX、CSV；AWT 模板只接受标准 12 列 CSV。
- 不新增与本功能无关的设置、持久化或输出文件。
- 用户已授权测试和编译验证；仍未授权创建提交或 push。
- Modbus 实现以 Modbus Organization 的 Application Protocol V1.1b3 和 Serial Line V1.02 为准；645/188 实现必须对照项目合法取得的 DL/T 645 与 CJ/T 188 标准文本，不使用博客字段表作为协议事实来源。

## File Structure

### Rust 后端

- Create `src-tauri/src/passthrough/mod.rs`：公共 DTO、顶层 `parse_messages` 流程和结果合并。
- Create `src-tauri/src/passthrough/input.rs`：空白清理、`&&` 分割、Hex 解码和逐片段错误定位。
- Create `src-tauri/src/passthrough/checksum.rs`：Modbus CRC16 与 645/188 加和校验纯函数。
- Create `src-tauri/src/passthrough/envelope.rs`：平台 `68 + ASCII序列号 + 68` 外层解析。
- Create `src-tauri/src/passthrough/modbus.rs`：标准功能码、异常响应和私有 PDU 通用寄存器拆分。
- Create `src-tauri/src/passthrough/dlt645.rs`：`FE` 前导后的 DL/T 645 标准帧解析。
- Create `src-tauri/src/passthrough/cjt188.rs`：`FE` 前导后的 CJ/T 188 标准帧解析。
- Create `src-tauri/src/passthrough/register_map.rs`：AWT CSV 索引、参量目录关联和寄存器值换算。
- Create `src-tauri/src/passthrough/explanation.rs`：说明书临时映射、受控 AI 请求/返回校验和解释合并。
- Create `src-tauri/src/passthrough/commands.rs`：资料文件选择和解析 Tauri commands。
- Modify `src-tauri/src/lib.rs`：导出模块、管理解析状态、注册 commands。
- Modify `src-tauri/src/error.rs`：增加稳定的报文/资料错误码。
- Modify `src-tauri/src/ai.rs`：暴露复用现有兼容接口的结构化解释调用。
- Modify `src-tauri/src/prompt.rs`：增加报文解释 system/user prompt 构造器。

### 前端

- Create `src/features/passthrough/PassthroughWorkspace.tsx`：确认过的输入卡与结果区域组合。
- Create `src/features/passthrough/PassthroughWorkspace.test.tsx`：工作区交互和降级测试。
- Create `src/features/passthrough/MessageResultCard.tsx`：方案 B 的摘要、参数表和详情展开。
- Create `src/features/passthrough/MessageResultCard.test.tsx`：结果来源、警告和技术详情测试。
- Create `src/features/passthrough/usePassthroughParser.ts`：调用状态与结果状态机。
- Modify `src/api/types.ts`：请求、资料、结果和错误 DTO。
- Modify `src/api/tauri.ts`、`src/api/tauri.test.ts`：新增 command 封装和参数契约测试。
- Modify `src/components/AppSidebar.tsx`：新增“透传报文解析”菜单。
- Modify `src/App.tsx`、`src/App.test.tsx`：工作区路由和文案测试。
- Modify `src/app.css`：输入资料行、参数表和技术详情样式。

### Fixtures 与文档

- Create `tests/fixtures/passthrough/modbus.json`：用户样例、标准读写、异常响应、私有命令和 CRC 期望。
- Create `tests/fixtures/passthrough/dlt645.json`：从正式标准/项目真实日志核对的完整帧与字段期望。
- Create `tests/fixtures/passthrough/cjt188.json`：从正式标准/项目真实日志核对的完整帧与字段期望。
- Create `tests/fixtures/passthrough/awt-template.csv`：12 列地址映射、类型、字节序、倍率、枚举/位测试数据。
- Create `tests/fixtures/passthrough/README.md`：每条协议 fixture 的标准条款或真实样本来源，禁止无法追溯的测试 Hex。
- Modify `README.md`、`docs/PARITY_CHECKLIST.md`：说明能力边界和验证状态。

---

### Task 1: 输入分割、公共 DTO 与校验工具

**Files:**
- Create: `src-tauri/src/passthrough/mod.rs`
- Create: `src-tauri/src/passthrough/input.rs`
- Create: `src-tauri/src/passthrough/checksum.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/error.rs`

**Interfaces:**
- Produces: `input::split_hex_messages(&str) -> Vec<MessageInput>`，保留逐片段成功或错误。
- Produces: `checksum::modbus_crc16(&[u8]) -> u16`、`checksum::additive_checksum(&[u8]) -> u8`。
- Produces: `MessageParseResult`、`ProtocolKind`、`FieldValue`、`ChecksumResult`、`ParseWarning` 公共 DTO。

- [ ] **Step 1: 写输入和校验失败测试**

在 `input.rs` 与 `checksum.rs` 的 `#[cfg(test)]` 中增加测试，覆盖：

```rust
#[test]
fn splits_before_any_protocol_detection_and_accepts_mixed_frames() {
    let parsed = split_hex_messages(" 01 03 00 00 00 01 84 0A &&\nFE FE 68 11 22 16 && FE 68 33 44 16 ");
    assert_eq!(parsed.len(), 3);
    assert!(parsed.iter().all(|item| item.cleaned_hex.as_ref().is_some()));
}

#[test]
fn rejects_single_ampersand_without_losing_neighboring_segments() {
    let parsed = split_hex_messages("0103&0203&&0303");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].error.as_ref().unwrap().code, "invalid_separator");
    assert_eq!(parsed[1].cleaned_hex.as_deref(), Some("0303"));
}

#[test]
fn crc_matches_user_modbus_sample() {
    let bytes = hex_bytes("0420035400050A000D020301071A000000");
    assert_eq!(modbus_crc16(&bytes), 0x6849);
}
```

- [ ] **Step 2: 运行定向测试并确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml passthrough::input passthrough::checksum`

Expected: FAIL，因为 `passthrough` 模块和函数尚不存在。

- [ ] **Step 3: 实现最小输入层和 DTO**

公共类型至少包含以下稳定字段，全部使用 `#[serde(rename_all = "camelCase")]`：

```rust
pub enum ProtocolKind { ModbusRtu, Dlt645, Cjt188, Unknown }

pub struct FieldValue {
    pub name: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub raw_hex: String,
    pub display_value: String,
    pub source: FactSource,
}

pub struct MessageParseResult {
    pub index: usize,
    pub raw_segment: String,
    pub cleaned_hex: Option<String>,
    pub protocol: ProtocolKind,
    pub summary: String,
    pub fields: Vec<FieldValue>,
    pub registers: Vec<RegisterValue>,
    pub checksum: Option<ChecksumResult>,
    pub warnings: Vec<ParseWarning>,
    pub error: Option<PassthroughError>,
}
```

输入算法必须先删除 Unicode 空白，再扫描 `&&`；任何剩余 `&` 产生当前片段错误；Hex 奇数长度、非法字符和空片段分别返回稳定错误码。

- [ ] **Step 4: 实现无依赖校验函数**

用纯 Rust 实现 Modbus CRC16（初值 `0xFFFF`、多项式 `0xA001`、线序低字节在前）和逐字节加和低 8 位函数；不要引入只为两个小函数服务的新依赖。

- [ ] **Step 5: 运行定向测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml passthrough::input passthrough::checksum`

Expected: PASS，且用户私有 Modbus 样例 CRC 计算值为 `0x6849`。

- [ ] **Step 6: 审查检查点（不提交）**

Run: `git diff --check`

Expected: 无输出。保留改动未提交，等待用户授权后再进入下一任务。

### Task 2: 平台外层与 Modbus RTU 解析

**Files:**
- Create: `src-tauri/src/passthrough/envelope.rs`
- Create: `src-tauri/src/passthrough/modbus.rs`
- Create: `tests/fixtures/passthrough/modbus.json`
- Create: `tests/fixtures/passthrough/README.md`
- Modify: `src-tauri/src/passthrough/mod.rs`

**Interfaces:**
- Consumes: `modbus_crc16`、Task 1 公共 DTO。
- Produces: `envelope::strip_platform_envelope(&[u8]) -> EnvelopeResult`。
- Produces: `modbus::parse_modbus(&[u8], Option<&PlatformEnvelope>) -> ProtocolParse`。

- [ ] **Step 1: 固化用户真实样例 fixture**

`modbus.json` 至少写入：四条由 `&&` 分隔且带序列号 `26062509330001` 的样例、带序列号 `26042009700006` 的样例、直接私有命令样例 `0420035400050A000d020301071a0000004968`。每条记录包含 `input`、`platformSerial`、`slave`、`function`、`startAddress`、`quantity`、`crcValid` 和 `source`。

- [ ] **Step 2: 写外层与标准/私有 Modbus 失败测试**

```rust
#[test]
fn strips_ascii_serial_envelope_before_modbus() {
    let frame = hex_bytes("683236303632353039333330303031680110E000000677CB");
    let envelope = strip_platform_envelope(&frame).unwrap();
    assert_eq!(envelope.serial, "26062509330001");
    assert_eq!(bytes_to_upper_hex(&envelope.inner), "0110E000000677CB");
}

#[test]
fn parses_unknown_private_function_as_modbus_without_inventing_a_standard_name() {
    let frame = hex_bytes("0420035400050A000D020301071A0000004968");
    let parsed = parse_modbus(&frame, None);
    assert_eq!(parsed.function_code, 0x20);
    assert_eq!(parsed.function_kind, FunctionKind::Private);
    assert_eq!(parsed.start_address, Some(0x0354));
    assert_eq!(parsed.quantity, Some(5));
    assert_eq!(parsed.byte_count, Some(10));
    assert!(parsed.checksum.unwrap().valid);
}
```

测试辅助函数不要依赖生产解析器；Hex 解码和 `bytes_to_upper_hex` 使用测试内小函数，避免测试与实现共享错误，也不要只为测试显示引入 `hex` 依赖。

- [ ] **Step 3: 运行测试并确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml passthrough::envelope passthrough::modbus`

Expected: FAIL，因为解析器尚未实现。

- [ ] **Step 4: 实现平台外层解析**

仅在以下条件全部满足时接受外层：首字节 `0x68`、能找到第二个 `0x68`、二者之间为偶数个 ASCII 数字字节、序列号非空、第二个 `0x68` 后至少有 Modbus 地址/功能码/CRC。外层不参与 Modbus CRC。

- [ ] **Step 5: 实现标准 Modbus PDU**

用表驱动匹配标准请求/响应：`01/02/03/04/05/06/0F/10/16/17` 和异常响应 `function | 0x80`。输出读写方向、地址、数量、字节数、数据和异常码；响应缺少请求上下文时不得制造寄存器地址。

- [ ] **Step 6: 实现私有 PDU 通用拆分**

对未知功能码始终输出地址、功能码、PDU 和 CRC；仅当 PDU 同时满足 `start(2) + quantity(2) + byte_count(1) + data(byte_count)` 且 `byte_count == quantity * 2` 时，标记 `RegisterLayout::StartQuantityByteCount` 并逐 2 字节列寄存器。其他形态保留为原始 PDU 和候选警告，不强制拆寄存器。

- [ ] **Step 7: 运行测试与静态检查**

Run: `cargo test --manifest-path src-tauri/Cargo.toml passthrough::envelope passthrough::modbus`

Expected: PASS。

Run: `git diff --check`

Expected: 无输出；不提交。

### Task 3: DL/T 645 与 CJ/T 188 标准帧解析

**Files:**
- Create: `src-tauri/src/passthrough/dlt645.rs`
- Create: `src-tauri/src/passthrough/cjt188.rs`
- Create: `tests/fixtures/passthrough/dlt645.json`
- Create: `tests/fixtures/passthrough/cjt188.json`
- Modify: `tests/fixtures/passthrough/README.md`
- Modify: `src-tauri/src/passthrough/mod.rs`

**Interfaces:**
- Consumes: `additive_checksum` 与公共 DTO。
- Produces: `dlt645::parse_dlt645(&[u8], usize) -> ProtocolParse`。
- Produces: `cjt188::parse_cjt188(&[u8], usize) -> ProtocolParse`。
- Produces: 顶层 `route_protocol(&[u8]) -> ProtocolParse`，首字节非 `FE` 直接 Modbus；`FE+` 剥离后只在 645/188 中判别。

- [ ] **Step 1: 建立可追溯标准 fixture**

从项目合法取得的 DL/T 645 与 CJ/T 188 标准文本各选至少一条读请求、一条读响应、一条写请求，手工记录完整 Hex、唤醒字节数量、地址、控制码、数据长度、数据区、校验值和条款页码。将标准号、版本、页码和录入人写入 `tests/fixtures/passthrough/README.md`；任何字段未核对前不得进入实现。

- [ ] **Step 2: 写路由和完整帧失败测试**

测试必须逐条读取 JSON fixture，并断言：

```rust
assert_eq!(parsed.protocol, ProtocolKind::Dlt645); // 或 Cjt188
assert_eq!(parsed.wakeup_count, fixture.wakeup_count);
assert_eq!(parsed.address, fixture.address);
assert_eq!(parsed.control_code, fixture.control_code);
assert_eq!(parsed.data_hex, fixture.data_hex);
assert!(parsed.checksum.unwrap().valid);
```

另写 `FE` 数量为 1、2、4，以及长度错误、结束符错误、校验错误测试。

- [ ] **Step 3: 运行测试并确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml passthrough::dlt645 passthrough::cjt188`

Expected: FAIL，因为标准帧解析器尚不存在。

- [ ] **Step 4: 实现 645 解析器**

严格按 fixture 对应的 DL/T 645 标准版本解析双 `0x68` 帧标记、表地址、控制码、数据长度、数据域、校验和和 `0x16` 结束符；只对标准规定的数据域执行规定的编码/解码变换，并同时保留线上的原始 Hex。

- [ ] **Step 5: 实现 188 解析器**

严格按 fixture 对应的 CJ/T 188 标准版本解析仪表类型、地址、控制码、序号、数据长度、数据域、校验和与结束符；字段偏移来自标准，不复用 645 偏移。

- [ ] **Step 6: 实现 FE 路由冲突规则**

先统一计数并剥离 `FE`；分别运行 645/188 结构校验，只接受帧标记、声明长度、结束符和校验和全部成立的候选。双候选同时成立或都不成立时返回 `ProtocolKind::Unknown` 和候选诊断，不调用 AI 决策。

- [ ] **Step 7: 运行测试与检查**

Run: `cargo test --manifest-path src-tauri/Cargo.toml passthrough::dlt645 passthrough::cjt188 passthrough::tests::routes_mixed_batch`

Expected: PASS，包括 `Modbus&&645&&188` 混合批次。

Run: `git diff --check`

Expected: 无输出；不提交。

### Task 4: AWT 模板索引与寄存器值换算

**Files:**
- Create: `src-tauri/src/passthrough/register_map.rs`
- Create: `tests/fixtures/passthrough/awt-template.csv`
- Modify: `src-tauri/src/register_csv.rs`
- Modify: `src-tauri/src/naming.rs`

**Interfaces:**
- Consumes: `CSV_HEADER`、`sanitize_csv`、`NamingCatalog`、协议解析得到的 `RegisterValue`。
- Produces: `RegisterMap::from_awt_csv(&str, &NamingCatalog) -> Result<RegisterMap, AppError>`。
- Produces: `RegisterMap::explain(&[RegisterValue]) -> Vec<RegisterExplanation>`。

- [ ] **Step 1: 写 AWT 校验和映射失败测试**

Fixture 覆盖单寄存器、多寄存器、大小端、`dcm`、`k`、枚举 JSON、位状态、同地址 bit 项和不存在的 `data_name`。断言缺少任一标准列时返回列名列表，而不是把它当说明书 CSV。

- [ ] **Step 2: 运行测试并确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml passthrough::register_map`

Expected: FAIL，因为 `RegisterMap` 尚不存在。

- [ ] **Step 3: 实现严格 AWT 加载**

要求表头精确覆盖 12 列；复用现有地址归一化和命名目录，不重复实现 CSV 修复。构建 `BTreeMap<u16, Vec<RegisterDefinition>>`，保留同地址 bit 项和多寄存器占用范围。

- [ ] **Step 4: 实现值换算**

以代码解析的原始寄存器字节为输入，按模板的 `reg_type/endian/dcm/k/calc/style` 生成可选换算值；任何长度不足、未知类型或非法表达式只生成逐项警告，原始值始终保留。`data_name` 命中内置目录时附加中文说明；未命中时保留原编码。

- [ ] **Step 5: 运行测试与检查**

Run: `cargo test --manifest-path src-tauri/Cargo.toml passthrough::register_map`

Expected: PASS。

Run: `git diff --check`

Expected: 无输出；不提交。

### Task 5: 说明书临时映射与受控 AI 解释

**Files:**
- Create: `src-tauri/src/passthrough/explanation.rs`
- Modify: `src-tauri/src/ai.rs`
- Modify: `src-tauri/src/prompt.rs`
- Modify: `src-tauri/src/task.rs`

**Interfaces:**
- Consumes: 现有 `extract_document`、AI 配置、CSV 纠错/分块链路、协议事实和 `RegisterMap`。
- Produces: `build_manual_register_map(path, context) -> Result<RegisterMap, AppError>`。
- Produces: `explain_with_source(facts, source, ai_client) -> Result<Vec<RegisterExplanation>, AppError>`。

- [ ] **Step 1: 写“无资料绝不调用 AI”失败测试**

使用计数 mock：`parse_messages(request_without_source)` 后断言 AI 调用次数为 0，并保留协议结果。

- [ ] **Step 2: 写说明书/AWT 安全边界失败测试**

测试 AI 尝试修改地址、原始值、CRC 或协议时，该字段被丢弃并生成 `ai_fact_conflict`；测试 AI 无依据返回名称时生成 `missing_evidence`；测试 AWT 模板模式不允许 AI 创造私有命令定义。

- [ ] **Step 3: 运行测试并确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml passthrough::explanation`

Expected: FAIL，因为解释服务尚不存在。

- [ ] **Step 4: 复用说明书提取链路**

调用现有文档验证、文本提取、分块、AI CSV 生成、一次纠错和二分降级，生成内存中的 `RegisterMap`；不调用 `output` 保存文件。任务日志不得包含说明书正文、原始 AI 返回或 API key。

- [ ] **Step 5: 定义结构化 AI 返回**

返回 JSON 只允许：

```rust
pub struct AiRegisterExplanation {
    pub register_address: u16,
    pub parameter_code: Option<String>,
    pub parameter_name: Option<String>,
    pub converted_value: Option<String>,
    pub meaning: Option<String>,
    pub evidence_ids: Vec<String>,
    pub confidence: ExplanationConfidence,
}
```

Prompt 明确列出不可修改的事实和允许解释的字段；合并时以地址为键，找不到地址或依据 ID 的条目全部拒绝。

- [ ] **Step 6: 实现降级**

AI 未配置、认证失败、超时、空响应或 JSON 无效时返回解释层警告，不删除协议解析结果。说明书无法生成临时映射时，command 返回可选择“无资料继续”的状态；AWT 模板路径不依赖 AI。

- [ ] **Step 7: 运行测试与检查**

Run: `cargo test --manifest-path src-tauri/Cargo.toml passthrough::explanation`

Expected: PASS，包含零 AI 调用断言和冲突字段拒绝。

Run: `git diff --check`

Expected: 无输出；不提交。

### Task 6: Tauri command 与前端契约

**Files:**
- Create: `src-tauri/src/passthrough/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/error.rs`
- Modify: `src/api/types.ts`
- Modify: `src/api/tauri.ts`
- Modify: `src/api/tauri.test.ts`

**Interfaces:**
- Produces command: `select_passthrough_source(sourceKind) -> SelectedInput | null`。
- Produces command: `parse_passthrough_messages(request) -> PassthroughBatchResult`。
- Produces TS: `selectPassthroughSource`、`parsePassthroughMessages`。

- [ ] **Step 1: 写前端 API 失败测试**

```ts
await parsePassthroughMessages({ messageHex, source: null });
expect(mockedInvoke).toHaveBeenCalledWith('parse_passthrough_messages', {
  request: { messageHex, source: null },
});
```

另断言说明书选择扩展名为 PDF/DOCX/XLS/XLSX/CSV，AWT 模板只允许 CSV。

- [ ] **Step 2: 运行测试并确认失败**

Run: `npm run test:run -- src/api/tauri.test.ts`

Expected: FAIL，因为函数和 DTO 尚不存在。

- [ ] **Step 3: 定义共享 DTO 契约**

TS 类型逐字段镜像 Rust camelCase 序列化：`PassthroughSourceKind`、`PassthroughSource`、`PassthroughParseRequest`、`PassthroughBatchResult`、`MessageParseResult`、`RegisterExplanation`。不要使用 `Record<string, unknown>` 替代结果字段。

- [ ] **Step 4: 实现 command 与文件授权**

复用现有安全文件选择/临时 staging 机制，不允许前端传入任意未授权绝对路径。说明书走支持格式验证；AWT 模板走 `.csv` 与 12 列表头验证。解析 command 先对完整输入调用 Task 1 分割器，再逐段解析并保持输入顺序。

- [ ] **Step 5: 注册 command 并实现 API 封装**

在 `lib.rs` 的 `generate_handler!` 中注册两个 command；在 `tauri.ts` 中只通过 `invokeCommand` 暴露 typed wrapper，保持统一错误归一化。

- [ ] **Step 6: 运行契约测试与 Rust 定向测试**

Run: `npm run test:run -- src/api/tauri.test.ts`

Expected: PASS。

Run: `cargo test --manifest-path src-tauri/Cargo.toml passthrough::commands`

Expected: PASS；不提交。

### Task 7: 工作区、菜单和输入交互

**Files:**
- Create: `src/features/passthrough/PassthroughWorkspace.tsx`
- Create: `src/features/passthrough/PassthroughWorkspace.test.tsx`
- Create: `src/features/passthrough/usePassthroughParser.ts`
- Modify: `src/components/AppSidebar.tsx`
- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`
- Modify: `src/app.css`

**Interfaces:**
- Consumes: Task 6 前端 API/DTO。
- Produces: `Workspace` 增加 `'passthrough'`，`PassthroughWorkspace` 管理输入和批量结果。

- [ ] **Step 1: 写菜单与确认文案失败测试**

```ts
expect(screen.getByRole('button', { name: '透传报文解析' })).toBeInTheDocument();
fireEvent.click(screen.getByRole('button', { name: '透传报文解析' }));
expect(screen.getByRole('heading', { name: '透传报文解析' })).toBeInTheDocument();
expect(screen.getByText('王大佬帮看下这报文什么意思？')).toBeInTheDocument();
```

- [ ] **Step 2: 写输入卡布局与行为失败测试**

断言“开始解析”与“透传命令”处于同一 header；“寄存器说明书”“AWT模板”和文件选择位于同一行；页面不存在已删除的两段帮助文案；未选择文件时仍可解析；切换 AWT 后文件提示只显示 CSV。

- [ ] **Step 3: 运行测试并确认失败**

Run: `npm run test:run -- src/App.test.tsx src/features/passthrough/PassthroughWorkspace.test.tsx`

Expected: FAIL，因为工作区尚不存在。

- [ ] **Step 4: 实现工作区路由和静态布局**

在 Sidebar items 中新增 Tabler 报文/代码图标；`App.tsx` 与现有工作区一样用显示状态保留组件，不影响 AWT 和日志分析。输入卡严格使用已确认文案，不增加辅助资料说明文字。

- [ ] **Step 5: 实现状态机**

`usePassthroughParser` 状态固定为 `idle | selecting_source | extracting_source | parsing | completed | failed`。请求期间禁用输入、资料切换和菜单跳转；完成或失败后允许保留输入重新解析。无资料请求的 `source` 必须是 `null`。

- [ ] **Step 6: 实现选择文件状态**

寄存器说明书为默认选中类型，但“未选择文件”不是错误；切换类型时清空先前文件。选中文件后在同一行显示文件名和清除按钮，清除后回到无资料模式。

- [ ] **Step 7: 运行测试与检查**

Run: `npm run test:run -- src/App.test.tsx src/features/passthrough/PassthroughWorkspace.test.tsx`

Expected: PASS。

Run: `git diff --check`

Expected: 无输出；不提交。

### Task 8: 方案 B 结果卡与安全降级展示

**Files:**
- Create: `src/features/passthrough/MessageResultCard.tsx`
- Create: `src/features/passthrough/MessageResultCard.test.tsx`
- Modify: `src/features/passthrough/PassthroughWorkspace.tsx`
- Modify: `src/app.css`

**Interfaces:**
- Consumes: `MessageParseResult`。
- Produces: 每个 `&&` 片段一张结果卡，包含摘要、参数表和原生 `<details>` 技术详情。

- [ ] **Step 1: 写结果卡失败测试**

构造三个 DTO：无资料 Modbus、带 AWT 解释 Modbus、校验失败 645。断言无资料卡不显示伪造参量名；解释卡显示来源；校验失败仍显示字段；技术详情默认关闭并可展开。

- [ ] **Step 2: 运行测试并确认失败**

Run: `npm run test:run -- src/features/passthrough/MessageResultCard.test.tsx`

Expected: FAIL，因为组件尚不存在。

- [ ] **Step 3: 实现摘要和标签**

卡头显示“报文 N”、协议、读写类型、私有命令标记和校验状态。摘要优先使用已校验 AI 摘要，否则使用代码生成的确定性中文摘要。

- [ ] **Step 4: 实现参数表**

固定列为地址/标识、参量、原始值、换算值、解释。没有资料字段时显示 `—`，并保持原始地址和值；警告逐项关联到对应行或卡片。

- [ ] **Step 5: 实现技术详情**

使用 `<details>` 展示清洗后 Hex、平台序列号、FE 数量、字段偏移、PDU、报文校验值/计算值、解释来源和证据 ID。原始 Hex 使用等宽字体并允许换行复制。

- [ ] **Step 6: 运行测试与检查**

Run: `npm run test:run -- src/features/passthrough/MessageResultCard.test.tsx src/features/passthrough/PassthroughWorkspace.test.tsx`

Expected: PASS。

Run: `git diff --check`

Expected: 无输出；不提交。

### Task 9: 端到端回归、文档与交付验证

**Files:**
- Modify: `src-tauri/src/passthrough/mod.rs`
- Modify: `src/App.test.tsx`
- Modify: `README.md`
- Modify: `docs/PARITY_CHECKLIST.md`
- Test: `tests/fixtures/passthrough/*`

**Interfaces:**
- Consumes: Tasks 1-8 全部接口。
- Produces: 从用户输入到批量 UI 结果的稳定验收证据。

- [ ] **Step 1: 增加 Rust 批量验收测试**

用用户提供的带外层四条 Modbus `&&` 输入、直接私有 Modbus、标准 645 和标准 188 fixture 组成混合批次；断言分割先执行、顺序不变、协议正确、单条坏 CRC 不影响其他条。

- [ ] **Step 2: 增加前端完整流程测试**

Mock Tauri 返回混合批次，验证点击解析后逐卡显示；分别覆盖无资料零 AI、说明书提取失败后无资料继续、AWT 模板解释成功。

- [ ] **Step 3: 更新用户文档**

README 写清支持协议、`&&` 规则、资料源二选一、无资料不调用 AI、私有 Modbus 边界和校验失败仍展示字段。PARITY_CHECKLIST 将原“透传命令识别空白”改为对应模块与测试证据。

- [ ] **Step 4: 运行允许范围内的定向验证**

只有用户明确授权编译/测试后才运行：

Run: `cargo test --manifest-path src-tauri/Cargo.toml passthrough`

Expected: 所有 passthrough Rust 测试 PASS。

Run: `npm run test:run -- src/api/tauri.test.ts src/App.test.tsx src/features/passthrough`

Expected: 所有前端定向测试 PASS。

- [ ] **Step 5: 运行全量质量门禁**

只有用户明确授权编译后才运行：

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`

Run: `npm run test:run`

Run: `npm run typecheck`

Run: `npm run lint`

Run: `npm run build`

Expected: 全部退出码为 0；不得把 fixture/mock 验证描述成真实设备、真实 AI 或桌面运行验证。

- [ ] **Step 6: 最终静态检查与未提交交付**

Run: `git diff --check`

Run: `git status --short`

Expected: 无空白错误；只包含本功能设计、计划、源码、测试 fixture 和文档变化。除非用户届时明确要求，否则不创建 commit、不 push。

### Task 10: 工作区布局、全局设置入口与完整寄存器表

**Files:**
- Modify: `src/components/AppSidebar.tsx`
- Modify: `src/App.tsx`
- Modify: `src/features/awt/AwtWorkspace.tsx`
- Modify: `src/features/log-analysis/LogAnalysisWorkspace.tsx`
- Modify: `src/features/passthrough/PassthroughWorkspace.tsx`
- Modify: `src/features/passthrough/MessageResultCard.tsx`
- Modify: `src-tauri/src/passthrough/modbus.rs`
- Modify: `src/app.css`
- Test: `src/App.test.tsx`
- Test: `src/features/passthrough/*.test.tsx`
- Test: `src-tauri/src/passthrough/modbus.rs`

- [x] **Step 1:** 写全局设置入口、页面右上角设置移除、等宽字体、固定寄存器表和读请求地址展开测试。
- [x] **Step 2:** 运行定向测试确认新增断言失败。
- [x] **Step 3:** 将设置按钮移到侧栏底部“检查更新”下方，并复用 App 的设置弹窗状态。
- [x] **Step 4:** 透传页面布局对齐 AWT 工作区，输入框应用跨平台系统等宽字体。
- [x] **Step 5:** Modbus 读请求按起始地址和数量生成无值寄存器行；结果卡无条件显示表格并用 `—` 表示未知字段。
- [x] **Step 6:** 运行定向测试、全量测试、类型检查、lint、Clippy 和生产构建；不提交、不 push。

### Task 11: 直出协议字段与 0x10 请求/响应值语义

**Files:**
- Modify: `src-tauri/src/passthrough/modbus.rs`
- Modify: `src-tauri/src/passthrough/mod.rs`
- Modify: `src/features/passthrough/MessageResultCard.tsx`
- Modify: `src/features/passthrough/PassthroughWorkspace.tsx`
- Modify: `src/App.test.tsx`
- Test: `src-tauri/src/passthrough/modbus.rs`
- Test: `src/features/passthrough/MessageResultCard.test.tsx`
- Test: `src/features/passthrough/PassthroughWorkspace.test.tsx`

**Interfaces:**
- Consumes: `ProtocolParse.fields`, `ProtocolParse.registers`, `MessageParseResult.fields`。
- Produces: 按字节顺序可直接渲染的字段解释；`0x10` 请求携带值、响应明确缺值。

- [x] **Step 1:** 添加用户四条 `0x10` 响应和一条完整 `0x10` 请求的失败测试，断言响应寄存器值为空、请求寄存器值来自数据区。
- [x] **Step 2:** 添加结果卡失败测试，断言不存在“技术详情”，序列号、地址、功能码、范围、数量和 CRC 直接显示，响应表格显示“响应帧未携带写入值”。
- [x] **Step 3:** 运行 Rust 与前端定向测试并确认因字段不足和旧折叠布局失败。
- [x] **Step 4:** 补齐 Modbus 字段列表和 `0x10` 请求/响应分类，保持 CRC 与外层序列号字节范围准确。
- [x] **Step 5:** 重排结果卡为字段详情后接寄存器说明表，并恢复副标题及 AWT 同款间距。
- [x] **Step 6:** 运行定向测试并设置复核检查点；不提交、不 push。

### Task 12: 可取消的说明书资料提取

**Files:**
- Modify: `src-tauri/src/passthrough/commands.rs`
- Modify: `src-tauri/src/passthrough/explanation.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/tauri.ts`
- Modify: `src/features/passthrough/usePassthroughParser.ts`
- Modify: `src/features/passthrough/PassthroughWorkspace.tsx`
- Test: `src/api/tauri.test.ts`
- Test: `src/features/passthrough/PassthroughWorkspace.test.tsx`
- Test: `src-tauri/src/passthrough/commands.rs`

**Interfaces:**
- Produces: `cancel_passthrough_parse()` Tauri 命令；前端 `cancelPassthroughParse(): Promise<void>`；解析 hook 的 `cancel()`。

- [x] **Step 1:** 添加 API、hook/UI 和 Rust 取消行为失败测试，覆盖提取中按钮、取消状态及旧请求不覆盖。
- [x] **Step 2:** 运行定向测试确认取消接口尚不存在。
- [x] **Step 3:** 为透传解析维护单任务取消令牌，将令牌传入 `build_manual_register_map`，复用 AI 客户端已有取消检查。
- [x] **Step 4:** 注册取消命令并连接前端 API；取消错误按普通取消处理，恢复 `idle`。
- [x] **Step 5:** 运行定向测试并设置复核检查点。
- [x] **Step 6:** 运行全量前后端测试、类型检查、Lint、Rustfmt、Clippy 和生产构建，再执行 `git diff --check`；不提交、不 push。

### Task 13: 透传命令卡布局与多结果防压缩

**Files:**
- Modify: `src/features/passthrough/PassthroughWorkspace.tsx`
- Modify: `src/features/passthrough/MessageResultCard.tsx`
- Modify: `src/app.css`
- Test: `src/features/passthrough/PassthroughWorkspace.test.tsx`
- Test: `src/features/passthrough/MessageResultCard.test.tsx`

- [x] **Step 1:** 添加失败测试，断言支持协议位于命令标题下方、按钮与两行文本同属顶部左右结构、上传控件仍存在、结果区和结果卡带防压缩类。
- [x] **Step 2:** 运行前端定向测试，确认旧 footer、额外 `mt="lg"` 和缺少防压缩样式导致失败。
- [x] **Step 3:** 将支持协议移入命令卡标题区，删除底部 footer，并设置命令卡 `flexShrink: 0`。
- [x] **Step 4:** 移除结果 Stack 额外上边距，为结果容器和结果卡设置 `flex-shrink: 0`，保持工作区统一滚动。
- [x] **Step 5:** 运行定向测试作为复核检查点。
- [x] **Step 6:** 运行全量前端测试、类型检查、Lint、生产构建和 `git diff --check`；不提交、不 push。

### Task 14: 透传解析日志与成功/失败视图切换

**Files:**
- Modify: `src/components/LogPanel.tsx`
- Modify: `src/features/passthrough/usePassthroughParser.ts`
- Modify: `src/features/passthrough/PassthroughWorkspace.tsx`
- Test: `src/features/passthrough/PassthroughWorkspace.test.tsx`

- [x] **Step 1:** 添加失败测试，覆盖初始无日志、解析中显示日志、成功切换为结果卡、失败保留日志且不显示结果、取消日志。
- [x] **Step 2:** 运行定向测试确认当前解析期间没有日志框。
- [x] **Step 3:** 为解析 hook 增加有界日志状态，记录开始、资料模式、协议解析、警告、完成、失败和取消节点。
- [x] **Step 4:** 扩展 `LogPanel` 支持隐藏内部操作按钮和自定义活动文案，默认行为保持 AWT 不变。
- [x] **Step 5:** 在透传工作区按状态互斥渲染日志框或结果卡，并禁止日志框 Flex 压缩。
- [x] **Step 6:** 运行定向及全量前端测试、类型检查、Lint、构建和 `git diff --check`；不提交、不 push。

### Task 15: 请求与回复输入契约及双栏界面

**Files:**
- Modify: `src/api/types.ts`
- Modify: `src/features/passthrough/PassthroughWorkspace.tsx`
- Modify: `src/features/passthrough/usePassthroughParser.ts`
- Modify: `src/features/passthrough/PassthroughWorkspace.test.tsx`
- Modify: `src/app.css`

**Interfaces:**
- Produces: `PassthroughParseRequest { requestHex: string; responseHex: string | null; source: PassthroughSource | null }`。
- Produces: 左侧请求输入可单独提交；右侧回复存在而请求为空时前端日志失败，不调用 Tauri。

- [x] **Step 1:** 添加前端失败测试，断言左右输入框、请求单独解析、回复缺少请求时日志报错以及左右 `&&` 数量约束。
- [x] **Step 2:** 运行 `npm run test:run -- src/features/passthrough/PassthroughWorkspace.test.tsx src/api/tauri.test.ts`，确认旧单输入契约导致失败。
- [x] **Step 3:** 将请求 DTO、hook 日志和工作区表单改为双输入；请求非空即允许开始，回复仅能随请求提交。
- [x] **Step 4:** 添加双栏响应式 CSS：桌面左右等宽，窄窗口纵向排列，输入框保持系统等宽字体和防压缩布局。
- [x] **Step 5:** 重跑定向前端测试，确认通过并执行 `git diff --check` 作为检查点。

### Task 16: 后端请求—回复配对及协议上下文

**Files:**
- Modify: `src-tauri/src/passthrough/commands.rs`
- Modify: `src-tauri/src/passthrough/mod.rs`
- Modify: `src-tauri/src/passthrough/modbus.rs`
- Modify: `src-tauri/src/passthrough/dlt645.rs`
- Modify: `src-tauri/src/passthrough/cjt188.rs`
- Modify: `src/api/types.ts`
- Modify: `src/features/passthrough/MessageResultCard.tsx`
- Test: `src-tauri/src/passthrough/modbus.rs`
- Test: `src-tauri/src/passthrough/commands.rs`
- Test: `src/features/passthrough/MessageResultCard.test.tsx`

**Interfaces:**
- Produces: `MessageRole::{Request, Response}` 序列化为 `request|response`。
- Produces: `parse_message_pairs(request_hex: &str, response_hex: Option<&str>) -> Result<Vec<MessageParseResult>, AppError>`。
- Modbus `03/04` 回复在从站、功能码和字节数匹配时继承请求起始地址并生成寄存器地址。

- [x] **Step 1:** 添加 Rust 失败测试：请求单独成功、`03` 请求与回复关联为 `0x016D..0x017E`、数量不一致失败、从站/功能码不匹配不应用上下文。
- [x] **Step 2:** 添加 645/188 配对失败测试，校验仪表地址、控制码方向和数据标识；缺少确定证据时只报告不匹配，不猜测字段。
- [x] **Step 3:** 运行定向 Rust 测试确认缺少配对接口和角色字段。
- [x] **Step 4:** 实现成对解析和 Modbus 上下文应用，移除成功关联回复中的 `response_without_request_context` 警告。
- [x] **Step 5:** 为 645/188 增加基于已解析字段的配对验证，并给结果附加请求/回复角色和明确警告。
- [x] **Step 6:** 更新前端类型及结果卡标题，运行 Rust/前端定向测试作为检查点。

### Task 17: 说明书映射诊断、参量名称和六列表格

**Files:**
- Modify: `src-tauri/src/passthrough/register_map.rs`
- Modify: `src-tauri/src/passthrough/commands.rs`
- Modify: `src-tauri/src/passthrough/mod.rs`
- Modify: `src/api/types.ts`
- Modify: `src/features/passthrough/MessageResultCard.tsx`
- Modify: `src/features/passthrough/usePassthroughParser.ts`
- Test: `src-tauri/src/passthrough/register_map.rs`
- Test: `src/features/passthrough/MessageResultCard.test.tsx`
- Test: `src/features/passthrough/PassthroughWorkspace.test.tsx`

**Interfaces:**
- Produces: `RegisterMapDiagnostics { extracted_count, matched_count, unmatched_addresses }`。
- Extends: `AppliedRegisterExplanation` with `meaning: Option<String>`；说明书 `DOC_` 名称回退为去前缀后的原文名称。

- [x] **Step 1:** 添加失败测试，覆盖 `016DH/0x016D/365` 地址归一、DOC 原文名、映射数量和未命中地址诊断。
- [x] **Step 2:** 添加前端失败测试，断言六列表头、“单位”不再冒充“说明”、解释警告可见且日志包含提取/命中数量。
- [x] **Step 3:** 运行 Rust 和前端定向测试确认失败原因。
- [x] **Step 4:** 扩展 RegisterMap 查询和诊断接口，将 `meaning`、命中统计和未命中原因穿过 Tauri DTO。
- [x] **Step 5:** 修改结果卡支持同一寄存器的多条解释，渲染参量名称、解析值、单位和说明；无解释时显示具体原因。
- [x] **Step 6:** 将资料提取/命中统计写入成功日志，运行定向测试作为检查点。

### Task 18: ADW300 复合寄存器与完整回归

**Files:**
- Modify: `src-tauri/src/prompt.rs`
- Modify: `src-tauri/src/passthrough/register_map.rs`
- Modify: `src-tauri/src/passthrough/explanation.rs`
- Test: `src-tauri/src/prompt.rs`
- Test: `src-tauri/src/passthrough/register_map.rs`
- Test: `src/features/passthrough/MessageResultCard.test.tsx`
- Fixture: `tests/fixtures/passthrough/adw300-time-period.csv`

**Interfaces:**
- A packed 16-bit register is represented by two definitions at the same address using `reg_type=4`, with `endian=0` for the high byte and `endian=1` for the low byte, only when the source explicitly supplies field order.
- Produces multiple `AppliedRegisterExplanation` rows for one physical register without discarding either subfield.

- [x] **Step 1:** 添加 ADW300 `0x016D..0x017E` fixture 和失败测试，断言 `0x016D=0x0100` 解析为“第1时段费率号=1；第1时段起始分=0”。
- [x] **Step 2:** 添加 prompt 失败测试，要求对说明书明确的高/低字节复合字段输出同地址两条 `uint8` 记录，证据不明确时不得拆分。
- [x] **Step 3:** 运行定向测试确认当前 prompt 和 UI 只保留单条解释。
- [x] **Step 4:** 更新说明书提取提示与 RegisterMap 多解释输出，保持普通标量转换行为不变。
- [x] **Step 5:** 运行 ADW fixture、全部透传 Rust/前端测试并复核 PDF 第 16 页地址范围。
- [x] **Step 6:** 运行 `npm run test:run`、`npm run typecheck`、`npm run lint`、`npm run build`、`cargo fmt --all -- --check`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo check --workspace` 和 `git diff --check`；不提交、不 push。

### Task 19: 说明书默认端序与单位保留

**Files:**
- Modify: `src-tauri/src/prompt.rs`
- Test: `src-tauri/src/prompt.rs`
- Test: `src-tauri/src/passthrough/register_map.rs`

- [x] **Step 1:** 添加失败测试，断言提示要求保留明确单位、单寄存器 uint16/int16 未声明交换时按高字节在前解析，并验证 `089D/0896/08A2` 得到 `220.5/219.8/221.0`。
- [x] **Step 2:** 运行 prompt/register_map 定向测试确认旧规则“unit 留空”和 16 位端序编号解释错误导致失败。
- [x] **Step 3:** 修改提取提示，保留单位并将单寄存器整型默认设为 `endian=1`（高字节在前）；明确说明书声明优先。
- [x] **Step 4:** 重跑 Rust 定向测试和 `cargo fmt --check` 作为检查点。

### Task 20: 可编辑 HEX 的寄存器合并解析弹窗

**Files:**
- Create: `src/features/passthrough/registerMerge.ts`
- Create: `src/features/passthrough/RegisterMergeModal.tsx`
- Create: `src/features/passthrough/registerMerge.test.ts`
- Create: `src/features/passthrough/RegisterMergeModal.test.tsx`
- Modify: `src/features/passthrough/MessageResultCard.tsx`
- Modify: `src/features/passthrough/MessageResultCard.test.tsx`
- Modify: `src/app.css`

- [x] **Step 1:** 添加纯函数失败测试，覆盖空白清理、非法字符、奇数长度、ASCII、全量二进制及 Big/Little Endian 的 8/16/32/64 位前缀读取。
- [x] **Step 2:** 添加组件失败测试，覆盖物理寄存器复选框、无值禁选、合并按钮、初始 HEX、手动修改后点击解析、错误保留上次结果和关闭重置。
- [x] **Step 3:** 运行前端定向测试确认工具与弹窗尚不存在。
- [x] **Step 4:** 实现无副作用的 HEX 解析工具，使用 BigInt 安全生成 Int64/UInt64 字符串。
- [x] **Step 5:** 实现双列弹窗、等宽 HEX 输入、ASCII、整数值和逐字节位格；窄屏改为单列。
- [x] **Step 6:** 在结果卡按物理寄存器维护选择状态，表头增加 checkbox 列并将合并按钮放在“寄存器说明”右侧。
- [x] **Step 7:** 运行定向测试、前端全量测试、类型检查、Lint、构建及 `git diff --check`；不提交、不 push。

### Task 21: AWT 扩展列兼容与完整端序换算

**Files:**
- Modify: `src-tauri/src/passthrough/register_map.rs`
- Modify: `src-tauri/src/prompt.rs`
- Test: `src-tauri/src/passthrough/register_map.rs`

- [x] **Step 1:** 添加失败测试：AWT CSV 前 12 列合法时忽略其后附加列；前 12 列缺失或乱序仍拒绝。
- [x] **Step 2:** 添加表驱动失败测试，覆盖 uint16/int16 的 H1H2/H2H1、uint32/int32/float 的四种排列、uint64/int64 的大小端排列。
- [x] **Step 3:** 添加 `dcm` 显示精度失败测试，断言 `089D/0896/08A2` 在 `endian=1,dcm=1` 下得到 `220.5/219.8/221.0`。
- [x] **Step 4:** 修改 AWT CSV 读取，仅截取并验证前 12 列；记录不足 12 列或前 12 列契约不符时继续返回 `InvalidPassthroughSource`。
- [x] **Step 5:** 保持按 `reg_type` 的端序排列规则；表驱动测试确认 64 位现有实现符合编码，无需改写；格式化换算值时固定保留 `dcm` 位小数。
- [x] **Step 6:** 同步 AWT 生成提示，单寄存器 Modbus 默认 `endian=1`（高字节在前），并保留 32/64 位完整编码说明。
- [x] **Step 7:** 运行 Rust 定向测试、rustfmt、全量测试、Clippy、cargo check 和 `git diff --check`；不提交、不 push。

### Task 22: 透传页面标题与资料入口默认值

**Files:**
- Modify: `src/features/passthrough/PassthroughWorkspace.tsx`
- Test: `src/features/passthrough/PassthroughWorkspace.test.tsx`

- [x] **Step 1:** 更新前端失败测试，断言菜单仍为“透传报文解析”，页面 H2 与副标题使用确认文案，资料选项按“AWT模板”“AI识别说明书”排列且默认选择 AWT。
- [x] **Step 2:** 运行 `npm run test:run -- src/features/passthrough/PassthroughWorkspace.test.tsx src/App.test.tsx`，确认旧标题、旧选项文案和默认 `manual` 导致失败。
- [x] **Step 3:** 修改页面标题、副标题、SegmentedControl 选项顺序及 `sourceKind` 默认值，不改变 `manual/awt_template` API 值。
- [x] **Step 4:** 更新切换资料类型测试，确认默认按钮为“选择 CSV”，切换到“AI识别说明书”后显示说明书文件类型并清除旧选择。
- [x] **Step 5:** 运行前端定向测试、全量测试、TypeScript、ESLint、生产构建和 `git diff --check`；不提交、不 push。

### Task 23: 更新检查 15 秒超时

**Files:**
- Modify: `src-tauri/src/updater.rs`
- Modify: `src-tauri/src/error.rs`
- Modify: `src/api/types.ts`
- Test: `src-tauri/src/updater.rs`
- Test: `src-tauri/src/error.rs`
- Test: `src/App.test.tsx`

- [x] **Step 1:** 添加 Rust 失败测试，使用暂停时间的 pending Future 验证 15 秒前不结束、到时返回 `update_timeout`；同时验证普通 updater 错误仍走现有手动/后台策略。
- [x] **Step 2:** 添加错误契约测试及前端错误码契约，断言固定安全文案且 `ERROR_CODES` 包含 `update_timeout`。
- [x] **Step 3:** 用 `tokio::time::timeout(Duration::from_secs(15), updater.check())` 包装更新清单请求，并将 elapsed 映射为独立错误。
- [x] **Step 4:** 运行 updater/error 定向测试和 App 前端测试，确认手动检查通过 `finally` 恢复按钮、后台检查保持静默。
- [x] **Step 5:** 运行 Rust/前端全量测试、TypeScript、ESLint、rustfmt、Clippy、cargo check、生产构建和 `git diff --check`；不提交、不 push。
