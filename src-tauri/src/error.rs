use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    FileNotFound,
    FileTooLarge,
    UnsupportedFormat,
    NoExtractableText,
    ParseFailed,
    InvalidSettings,
    NetworkFailed,
    AuthenticationFailed,
    ContextTooLarge,
    EmptyAiResponse,
    InvalidAiCsv,
    SaveFailed,
    TaskActive,
    NoActiveTask,
    Cancelled,
    UpdateFailed,
    UpdateBlocked,
    InvalidPassthroughInput,
    InvalidPassthroughSource,
}

impl ErrorCode {
    fn safe_message(&self) -> &'static str {
        match self {
            Self::FileNotFound => "文件不存在。",
            Self::FileTooLarge => "文件超过 50 MB 限制。",
            Self::UnsupportedFormat => "不支持的文件格式。",
            Self::NoExtractableText => {
                "未提取到可用文本；若为 PDF，可能是扫描件或图片型 PDF，本期不支持 OCR。"
            }
            Self::ParseFailed => "文件解析失败。",
            Self::InvalidSettings => "设置无效。",
            Self::NetworkFailed => "网络请求失败。",
            Self::AuthenticationFailed => "认证失败。",
            Self::ContextTooLarge => "上下文过大。",
            Self::EmptyAiResponse => "AI 未返回有效内容。",
            Self::InvalidAiCsv => "AI 返回的 CSV 无效。",
            Self::SaveFailed => "保存文件失败。",
            Self::TaskActive => "已有任务正在执行。",
            Self::NoActiveTask => "当前没有活动任务。",
            Self::Cancelled => "任务已取消。",
            Self::UpdateFailed => "更新失败。",
            Self::UpdateBlocked => "当前有任务、更新或清理操作正在进行，请完成后重试。",
            Self::InvalidPassthroughInput => "透传报文输入无效。",
            Self::InvalidPassthroughSource => "透传解析资料无效。",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct AppError {
    code: ErrorCode,
    message: &'static str,
    detail: Option<String>,
    #[serde(skip)]
    internal_detail: Option<String>,
}

impl std::fmt::Debug for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppError")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("detail", &self.detail)
            .finish()
    }
}

impl AppError {
    pub fn new(code: ErrorCode) -> Self {
        let message = code.safe_message();

        Self {
            code,
            message,
            detail: None,
            internal_detail: None,
        }
    }

    pub(crate) fn legacy_doc() -> Self {
        Self {
            code: ErrorCode::UnsupportedFormat,
            message: "旧版 .doc 文件不受支持，请另存为 DOCX 后重试。",
            detail: None,
            internal_detail: None,
        }
    }

    pub(crate) fn ai_reasoning_exhausted() -> Self {
        Self {
            code: ErrorCode::EmptyAiResponse,
            message: "AI 的推理过程耗尽了最大输出 token，未生成 CSV。请增加“最大输出 token”或改用非推理模型（如 deepseek-chat）。",
            detail: None,
            internal_detail: None,
        }
    }

    pub(crate) fn ai_output_exhausted() -> Self {
        Self {
            code: ErrorCode::EmptyAiResponse,
            message: "AI 达到最大输出 token，但未生成 CSV。请增加“最大输出 token”后重试。",
            detail: None,
            internal_detail: None,
        }
    }

    #[allow(dead_code)] // Consumed by later command/logging modules.
    pub(crate) fn internal(code: ErrorCode, detail: impl Into<String>) -> Self {
        let mut error = Self::new(code);
        error.internal_detail = Some(detail.into());
        error
    }

    #[allow(dead_code)] // Consumed by later command/logging modules.
    pub(crate) fn internal_detail(&self) -> Option<&str> {
        self.internal_detail.as_deref()
    }
}

