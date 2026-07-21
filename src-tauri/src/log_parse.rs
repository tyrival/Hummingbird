use chardetng::EncodingDetector;
use encoding_rs::GB18030;
use flate2::read::GzDecoder;
use regex::Regex;
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
    sync::LazyLock,
};

use crate::error::{AppError, ErrorCode};

static LOG_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}) \[(\w+)\]\[([^\]]+)\]\[([^:]+):(\d+)\] (.*)$",
    )
    .unwrap()
});

static STACK_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\t(at |Caused by:|\.\.\.\d+ more)").unwrap()
});

static SN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(\d{14,})\b").unwrap());
static DISPATCH_COST_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[DISPATCH-END\].*cost=(\d+)ms").unwrap());
static DISABLED_RULE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"disabled ruleId=(\d+)").unwrap());
static LEAK_COUNT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Clear leak clients end: (\d+)").unwrap());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub timestamp: String,
    pub thread: String,
    pub class: String,
    pub line: u32,
    pub message: String,
    pub has_stack: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    HttpRequest,
    DeviceNotRegistered,
    DispatchLifecycle,
    DispatchDisabled,
    ProtocolTransformError,
    MqttAuthError,
    DeviceRegisterError,
    TokenAuthError,
    ModbusTcpError,
    ConnectionLeak,
    ByteBufError,
    SafeElectricError,
    EnergyProcessError,
    AttachmentError,
    Other,
}

pub fn parse_log_line(line: &str) -> Option<LogEntry> {
    let caps = LOG_LINE_RE.captures(line)?;
    Some(LogEntry {
        timestamp: caps[1].to_string(),
        thread: caps[3].to_string(),
        class: caps[4].to_string(),
        line: caps[5].parse().unwrap_or(0),
        message: caps[6].to_string(),
        has_stack: false,
    })
}

pub fn is_stack_line(line: &str) -> bool {
    STACK_LINE_RE.is_match(line)
        || (!line.starts_with("202") && line.starts_with('\t'))
}

pub fn categorize_entry(entry: &LogEntry) -> ErrorCategory {
    let msg = &entry.message;
    let cls = &entry.class;

    if cls.contains("RequestRouterAspect") {
        ErrorCategory::HttpRequest
    } else if msg.contains("device not register") {
        ErrorCategory::DeviceNotRegistered
    } else if msg.contains("[DISPATCH-START]")
        || msg.contains("[DISPATCH-END]")
        || msg.contains("[DISPATCH-COST]")
    {
        ErrorCategory::DispatchLifecycle
    } else if msg.contains("DispatchStrategyCount fail")
        || msg.contains("disable window")
        || msg.contains("disabled ruleId=")
    {
        ErrorCategory::DispatchDisabled
    } else if cls.contains("ComposeHandler")
        || msg.contains("未包含协议网关转换规则")
    {
        ErrorCategory::ProtocolTransformError
    } else if cls.contains("MqttAuthService") || msg.contains("URISyntaxException") {
        ErrorCategory::MqttAuthError
    } else if msg.contains("参数缺失") {
        ErrorCategory::DeviceRegisterError
    } else if msg.contains("登录信息错误") {
        ErrorCategory::TokenAuthError
    } else if msg.contains("CommonService.getData error") {
        ErrorCategory::ModbusTcpError
    } else if msg.contains("Clear leak clients end:") {
        let leaked = LEAK_COUNT_RE
            .captures(msg)
            .and_then(|c| c[1].parse().ok())
            .unwrap_or(0);
        if leaked > 0 {
            ErrorCategory::ConnectionLeak
        } else {
            ErrorCategory::Other
        }
    } else if cls.contains("AlgorithmHandler") {
        ErrorCategory::ByteBufError
    } else if cls.contains("SafeElectricServiceImpl") {
        ErrorCategory::SafeElectricError
    } else if cls.contains("EnergyStatusCacheImpl")
        || cls.contains("FrEnergyServiceImpl")
    {
        ErrorCategory::EnergyProcessError
    } else if cls.contains("AttachmentServiceImpl")
        || msg.contains("TokenFilter Exception")
    {
        ErrorCategory::AttachmentError
    } else {
        ErrorCategory::Other
    }
}

pub fn extract_device_sns(message: &str) -> Vec<String> {
    SN_RE
        .find_iter(message)
        .map(|m| m.as_str().to_string())
        .collect()
}

pub fn extract_dispatch_cost(message: &str) -> Option<u64> {
    DISPATCH_COST_RE
        .captures(message)
        .and_then(|c| c[1].parse().ok())
}

pub fn extract_disabled_rule_id(message: &str) -> Option<String> {
    DISABLED_RULE_RE
        .captures(message)
        .map(|c| c[1].to_string())
}

pub fn parse_log_file(path: &Path) -> Result<(String, usize, Vec<LogEntry>), AppError> {
    let file = File::open(path).map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    let mut raw = Vec::new();
    if file_name.ends_with(".gz") {
        let decoder = GzDecoder::new(file);
        let mut reader = BufReader::new(decoder);
        reader
            .read_to_end(&mut raw)
            .map_err(|_| AppError::new(ErrorCode::ParseFailed))?;
    } else {
        let mut reader = BufReader::new(file);
        reader
            .read_to_end(&mut raw)
            .map_err(|_| AppError::new(ErrorCode::ParseFailed))?;
    }

    let text = decode_log_bytes(&raw);
    let _total_lines = text.lines().filter(|l| !l.trim().is_empty()).count();
    let entries = parse_log_text(&text)?;
    let count = entries.len();
    Ok((file_name.into_owned(), count, entries))
}

