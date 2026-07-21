# 平台日志分析 Implementation Plan

> **Goal:** Build log analysis workspace — download/parse iot-exchange logs, run batched AI analysis.
> **Tech:** Tauri 2, Rust (ssh2, aes-gcm, flate2, regex), React 19 + Mantine 9.

## Global Constraints
- SSH uses SFTP only (no shell/PTY). Credentials encrypted AES-256-GCM in config.txt.
- Large logs: streaming parse, AI analysis in 4 batched rounds (~10K chars each).
- Follow existing patterns: `TaskStage`, `EventSink`, `CancellationToken`, TDD.

## Task Summary

| # | Task | Files |
|---|------|-------|
| 1 | Crypto (AES encrypt/decrypt) | `crypto.rs` new, Cargo.toml |
| 2 | Log parser (streaming) | `log_parse.rs` new |
| 3 | Log stats aggregator | `log_stats.rs` new |
| 4 | SFTP download | `sftp_download.rs` new, Cargo.toml |
| 5 | Analyse config + commands | `analyse_commands.rs` new, lib.rs, commands.rs |
| 6 | Analysis task orchestrator | `analyse_task.rs` new |
| 7 | Frontend types + Tauri API | `api/types.ts`, `api/tauri.ts` |
| 8 | ServerListModal | `features/log-analysis/ServerListModal.tsx` |
| 9 | LogAnalysisWorkspace | `features/log-analysis/LogAnalysisWorkspace.tsx` |
|10 | AnalysisResults + charts | `features/log-analysis/AnalysisResults.tsx` |
|11 | Integration — App.tsx, sidebar | `App.tsx`, `AppSidebar.tsx`, `app.css` |
|12 | Integration tests + verify | `cargo test`, `npm test` |
