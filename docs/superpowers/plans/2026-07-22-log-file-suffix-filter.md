# 日志文件后缀过滤 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 本地与远端日志选择仅根据 `.log`、`.gz` 后缀识别文件，不限制文件名前缀。

**Architecture:** 在 `sftp_download.rs` 提供一个不区分大小写的共享后缀判断函数，远端 SFTP 列表和本地目录扫描共同调用它。远端下载取消基于固定前缀的历史文件清理，仅由写文件操作覆盖本次选择的同名目标。

**Tech Stack:** Rust、Tauri 2、Rust 内置单元测试

## Global Constraints

- 不递归扫描子目录。
- 只接受普通文件。
- 后缀 `.log`、`.gz` 不区分大小写。
- 不编译代码。
- 不提交代码。

---

### Task 1: 统一日志文件名过滤规则

**Files:**
- Modify: `src-tauri/src/sftp_download.rs`
- Test: `src-tauri/src/sftp_download.rs`

**Interfaces:**
- Produces: `pub(crate) fn is_supported_log_file_name(name: &str) -> bool`

- [ ] **Step 1: 写失败测试**

验证任意名称的 `.log`、`.gz` 及大小写后缀被接受，其他后缀和伪后缀被拒绝。

- [ ] **Step 2: 运行定向测试并确认因函数尚不存在而失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml sftp_download::tests::recognizes_supported_log_file_suffixes`

- [ ] **Step 3: 实现最小共享过滤函数并用于远端列表**

将名称转为 ASCII 小写，再判断是否以 `.log` 或 `.gz` 结尾；远端列表仍通过 `stat.is_file()` 排除目录。

- [ ] **Step 4: 运行定向测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml sftp_download::tests::recognizes_supported_log_file_suffixes`

### Task 2: 本地扫描复用规则并收窄下载清理行为

**Files:**
- Modify: `src-tauri/src/analyse_commands.rs`
- Modify: `src-tauri/src/sftp_download.rs`
- Test: `src-tauri/src/analyse_commands.rs`

**Interfaces:**
- Consumes: `is_supported_log_file_name(name: &str) -> bool`

- [ ] **Step 1: 写失败测试**

把本地目录扫描提取为可测试函数，验证任意名称日志均被发现，其他文件和同后缀目录被排除。

- [ ] **Step 2: 运行定向测试并确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml analyse_commands::tests::finds_supported_logs_without_filename_prefix`

- [ ] **Step 3: 实现最小改动**

本地选择调用扫描函数；删除下载开始前按 `service-exchange.log` 前缀批量删除文件的代码。本次下载的同名文件仍由 `std::fs::write` 覆盖。

- [ ] **Step 4: 运行两个定向测试与静态格式检查**

Run: `cargo test --manifest-path src-tauri/Cargo.toml sftp_download::tests::recognizes_supported_log_file_suffixes analyse_commands::tests::finds_supported_logs_without_filename_prefix`

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`

- [ ] **Step 5: 检查差异，不提交**

Run: `git diff --check && git diff -- src-tauri/src/sftp_download.rs src-tauri/src/analyse_commands.rs`
