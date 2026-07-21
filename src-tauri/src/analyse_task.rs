use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::ai::AiClient;
use crate::error::{AppError, ErrorCode};
use crate::log_parse::{parse_log_file, ErrorCategory};
use crate::log_stats::{self, LogSummary};
use crate::settings::Settings;

pub type AnalyseEventSink = Arc<dyn Fn(AnalyseEvent) + Send + Sync + 'static>;

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AnalyseEvent {
    Stage {
        task_id: Uuid,
        stage: String,
    },
    Progress {
        task_id: Uuid,
        completed: u32,
        total: u32,
        detail: String,
    },
    AiChunk {
        task_id: Uuid,
        batch: u32,
        content: String,
    },
    Completed {
        task_id: Uuid,
        summary_json: String,
        heatmap_json: String,
    },
    Cancelled {
        task_id: Uuid,
    },
    Failed {
        task_id: Uuid,
        error: AppError,
    },
}

pub struct AnalyseTaskManager {
    inner: Arc<Mutex<AnalyseManagerState>>,
    terminal_changes: Arc<watch::Sender<u64>>,
}

#[derive(Default)]
struct AnalyseManagerState {
    active: bool,
    cancellation: Option<CancellationToken>,
}

impl AnalyseTaskManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(AnalyseManagerState::default())),
            terminal_changes: Arc::new(watch::channel(0).0),
        }
    }

    pub fn start(
        &self,
        file_paths: Vec<PathBuf>,
        settings: Settings,
        events: AnalyseEventSink,
    ) -> Result<Uuid, AppError> {
        let task_id = Uuid::new_v4();
        let cancellation = CancellationToken::new();
        {
            let mut state = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.active {
                return Err(AppError::new(ErrorCode::TaskActive));
            }
            state.active = true;
            state.cancellation = Some(cancellation.clone());
        }
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            let result =
                run_analysis(task_id, &file_paths, &settings, &events, &cancellation).await;
            match result {
                Ok(()) => {} // run_analysis emits its own Completed event
                Err(error) => {
                    (events)(AnalyseEvent::Failed {
                        task_id,
                        error: error.clone(),
                    });
                }
            }
            let mut state = manager
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.active = false;
            let _ = manager.terminal_changes.send(0);
        });
        Ok(task_id)
    }

    pub fn cancel(&self) -> Result<(), AppError> {
        let state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cancellation) = &state.cancellation {
            cancellation.cancel();
            Ok(())
        } else {
            Err(AppError::new(ErrorCode::NoActiveTask))
        }
    }

    pub fn is_active(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
    }
}

impl Clone for AnalyseTaskManager {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            terminal_changes: self.terminal_changes.clone(),
        }
    }
}