/// Removes configured secrets from diagnostic text before it reaches logs or a serialized error.
/// Empty configuration values are deliberately ignored so normal text remains unchanged.
pub fn redact_secrets(text: &str, secrets: &[&str]) -> String {
    secrets
        .iter()
        .filter(|secret| !secret.is_empty())
        .fold(text.to_owned(), |redacted, secret| {
            redacted.replace(secret, "[REDACTED]")
        })
}

#[cfg(test)]
mod tests {
    use super::{redact_secrets, AppError, ErrorCode};

    #[test]
    fn serializes_only_fixed_safe_fields_when_internal_detail_contains_secrets() {
        let secret = "hb-secret-123";
        let diagnostic = format!(
            "GET https://api.example.test/v1?api_key={secret}&model=x\nAuthorization: Bearer {secret}\nraw={secret}"
        );
        let error = AppError::internal(ErrorCode::NetworkFailed, diagnostic.clone());

        let serialized = serde_json::to_string(&error).expect("error should serialize");

        assert!(serialized.contains("\"code\":\"network_failed\""));
        assert!(serialized.contains("\"message\":\"网络请求失败。\""));
        assert!(serialized.contains("\"detail\":null"));
        assert!(!serialized.contains(secret));
        assert_eq!(error.internal_detail(), Some(diagnostic.as_str()));
    }

    #[test]
    fn serializes_a_message_that_is_determined_only_by_its_error_code() {
        let error = AppError::new(ErrorCode::Cancelled);

        let serialized = serde_json::to_string(&error).expect("error should serialize");

        assert_eq!(
            serialized,
            "{\"code\":\"cancelled\",\"message\":\"任务已取消。\",\"detail\":null}"
        );
    }

    #[test]
    fn redacts_url_query_bearer_headers_and_raw_configured_keys() {
        let secret = "hb-secret-123";
        let text = format!(
            "GET https://api.example.test/v1?api_key={secret}&model=x\nAuthorization: Bearer {secret}\nraw={secret}"
        );

        let redacted = redact_secrets(&text, &[secret]);

        assert!(!redacted.contains(secret));
        assert!(redacted.contains("api_key=[REDACTED]"));
        assert!(redacted.contains("Authorization: Bearer [REDACTED]"));
        assert!(redacted.contains("raw=[REDACTED]"));
    }

    #[test]
    fn ignores_empty_secrets_and_preserves_ordinary_text() {
        let text = "普通日志 https://api.example.test/v1?model=deepseek-chat";

        assert_eq!(redact_secrets(text, &[""]), text);
    }

    #[test]
    fn exposes_every_contract_error_code_in_snake_case() {
        let codes = [
            ErrorCode::FileNotFound,
            ErrorCode::FileTooLarge,
            ErrorCode::UnsupportedFormat,
            ErrorCode::NoExtractableText,
            ErrorCode::ParseFailed,
            ErrorCode::InvalidSettings,
            ErrorCode::NetworkFailed,
            ErrorCode::AuthenticationFailed,
            ErrorCode::ContextTooLarge,
            ErrorCode::EmptyAiResponse,
            ErrorCode::InvalidAiCsv,
            ErrorCode::SaveFailed,
            ErrorCode::TaskActive,
            ErrorCode::NoActiveTask,
            ErrorCode::Cancelled,
            ErrorCode::UpdateFailed,
            ErrorCode::UpdateBlocked,
            ErrorCode::InvalidPassthroughInput,
            ErrorCode::InvalidPassthroughSource,
        ];

        let serialized = serde_json::to_string(&codes).expect("codes should serialize");
        for code in [
            "file_not_found",
            "file_too_large",
            "unsupported_format",
            "no_extractable_text",
            "parse_failed",
            "invalid_settings",
            "network_failed",
            "authentication_failed",
            "context_too_large",
            "empty_ai_response",
            "invalid_ai_csv",
            "save_failed",
            "task_active",
            "no_active_task",
            "cancelled",
            "update_failed",
            "update_blocked",
            "invalid_passthrough_input",
            "invalid_passthrough_source",
        ] {
            assert!(serialized.contains(code), "missing {code}");
        }
    }
}