fn decode_log_bytes(bytes: &[u8]) -> String {
    // Try UTF-8 first
    if let Ok(text) = std::str::from_utf8(bytes) {
        if !text.contains('\u{fffd}') {
            return text.to_owned();
        }
    }
    // Try GB18030 (common on Chinese Linux servers)
    let (text, _, had_errors) = GB18030.decode(bytes);
    if !had_errors {
        return text.into_owned();
    }
    // Fallback: detect encoding
    let mut detector = EncodingDetector::new();
    detector.feed(bytes, true);
    let encoding = detector.guess(None, true);
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}

fn parse_log_text(text: &str) -> Result<Vec<LogEntry>, AppError> {
    let mut entries: Vec<LogEntry> = Vec::new();
    for line in text.lines() {
        if is_stack_line(line) {
            if let Some(last) = entries.last_mut() {
                last.has_stack = true;
            }
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(entry) = parse_log_line(trimmed) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_log_line() {
        let line = "2026-07-20 16:55:54.765 [ERROR][http-nio-20001-exec-24][com.acrel.aspect.router.RequestRouterAspect:155] Exclude injection param Tenant";
        let entry = parse_log_line(line).unwrap();
        assert_eq!(entry.timestamp, "2026-07-20 16:55:54.765");
        assert_eq!(entry.thread, "http-nio-20001-exec-24");
        assert_eq!(entry.class, "com.acrel.aspect.router.RequestRouterAspect");
        assert_eq!(entry.line, 155);
        assert_eq!(entry.message, "Exclude injection param Tenant");
    }

    #[test]
    fn parses_device_query_line() {
        let line = "2026-07-20 16:56:00.217 [ERROR][http-nio-20001-exec-18][com.acrel.aspect.router.RequestRouterAspect:129] action = 查询设备详情";
        let entry = parse_log_line(line).unwrap();
        assert_eq!(entry.message, "action = 查询设备详情");
        assert_eq!(entry.line, 129);
    }

    #[test]
    fn recognizes_stack_lines() {
        assert!(is_stack_line(
            "\tat com.acrel.filter.TokenFilter.doFilter(TokenFilter.java:171)"
        ));
        assert!(!is_stack_line("Caused by: java.lang.RuntimeException"));
        assert!(!is_stack_line(
            "2026-07-20 16:55:54.765 [ERROR][...] message"
        ));
        // Tab-indented non-timestamp line
        assert!(is_stack_line(
            "\tcom.acrel.exceptions.CommonException: 参数缺失"
        ));
    }

    #[test]
    fn categorizes_device_not_registered() {
        let entry = LogEntry {
            timestamp: "".into(),
            thread: "".into(),
            class: "SysServiceImpl".into(),
            line: 962,
            message: "SysService handlerLoginCache device not register: 25012004594201"
                .into(),
            has_stack: false,
        };
        assert_eq!(
            categorize_entry(&entry),
            ErrorCategory::DeviceNotRegistered
        );
    }

    #[test]
    fn categorizes_connection_leak() {
        let entry = LogEntry {
            timestamp: "".into(),
            thread: "".into(),
            class: "MqttConnectionCache".into(),
            line: 173,
            message: "=== Clear leak clients end: 1 ===".into(),
            has_stack: false,
        };
        assert_eq!(categorize_entry(&entry), ErrorCategory::ConnectionLeak);
    }

    #[test]
    fn categorizes_dispatch_disabled() {
        let entry = LogEntry {
            timestamp: "".into(),
            thread: "".into(),
            class: "DispatchServiceImpl".into(),
            line: 675,
            message: "DispatchService dispatchThirdPartyDispatchRule disabled ruleId=1224488946380046337".into(),
            has_stack: false,
        };
        assert_eq!(categorize_entry(&entry), ErrorCategory::DispatchDisabled);
    }

    #[test]
    fn categorizes_protocol_transform_error() {
        let entry = LogEntry {
            timestamp: "".into(),
            thread: "".into(),
            class: "ComposeHandler".into(),
            line: 45,
            message: "ComposeHandler ERROR:".into(),
            has_stack: false,
        };
        assert_eq!(
            categorize_entry(&entry),
            ErrorCategory::ProtocolTransformError
        );
    }

    #[test]
    fn categorizes_other_for_unknown() {
        let entry = LogEntry {
            timestamp: "".into(),
            thread: "".into(),
            class: "UnknownClass".into(),
            line: 1,
            message: "some random message".into(),
            has_stack: false,
        };
        assert_eq!(categorize_entry(&entry), ErrorCategory::Other);
    }

    #[test]
    fn extracts_device_sns_from_message() {
        let sns = extract_device_sns("sn = 26062911750005, device not register: 25012004594201");
        assert_eq!(sns, vec!["26062911750005", "25012004594201"]);
    }

    #[test]
    fn extracts_dispatch_cost() {
        assert_eq!(
            extract_dispatch_cost(
                "[DISPATCH-END] thread=dispatch-executor-5 cost=64195ms"
            ),
            Some(64195)
        );
        assert_eq!(extract_dispatch_cost("no cost here"), None);
    }

    #[test]
    fn extracts_disabled_rule_id() {
        assert_eq!(
            extract_disabled_rule_id(
                "disabled ruleId=1224488946380046337"
            ),
            Some("1224488946380046337".to_string())
        );
    }
}