async fn run_analysis(
    task_id: Uuid,
    file_paths: &[PathBuf],
    settings: &Settings,
    events: &AnalyseEventSink,
    cancellation: &CancellationToken,
) -> Result<(), AppError> {
    // ── Stage 1: Parse logs (0-30%) ───────────────────────────
    (events)(AnalyseEvent::Stage {
        task_id,
        stage: "parsing".to_string(),
    });
    if cancellation.is_cancelled() {
        return Err(AppError::new(ErrorCode::Cancelled));
    }

    let mut all_entries = Vec::new();
    let total_files = file_paths.len();
    for (i, path) in file_paths.iter().enumerate() {
        let (fname, parsed_count, file_entries) = parse_log_file(path)?;
        all_entries.extend(file_entries);
        let pct = ((i + 1) as u32 * 30) / total_files as u32;
        emit_progress(
            events,
            task_id,
            pct,
            100,
            &format!(
                "{}/{}解析: {} → {} 条, 共 {} 条",
                i + 1,
                total_files,
                fname,
                parsed_count,
                all_entries.len()
            ),
        );
    }

    if cancellation.is_cancelled() {
        return Err(AppError::new(ErrorCode::Cancelled));
    }

    // ── Stage 2: Aggregate stats (30-40%) ─────────────────────
    emit_stage(events, task_id, "aggregating", 30, 40, "正在统计分析...");
    let summary = log_stats::aggregate(&all_entries);
    let samples = log_stats::sample_by_category(&all_entries, 5);
    let stuck_threads = log_stats::find_thread_stuck_candidates(&all_entries, 10_000);
    let heatmap = log_stats::time_heatmap(&all_entries, 10);
    emit_progress(events, task_id, 40, 100, "统计分析完成");

    if cancellation.is_cancelled() {
        return Err(AppError::new(ErrorCode::Cancelled));
    }

    // ── Stage 3: AI Batched Analysis (40-90%) ─────────────────
    let client = AiClient::new(settings)?;
    let ai_batches = build_ai_batches(&summary, &samples, &stuck_threads, &heatmap);
    let ai_batch_count = ai_batches.len() as u32;

    for (i, (batch_name, prompt)) in ai_batches.iter().enumerate() {
        if cancellation.is_cancelled() {
            return Err(AppError::new(ErrorCode::Cancelled));
        }
        emit_stage(
            events,
            task_id,
            "ai_analysis",
            40 + ((i as u32) * 50 / ai_batch_count),
            90,
            &format!("AI 分析: {}", batch_name),
        );

        let system = build_system_prompt_for_batch(batch_name);

        match client.chat(&system, &prompt, cancellation).await {
            Ok(content) => {
                (events)(AnalyseEvent::AiChunk {
                    task_id,
                    batch: i as u32,
                    content,
                });
            }
            Err(_e) => {
                (events)(AnalyseEvent::AiChunk {
                    task_id,
                    batch: i as u32,
                    content: format!("*分析失败*"),
                });
            }
        }

        let pct = 40 + ((i as u32 + 1) * 50 / ai_batch_count);
        emit_progress(events, task_id, pct, 100, "");
    }

    emit_progress(events, task_id, 100, 100, "分析完成");
    let summary_json = serde_json::to_string(&summary).unwrap_or_default();
    let heatmap_json = serde_json::to_string(&heatmap).unwrap_or_default();
    (events)(AnalyseEvent::Completed {
        task_id,
        summary_json,
        heatmap_json,
    });
    Ok(())
}

fn build_system_prompt_for_batch(batch_name: &str) -> String {
    format!(
        r#"你是一个 IoT 平台（acrel-iot）exchange 容器日志分析专家。

当前正在分析日志的 "{batch_name}" 部分。

需要你：
1. 解读统计数据和样本日志
2. 识别异常模式和根本原因
3. 评估平台健康状态
4. 给出具体、可操作的修复建议

使用中文回复，用 Markdown 格式。直接给出分析结果，不要复述指令。"#
    )
}

