# AI CSV Extraction Resilience Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 恢复长说明书的可靠分块，并让可确定修复的 AI CSV 格式错误自动恢复。

**Architecture:** `register_csv` 负责纯函数式结构检查、定向修复和统计；`ai` 负责构造普通/格式纠错请求；`task` 负责一次纠错重试和失败后二分的状态机。设置层只调整默认值，不改变允许的用户配置范围。

**Tech Stack:** Rust、Tauri 2、reqwest、csv、tokio、现有 wiremock 单元测试。

## Global Constraints

- 不记录 API Key、说明书正文或 AI 原始响应。
- 只修复可以明确判定为缺少空 `unit` 的 11 列行。
- 默认分块为 12,000 字符，上下文为 1,500 字符。
- 无效块最多格式纠错一次，再进入二分；8,000 字符及以下不再二分。

---

### Task 1: 默认分块与定向 11 列修复

**Files:**
- Modify: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/chunking.rs`
- Modify: `src-tauri/src/register_csv.rs`

**Interfaces:**
- Produces: `SanitizedCsv` 增加结构统计；`sanitize_csv()` 返回修复后的十进制地址。

- [ ] 先写默认值和 11 列修复/拒绝测试并验证失败。
- [ ] 将默认值改为 12,000/1,500，实现严格形态判断与插列修复。
- [ ] 运行 `cargo test settings::tests register_csv::tests chunking::tests` 验证通过。

### Task 2: 格式纠错请求

**Files:**
- Modify: `src-tauri/src/ai.rs`

**Interfaces:**
- Consumes: CSV 结构统计摘要字符串。
- Produces: `extract_chunk_with_instruction(..., correction: Option<&str>)`，纠错说明只包含统计。

- [ ] 先写 wiremock 请求形态测试并验证失败。
- [ ] 增加纠错请求构造，保持普通请求完全兼容。
- [ ] 运行 `cargo test ai::tests` 验证通过。

### Task 3: 无效块重试与二分状态机

**Files:**
- Modify: `src-tauri/src/task.rs`

**Interfaces:**
- Consumes: `SanitizedCsv` 结构统计和 AI 纠错接口。
- Produces: 正常、纠错成功、二分成功、达到下限失败四条稳定路径。

- [ ] 先写任务测试覆盖纠错成功、纠错失败后二分、下限失败和日志脱敏。
- [ ] 实现一次纠错及二分队列逻辑，不重复已完成块。
- [ ] 运行 `cargo test task::tests` 验证通过。

### Task 4: 全量验证与实际样本验收

**Files:**
- Modify only if a test exposes a defect.

- [ ] 运行 Rust 全量测试、fmt、clippy。
- [ ] 运行前端测试、typecheck、lint、build。
- [ ] 对 ADF400L 的典型 11 列行运行定向清洗测试；核对 ASJ60 范围样例不会因一块格式错误直接退出。
- [ ] 运行 `git diff --check` 并确认没有密钥、PDF 或用户 CSV 被加入仓库。

