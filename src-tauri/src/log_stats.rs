use serde::Serialize;
use std::collections::HashMap;

use crate::log_parse::{categorize_entry, ErrorCategory, LogEntry};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSummary {
    pub total_lines: usize,
    pub entry_count: usize,
    pub time_start: Option<String>,
    pub time_end: Option<String>,
    pub category_counts: Vec<CategoryCount>,
    pub unique_sns: Vec<String>,
    pub unique_projects: Vec<String>,
    pub connection_leaks: usize,
    pub dispatch_disabled_rules: Vec<String>,
    pub thread_count: usize,
    pub sn_errors: Vec<SnErrorCount>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnErrorCount {
    pub sn: String,
    pub error_type: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryCount {
    pub category: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeBucket {
    pub hour: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadStuckInfo {
    pub thread: String,
    pub start_time: String,
    pub end_time: String,
    pub duration_ms: u64,
}

pub fn aggregate(entries: &[LogEntry]) -> LogSummary {
    let entry_count = entries.len();
    let total_lines = entry_count
        + entries.iter().filter(|e| e.has_stack).count();
    let time_start = entries.first().map(|e| e.timestamp.clone());
    let time_end = entries.last().map(|e| e.timestamp.clone());

    let mut cat_map: HashMap<String, usize> = HashMap::new();
    let mut sns: Vec<String> = Vec::new();
    let mut projects: Vec<String> = Vec::new();
    let mut connection_leaks = 0usize;
    let mut dispatch_disabled = Vec::new();
    let mut threads: Vec<String> = Vec::new();
    let mut sn_error_map: HashMap<(String, String), usize> = HashMap::new();

    for entry in entries {
        let cat = categorize_entry(entry);
        let cat_name = category_name(cat);
        *cat_map.entry(cat_name.clone()).or_default() += 1;

        let mut entry_sns = crate::log_parse::extract_device_sns(&entry.message);
        for sn in &entry_sns {
            let key = (sn.clone(), cat_name.clone());
            *sn_error_map.entry(key).or_default() += 1;
        }
        sns.append(&mut entry_sns);

        if cat == ErrorCategory::ConnectionLeak {
            connection_leaks += 1;
        }
        if cat == ErrorCategory::DispatchDisabled {
            if let Some(rule) = crate::log_parse::extract_disabled_rule_id(&entry.message) {
                if !dispatch_disabled.contains(&rule) {
                    dispatch_disabled.push(rule);
                }
            }
        }
        if !threads.contains(&entry.thread) {
            threads.push(entry.thread.clone());
        }

        // Extract project names from messages
        if entry.message.contains("\"name\":\"") {
            let re = regex::Regex::new(r#""name":"([^"]+)""#).unwrap();
            for cap in re.captures_iter(&entry.message) {
                let name = cap[1].to_string();
                if !name.chars().all(|c| c.is_ascii_digit()) && !projects.contains(&name) {
                    projects.push(name);
                }
            }
        }
    }

    sns.sort();
    sns.dedup();

    let category_counts: Vec<CategoryCount> = {
        let mut counts: Vec<_> = cat_map
            .into_iter()
            .map(|(category, count)| CategoryCount { category, count })
            .collect();
        counts.sort_by(|a, b| b.count.cmp(&a.count));
        counts
    };

    LogSummary {
        total_lines,
        entry_count,
        time_start,
        time_end,
        category_counts,
        unique_sns: sns,
        unique_projects: projects,
        connection_leaks,
        dispatch_disabled_rules: dispatch_disabled,
        thread_count: threads.len(),
        sn_errors: sn_error_map
            .into_iter()
            .map(|((sn, error_type), count)| SnErrorCount { sn, error_type, count })
            .collect(),
    }
}

fn category_name(cat: ErrorCategory) -> String {
    match cat {
        ErrorCategory::HttpRequest => "HTTP API请求".into(),
        ErrorCategory::DeviceNotRegistered => "设备未注册".into(),
        ErrorCategory::DispatchLifecycle => "Dispatch调度生命周期".into(),
        ErrorCategory::DispatchDisabled => "Dispatch策略禁用".into(),
        ErrorCategory::ProtocolTransformError => "协议转换错误".into(),
        ErrorCategory::MqttAuthError => "MQTT认证错误".into(),
        ErrorCategory::DeviceRegisterError => "设备注册参数缺失".into(),
        ErrorCategory::TokenAuthError => "Token认证失败".into(),
        ErrorCategory::ModbusTcpError => "Modbus TCP错误".into(),
        ErrorCategory::ConnectionLeak => "连接泄漏".into(),
        ErrorCategory::ByteBufError => "ByteBuf越界".into(),
        ErrorCategory::SafeElectricError => "电气设备数据异常".into(),
        ErrorCategory::EnergyProcessError => "Energy处理失败".into(),
        ErrorCategory::AttachmentError => "附件/Token错误".into(),
        ErrorCategory::Other => "其他".into(),
    }
}

pub fn sample_by_category(
    entries: &[LogEntry],
    per_category: usize,
) -> HashMap<ErrorCategory, Vec<LogEntry>> {
    let mut samples: HashMap<ErrorCategory, Vec<LogEntry>> = HashMap::new();
    for entry in entries {
        let cat = categorize_entry(entry);
        let list = samples.entry(cat).or_default();
        if list.len() < per_category {
            list.push(entry.clone());
        }
    }
    samples
}

pub fn time_heatmap(entries: &[LogEntry], bucket_minutes: u32) -> Vec<TimeBucket> {
    let mut buckets: Vec<TimeBucket> = Vec::new();
    let mut current_key: Option<String> = None;
    let mut current_count: usize = 0;
    for entry in entries {
        let ts = &entry.timestamp;
        if ts.len() >= 16 {
            let hour: u32 = ts[11..13].parse().unwrap_or(0);
            let min: u32 = ts[14..16].parse().unwrap_or(0);
            let rounded = (min / bucket_minutes) * bucket_minutes;
            let key = format!("{} {:02}:{:02}", &ts[..10], hour, rounded);
            if current_key.as_deref() == Some(&key) {
                current_count += 1;
            } else {
                if let Some(key) = current_key.take() {
                    buckets.push(TimeBucket {
                        hour: key,
                        count: current_count,
                    });
                }
                current_key = Some(key);
                current_count = 1;
            }
        }
    }
    if let Some(key) = current_key {
        buckets.push(TimeBucket {
            hour: key,
            count: current_count,
        });
    }
    buckets
}

pub fn find_thread_stuck_candidates(
    entries: &[LogEntry],
    threshold_ms: u64,
) -> Vec<ThreadStuckInfo> {
    let mut candidates = Vec::new();
    let mut start_map: HashMap<String, (String, usize)> = HashMap::new();

    for (i, entry) in entries.iter().enumerate() {
        if entry.message.starts_with("[DISPATCH-START]") {
            start_map.insert(
                entry.thread.clone(),
                (entry.timestamp.clone(), i),
            );
        } else if entry.message.starts_with("[DISPATCH-END]") {
            if let Some((start_time, _)) = start_map.remove(&entry.thread) {
                if let Some(cost) = crate::log_parse::extract_dispatch_cost(&entry.message) {
                    if cost > threshold_ms {
                        candidates.push(ThreadStuckInfo {
                            thread: entry.thread.clone(),
                            start_time,
                            end_time: entry.timestamp.clone(),
                            duration_ms: cost,
                        });
                    }
                }
            }
        }
    }
    candidates.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms));
    candidates
}

pub fn to_ai_prompt_input(
    summary: &LogSummary,
    samples: &HashMap<ErrorCategory, Vec<LogEntry>>,
    stuck_threads: &[ThreadStuckInfo],
    heatmap: &[TimeBucket],
) -> String {
    let mut buf = String::new();
    buf.push_str("## 日志概览\n\n");
    buf.push_str(&format!(
        "- 时间范围: {} ~ {}\n",
        summary.time_start.as_deref().unwrap_or("N/A"),
        summary.time_end.as_deref().unwrap_or("N/A")
    ));
    buf.push_str(&format!(
        "- 总行数: {}, 日志条目: {}, 线程数: {}\n",
        summary.total_lines, summary.entry_count, summary.thread_count
    ));
    buf.push_str(&format!("- 涉及设备 SN: {} 个\n", summary.unique_sns.len()));
    buf.push_str(&format!("- 连接泄漏: {} 次\n\n", summary.connection_leaks));

    buf.push_str("## 错误分类统计\n\n");
    for cc in &summary.category_counts {
        buf.push_str(&format!("- {}: {} 条\n", cc.category, cc.count));
    }
    buf.push('\n');

    buf.push_str("## 各类错误代表性样本\n\n");
    for (cat, entries) in samples {
        buf.push_str(&format!("### {}\n\n", category_name(*cat)));
        for entry in entries.iter().take(3) {
            buf.push_str(&format!(
                "```
{} [{}] {}
```
\n",
                entry.timestamp, entry.thread, entry.message
            ));
            if entry.has_stack {
                buf.push_str("  *(包含异常堆栈)*\n\n");
            }
        }
        buf.push('\n');
    }

    if !stuck_threads.is_empty() {
        buf.push_str("## ⚠️ 线程卡顿风险\n\n");
        buf.push_str("以下线程的 DISPATCH 执行时间超过阈值：\n\n");
        for st in stuck_threads {
            buf.push_str(&format!(
                "- 线程 `{}`: {}ms ({} ~ {})\n",
                st.thread, st.duration_ms, st.start_time, st.end_time
            ));
        }
        buf.push('\n');
    }

    if !summary.dispatch_disabled_rules.is_empty() {
        buf.push_str("## 被禁用的 Dispatch 规则\n\n");
        for rule in &summary.dispatch_disabled_rules {
            buf.push_str(&format!("- ruleId={}\n", rule));
        }
        buf.push('\n');
    }

    buf.push_str("## 时间热力图 (每10分钟)\n\n");
    for bucket in heatmap.iter().take(30) {
        buf.push_str(&format!("- {}: {} 条\n", bucket.hour, bucket.count));
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<LogEntry> {
        vec![
            LogEntry {
                timestamp: "2026-07-20 16:55:54.765".into(),
                thread: "http-nio-20001-exec-24".into(),
                class: "com.acrel.aspect.router.RequestRouterAspect".into(),
                line: 155,
                message: "Exclude injection param Tenant".into(),
                has_stack: false,
            },
            LogEntry {
                timestamp: "2026-07-20 16:56:00.001".into(),
                thread: "global-schedule-task-6".into(),
                class: "com.acrel.xch.mqtt.sys.service.impl.SysServiceImpl".into(),
                line: 962,
                message: "SysService handlerLoginCache device not register: 25012004594201"
                    .into(),
                has_stack: false,
            },
            LogEntry {
                timestamp: "2026-07-20 16:56:00.003".into(),
                thread: "dispatch-executor-5".into(),
                class: "com.acrel.disp.service.impl.DispatchServiceImpl".into(),
                line: 468,
                message: "[DISPATCH-START] thread=dispatch-executor-5 tid=1249767391"
                    .into(),
                has_stack: false,
            },
            LogEntry {
                timestamp: "2026-07-20 16:57:04.198".into(),
                thread: "dispatch-executor-5".into(),
                class: "com.acrel.disp.service.impl.DispatchServiceImpl".into(),
                line: 537,
                message:
                    "[DISPATCH-END] thread=dispatch-executor-5 tid=1249767391 cost=64195ms"
                        .into(),
                has_stack: false,
            },
        ]
    }

    #[test]
    fn aggregates_correctly() {
        let entries = sample_entries();
        let summary = aggregate(&entries);
        assert_eq!(summary.entry_count, 4);
        assert!(summary.unique_sns.contains(&"25012004594201".to_string()));
        assert_eq!(summary.connection_leaks, 0);
        assert!(!summary.category_counts.is_empty());
    }

    #[test]
    fn finds_stuck_threads() {
        let entries = sample_entries();
        let stuck = find_thread_stuck_candidates(&entries, 60_000);
        assert_eq!(stuck.len(), 1);
        assert_eq!(stuck[0].thread, "dispatch-executor-5");
        assert_eq!(stuck[0].duration_ms, 64195);
    }

    #[test]
    fn no_stuck_threads_below_threshold() {
        let entries = sample_entries();
        let stuck = find_thread_stuck_candidates(&entries, 100_000);
        assert!(stuck.is_empty());
    }

    #[test]
    fn samples_by_category() {
        let entries = sample_entries();
        let samples = sample_by_category(&entries, 2);
        assert!(!samples.is_empty());
    }

    #[test]
    fn generates_heatmap() {
        let entries = sample_entries();
        let heatmap = time_heatmap(&entries, 10);
        assert!(!heatmap.is_empty());
        assert_eq!(heatmap[0].hour, "2026-07-20 16:50");
    }

    #[test]
    fn generates_ai_prompt() {
        let entries = sample_entries();
        let summary = aggregate(&entries);
        let samples = sample_by_category(&entries, 3);
        let stuck = find_thread_stuck_candidates(&entries, 60_000);
        let heatmap = time_heatmap(&entries, 10);
        let prompt = to_ai_prompt_input(&summary, &samples, &stuck, &heatmap);
        assert!(prompt.contains("线程卡顿风险"));
        assert!(prompt.contains("64195ms"));
        assert!(prompt.contains("25012004594201"));
    }
}