fn build_ai_batches(
    summary: &LogSummary,
    samples: &HashMap<ErrorCategory, Vec<crate::log_parse::LogEntry>>,
    stuck_threads: &[log_stats::ThreadStuckInfo],
    heatmap: &[log_stats::TimeBucket],
) -> Vec<(String, String)> {
    let mut batches = Vec::new();

    // Batch 1: Overview + Performance
    let mut b1 = String::new();
    b1.push_str("## 批次 1: 平台概览与性能分析\n\n");
    b1.push_str("请分析以下日志统计数据，评估平台整体健康状态，重点关注性能问题（线程卡顿、连接泄漏等）。\n\n");
    b1.push_str("### 基本统计\n");
    b1.push_str(&format!(
        "- 时间范围: {} ~ {}\n",
        summary.time_start.as_deref().unwrap_or("N/A"),
        summary.time_end.as_deref().unwrap_or("N/A")
    ));
    b1.push_str(&format!(
        "- 总行数: {}, 条目: {}, 线程数: {}\n",
        summary.total_lines, summary.entry_count, summary.thread_count
    ));
    b1.push_str(&format!("- 涉及设备 SN: {} 个\n", summary.unique_sns.len()));
    b1.push_str(&format!("- 连接泄漏次数: {}\n\n", summary.connection_leaks));

    b1.push_str("### 错误分类统计\n");
    for cc in &summary.category_counts {
        b1.push_str(&format!("- {}: {} 条\n", cc.category, cc.count));
    }
    b1.push('\n');

    if !stuck_threads.is_empty() {
        b1.push_str("### 线程卡顿（超过10秒）\n");
        for st in stuck_threads.iter().take(10) {
            b1.push_str(&format!("- `{}`: {}ms\n", st.thread, st.duration_ms));
        }
        b1.push('\n');
    }

    b1.push_str("### 时间热力图（每10分钟）\n");
    for bucket in heatmap.iter().take(20) {
        b1.push_str(&format!("- {}: {} 条\n", bucket.hour, bucket.count));
    }
    b1.push('\n');

    b1.push_str("请给出：\n1. 平台整体健康评估\n2. 性能风险等级和建议\n3. 最需要关注的 3 个问题\n");
    batches.push(("平台概览与性能".into(), b1));

    // Batch 2: Error categorization deep-dive
    let mut b2 = String::from("## 批次 2: 错误分类深度分析\n\n请对以下各类错误进行深入分析，给出每类错误的根因和修复建议。\n\n");
    for (cat, entries) in samples {
        b2.push_str(&format!("### {}\n\n", category_display_name(cat)));
        for entry in entries.iter().take(3) {
            b2.push_str(&format!(
                "- `{}` [{}] {}\n",
                entry.timestamp, entry.thread, entry.message
            ));
        }
        b2.push('\n');
    }
    b2.push_str("请给出：\n1. 各类错误的根因分析\n2. 每类的修复优先级和建议\n");
    batches.push(("错误分类深度分析".into(), b2));

    // Batch 3: Device SN analysis
    let mut b3 = String::from("## 批次 3: 设备维度的异常分析\n\n");
    if !summary.unique_sns.is_empty() {
        b3.push_str("### 涉及异常的设备 SN\n\n");
        for sn in summary.unique_sns.iter().take(20) {
            b3.push_str(&format!("- `{sn}`\n"));
        }
        b3.push('\n');
    }
    if !summary.dispatch_disabled_rules.is_empty() {
        b3.push_str("### 被禁用的 Dispatch 规则\n\n");
        for rule in &summary.dispatch_disabled_rules {
            b3.push_str(&format!("- {rule}\n"));
        }
        b3.push('\n');
    }
    b3.push_str("请给出：\n1. 异常设备的影响范围\n2. 是否需要紧急处理\n");
    batches.push(("设备维度分析".into(), b3));

    // Batch 4: Synthesis
    let mut b4 = String::from("## 批次 4: 综合分析报告\n\n");
    b4.push_str("请基于前面三个批次的分析结果，生成一份综合报告：\n\n");
    b4.push_str("1. 总体健康评分（1-10）\n");
    b4.push_str("2. 最关键 5 个问题及修复方案\n");
    b4.push_str("3. 短期（本周）和长期（本月）改进建议\n");
    b4.push_str("4. 是否需要紧急干预\n");
    batches.push(("综合分析报告".into(), b4));

    batches
}

fn category_display_name(cat: &ErrorCategory) -> &str {
    match cat {
        ErrorCategory::HttpRequest => "HTTP API请求",
        ErrorCategory::DeviceNotRegistered => "设备未注册",
        ErrorCategory::DispatchLifecycle => "Dispatch调度生命周期",
        ErrorCategory::DispatchDisabled => "Dispatch策略禁用",
        ErrorCategory::ProtocolTransformError => "协议转换错误",
        ErrorCategory::MqttAuthError => "MQTT认证错误",
        ErrorCategory::DeviceRegisterError => "设备注册参数缺失",
        ErrorCategory::TokenAuthError => "Token认证失败",
        ErrorCategory::ModbusTcpError => "Modbus TCP错误",
        ErrorCategory::ConnectionLeak => "连接泄漏",
        ErrorCategory::ByteBufError => "ByteBuf越界",
        ErrorCategory::SafeElectricError => "电气设备数据异常",
        ErrorCategory::EnergyProcessError => "Energy处理失败",
        ErrorCategory::AttachmentError => "附件/Token错误",
        ErrorCategory::Other => "其他",
    }
}

fn emit_stage(
    events: &AnalyseEventSink,
    task_id: Uuid,
    stage: &str,
    _pct: u32,
    _end_pct: u32,
    _detail: &str,
) {
    (events)(AnalyseEvent::Stage {
        task_id,
        stage: stage.to_string(),
    });
}

fn emit_progress(events: &AnalyseEventSink, task_id: Uuid, pct: u32, total: u32, detail: &str) {
    (events)(AnalyseEvent::Progress {
        task_id,
        completed: pct,
        total,
        detail: detail.to_string(),
    });
}
