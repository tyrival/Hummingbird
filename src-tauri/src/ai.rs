use crate::{
    chunking::DocumentChunk,
    error::{AppError, ErrorCode},
    settings::Settings,
};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const TEMPERATURE: f32 = 0.1;
const MAX_RETRIES: usize = 2;

#[derive(Clone)]
pub struct AiClient {
    http: reqwest::Client,
    endpoint: Url,
    api_key: String,
    model: String,
    max_tokens: u32,
    retry_delays: [Duration; MAX_RETRIES],
}

impl AiClient {
    pub fn new(settings: &Settings) -> Result<Self, AppError> {
        settings.validate()?;
        let mut base_url = Url::parse(&settings.base_url)
            .map_err(|_| AppError::new(ErrorCode::InvalidSettings))?;
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }
        let endpoint = base_url
            .join("chat/completions")
            .map_err(|_| AppError::new(ErrorCode::InvalidSettings))?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(settings.timeout_seconds))
            .build()
            .map_err(|_| AppError::new(ErrorCode::InvalidSettings))?;

        Ok(Self {
            http,
            endpoint,
            api_key: settings.api_key.clone(),
            model: settings.model.clone(),
            max_tokens: settings.max_tokens,
            retry_delays: [Duration::from_millis(250), Duration::from_millis(500)],
        })
    }

    pub async fn extract_chunk(
        &self,
        system_prompt: &str,
        chunk: &DocumentChunk,
        position: usize,
        total: usize,
        cancellation: &CancellationToken,
    ) -> Result<String, AppError> {
        self.extract_chunk_with_instruction(
            system_prompt,
            chunk,
            position,
            total,
            None,
            cancellation,
        )
        .await
    }

    pub async fn extract_chunk_with_correction(
        &self,
        system_prompt: &str,
        chunk: &DocumentChunk,
        position: usize,
        total: usize,
        diagnostics: &str,
        cancellation: &CancellationToken,
    ) -> Result<String, AppError> {
        self.extract_chunk_with_instruction(
            system_prompt,
            chunk,
            position,
            total,
            Some(diagnostics),
            cancellation,
        )
        .await
    }

    async fn extract_chunk_with_instruction(
        &self,
        system_prompt: &str,
        chunk: &DocumentChunk,
        position: usize,
        total: usize,
        correction: Option<&str>,
        cancellation: &CancellationToken,
    ) -> Result<String, AppError> {
        let user_content = build_user_content(chunk, position, total, correction);

        for attempt in 0..=MAX_RETRIES {
            match self
                .request_once(system_prompt, &user_content, cancellation)
                .await
            {
                Ok(content) => return Ok(content),
                Err(AttemptError::Final(error)) => return Err(error),
                Err(AttemptError::Retryable(error)) if attempt == MAX_RETRIES => return Err(error),
                Err(AttemptError::Retryable(_)) => {
                    let delay = self.retry_delays[attempt];
                    tokio::select! {
                        _ = cancellation.cancelled() => {
                            return Err(AppError::new(ErrorCode::Cancelled));
                        }
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
            }
        }

        unreachable!("retry loop always returns")
    }

    async fn request_once(
        &self,
        system_prompt: &str,
        user_content: &str,
        cancellation: &CancellationToken,
    ) -> Result<String, AttemptError> {
        let request = ChatRequest {
            model: &self.model,
            messages: [
                ChatMessage {
                    role: "system",
                    content: system_prompt,
                },
                ChatMessage {
                    role: "user",
                    content: user_content,
                },
            ],
            temperature: TEMPERATURE,
            max_tokens: self.max_tokens,
        };
        let pending = self
            .http
            .post(self.endpoint.clone())
            .bearer_auth(&self.api_key)
            .json(&request)
            .send();
        let response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                return Err(AttemptError::Final(AppError::new(ErrorCode::Cancelled)));
            }
            response = pending => response.map_err(classify_transport_error)?,
        };
        let status = response.status();
        let body = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                return Err(AttemptError::Final(AppError::new(ErrorCode::Cancelled)));
            }
            body = response.text() => body.map_err(classify_transport_error)?,
        };

        if !status.is_success() {
            return Err(classify_http_error(status, &body));
        }

        let response: ChatResponse = serde_json::from_str(&body)
            .map_err(|_| AttemptError::Final(AppError::new(ErrorCode::EmptyAiResponse)))?;
        let usage = response.usage.unwrap_or_default();
        let choice = response.choices.into_iter().next();
        let content = choice
            .as_ref()
            .and_then(|value| value.message.content.as_deref())
            .unwrap_or_default();
        if content.trim().is_empty() {
            let reasoning_present = choice.as_ref().is_some_and(|value| {
                value
                    .message
                    .reasoning_content
                    .as_deref()
                    .is_some_and(|reasoning| !reasoning.trim().is_empty())
            });
            let length_finish = choice
                .as_ref()
                .and_then(|value| value.finish_reason.as_deref())
                .is_some_and(|reason| matches!(reason, "length" | "max_tokens"));
            let token_limit = u64::from(self.max_tokens);
            let completion_exhausted = usage
                .completion_tokens
                .or(usage.output_tokens)
                .is_some_and(|tokens| tokens >= token_limit);
            let reasoning_exhausted = usage
                .completion_tokens_details
                .as_ref()
                .or(usage.output_tokens_details.as_ref())
                .and_then(|details| details.reasoning_tokens)
                .is_some_and(|tokens| tokens >= token_limit);

            let error = if reasoning_present
                && (length_finish || completion_exhausted || reasoning_exhausted)
            {
                AppError::ai_reasoning_exhausted()
            } else if length_finish || completion_exhausted {
                AppError::ai_output_exhausted()
            } else {
                AppError::new(ErrorCode::EmptyAiResponse)
            };
            Err(AttemptError::Final(error))
        } else {
            Ok(content.trim().to_owned())
        }
    }

    #[cfg(test)]
    fn with_retry_delays(mut self, retry_delays: [Duration; MAX_RETRIES]) -> Self {
        self.retry_delays = retry_delays;
        self
    }
}

fn build_user_content(
    chunk: &DocumentChunk,
    position: usize,
    total: usize,
    correction: Option<&str>,
) -> String {
    let context = chunk.prior_context.as_deref().map_or_else(String::new, |value| {
        format!(
            "上一块末尾上下文（仅用于理解跨块表格，不要重复提取其中已完整出现的记录）：\n{value}\n\n"
        )
    });
    let correction = correction.map_or_else(String::new, |diagnostics| {
        format!(
            "上一次响应未通过 CSV 结构校验（{diagnostics}）。请重新提取当前块。每条记录必须严格包含 12 列；unit 为空也必须保留两个逗号。不要复述诊断，不要输出解释。\n\n"
        )
    });
    format!(
        "{correction}以下是设备说明书的第 {position}/{total} 块内容。请只提取本块出现的寄存器信息，按模板输出 CSV：\n\n{context}{}",
        chunk.text
    )
}

fn classify_transport_error(error: reqwest::Error) -> AttemptError {
    let detail = if error.is_timeout() {
        "request timed out"
    } else {
        "request transport failed"
    };
    AttemptError::Retryable(AppError::internal(ErrorCode::NetworkFailed, detail))
}

fn classify_http_error(status: StatusCode, body: &str) -> AttemptError {
    if matches!(
        status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::PAYMENT_REQUIRED
    ) {
        return AttemptError::Final(AppError::new(ErrorCode::AuthenticationFailed));
    }
    if is_context_overflow(status, body) {
        return AttemptError::Final(AppError::new(ErrorCode::ContextTooLarge));
    }
    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        return AttemptError::Retryable(AppError::internal(
            ErrorCode::NetworkFailed,
            format!("AI service returned HTTP {}", status.as_u16()),
        ));
    }
    AttemptError::Final(AppError::internal(
        ErrorCode::InvalidSettings,
        format!("AI service rejected request with HTTP {}", status.as_u16()),
    ))
}

fn is_context_overflow(status: StatusCode, body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    let explicit_marker = [
        "context_length_exceeded",
        "maximum context length",
        "context window",
        "context too long",
        "too many tokens",
        "token limit exceeded",
        "上下文过大",
        "上下文超限",
    ]
    .iter()
    .any(|marker| body.contains(marker));
    explicit_marker
        && matches!(
            status,
            StatusCode::BAD_REQUEST
                | StatusCode::PAYLOAD_TOO_LARGE
                | StatusCode::UNPROCESSABLE_ENTITY
        )
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    temperature: f32,
    max_tokens: u32,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<TokenUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    #[serde(default)]
    finish_reason: Option<String>,
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default, alias = "reasoning")]
    reasoning_content: Option<String>,
}

#[derive(Default, Deserialize)]
struct TokenUsage {
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens_details: Option<TokenDetails>,
    #[serde(default)]
    output_tokens_details: Option<TokenDetails>,
}

#[derive(Deserialize)]
struct TokenDetails {
    #[serde(default)]
    reasoning_tokens: Option<u64>,
}

enum AttemptError {
    Retryable(AppError),
    Final(AppError),
}

#[cfg(test)]
mod tests {
    use super::AiClient;
    use crate::{chunking::DocumentChunk, settings::Settings};
    use serde_json::json;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use wiremock::{
        matchers::{body_partial_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const CSV: &str = "id,group,data_name,unit,reg_add,reg_type,endian,dcm,k,fun_num,calc,style\n1,1,Ua,,100,6,1,1,1,3,,0";

    #[tokio::test]
    async fn resolves_chat_completions_and_sends_the_legacy_request_shape() {
        let server = MockServer::start().await;
        let expected_authorization = format!("Bearer {}", test_api_key());
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", expected_authorization.as_str()))
            .and(body_partial_json(json!({
                "model": "deepseek-chat",
                "temperature": 0.1,
                "max_tokens": 4096,
                "messages": [
                    {"role": "system", "content": "SYSTEM PROMPT"},
                    {"role": "user"}
                ]
            })))
            .respond_with(success(CSV))
            .expect(1)
            .mount(&server)
            .await;
        let client = client(&server, 5, 4096);
        let chunk = DocumentChunk {
            index: 1,
            text: "CURRENT CHUNK".into(),
            prior_context: Some("PREVIOUS TAIL".into()),
        };

        let response = client
            .extract_chunk("SYSTEM PROMPT", &chunk, 2, 3, &CancellationToken::new())
            .await
            .expect("request should succeed");

        assert_eq!(response, CSV);
        let request = server.received_requests().await.unwrap().remove(0);
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let user = body["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("第 2/3 块"));
        assert!(user.contains("上一块末尾上下文"));
        assert!(user.contains("不要重复提取"));
        assert!(user.contains("PREVIOUS TAIL"));
        assert!(user.ends_with("CURRENT CHUNK"));
    }

    #[tokio::test]
    async fn correction_request_contains_only_structural_diagnostics_and_not_the_prior_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(success(CSV))
            .expect(1)
            .mount(&server)
            .await;
        let client = client(&server, 5, 4096);
        let diagnostics = "有效记录 0；修复缺失 unit 0；拒绝记录 3；列数分布 11列=3";

        client
            .extract_chunk_with_correction(
                "SYSTEM PROMPT",
                &chunk("CURRENT CHUNK"),
                1,
                1,
                diagnostics,
                &CancellationToken::new(),
            )
            .await
            .expect("correction request should succeed");

        let request = server.received_requests().await.unwrap().remove(0);
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let user = body["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("上一次响应未通过 CSV 结构校验"));
        assert!(user.contains(diagnostics));
        assert!(user.contains("unit 为空也必须保留两个逗号"));
        assert!(user.ends_with("CURRENT CHUNK"));
        assert!(!user.contains("PRIOR RAW RESPONSE"));
    }

    #[tokio::test]
    async fn classifies_authentication_context_overflow_and_empty_responses_without_retrying() {
        for (status, body, expected) in [
            (
                401,
                json!({"error": {"message": "bad key"}}),
                "authentication_failed",
            ),
            (
                403,
                json!({"error": {"message": "forbidden"}}),
                "authentication_failed",
            ),
            (
                400,
                json!({"error": {"code": "context_length_exceeded"}}),
                "context_too_large",
            ),
            (
                200,
                json!({"choices": [{"message": {"content": ""}}]}),
                "empty_ai_response",
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .expect(1)
                .mount(&server)
                .await;
            let error = client(&server, 5, 4096)
                .extract_chunk("system", &chunk("text"), 1, 1, &CancellationToken::new())
                .await
                .expect_err("response should fail");
            assert_eq!(error_code(error), expected);
        }
    }

    #[tokio::test]
    async fn reports_reasoning_token_exhaustion_without_exposing_reasoning_content() {
        for body in [
            json!({
                "choices": [{
                    "finish_reason": "length",
                    "message": {
                        "content": null,
                        "reasoning_content": "private reasoning must not be exposed"
                    }
                }],
                "usage": {
                    "completion_tokens": 4096,
                    "completion_tokens_details": {"reasoning_tokens": 4096}
                }
            }),
            json!({
                "choices": [{
                    "finish_reason": "length",
                    "message": {
                        "content": "",
                        "reasoning": "compatible private reasoning field"
                    }
                }],
                "usage": {
                    "output_tokens": 4096,
                    "output_tokens_details": {"reasoning_tokens": 4096}
                }
            }),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .expect(1)
                .mount(&server)
                .await;

            let error = client(&server, 5, 4096)
                .extract_chunk("system", &chunk("text"), 1, 1, &CancellationToken::new())
                .await
                .expect_err("reasoning-only response should explain token exhaustion");
            let serialized = serde_json::to_value(error).unwrap();

            assert_eq!(serialized["code"], "empty_ai_response");
            let message = serialized["message"].as_str().unwrap();
            assert!(message.contains("推理"));
            assert!(message.contains("最大输出 token"));
            assert!(message.contains("非推理模型"));
            assert!(!serialized.to_string().contains("private reasoning"));
        }
    }

    #[tokio::test]
    async fn reports_non_reasoning_length_finish_as_output_token_exhaustion() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "finish_reason": "length",
                    "message": {"content": ""}
                }],
                "usage": {"completion_tokens": 4096}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let error = client(&server, 5, 4096)
            .extract_chunk("system", &chunk("text"), 1, 1, &CancellationToken::new())
            .await
            .expect_err("length finish without content should explain output exhaustion");
        let serialized = serde_json::to_value(error).unwrap();
        let message = serialized["message"].as_str().unwrap();

        assert_eq!(serialized["code"], "empty_ai_response");
        assert!(message.contains("达到最大输出 token"));
        assert!(!message.contains("推理消耗"));
    }

    #[tokio::test]
    async fn ordinary_empty_response_keeps_the_generic_safe_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": null}}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let error = client(&server, 5, 4096)
            .extract_chunk("system", &chunk("text"), 1, 1, &CancellationToken::new())
            .await
            .expect_err("ordinary empty response should remain a stable error");
        let serialized = serde_json::to_value(error).unwrap();

        assert_eq!(serialized["code"], "empty_ai_response");
        assert_eq!(serialized["message"], "AI 未返回有效内容。");
        assert_eq!(serialized["detail"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn retries_429_and_5xx_at_most_twice_but_not_invalid_requests() {
        for status in [429, 500, 503] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status))
                .expect(3)
                .mount(&server)
                .await;
            let error = fast_client(&server, 5)
                .extract_chunk("system", &chunk("text"), 1, 1, &CancellationToken::new())
                .await
                .expect_err("transient response should exhaust retries");
            assert_eq!(error_code(error), "network_failed");
        }

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_string("invalid model"))
            .expect(1)
            .mount(&server)
            .await;
        let error = fast_client(&server, 5)
            .extract_chunk("system", &chunk("text"), 1, 1, &CancellationToken::new())
            .await
            .expect_err("invalid request should not retry");
        assert_eq!(error_code(error), "invalid_settings");
    }

    #[tokio::test]
    async fn classifies_timeout_after_bounded_retries_without_leaking_the_api_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(success(CSV).set_delay(Duration::from_secs(2)))
            .expect(3)
            .mount(&server)
            .await;
        let error = fast_client(&server, 1)
            .extract_chunk("system", &chunk("text"), 1, 1, &CancellationToken::new())
            .await
            .expect_err("request should time out");

        assert_eq!(error_code(error.clone()), "network_failed");
        assert!(!format!("{error:?}").contains("hb-secret"));
        assert!(!serde_json::to_string(&error).unwrap().contains("hb-secret"));
    }

    #[tokio::test]
    async fn cancellation_interrupts_an_in_flight_request_and_retry_backoff() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(success(CSV).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let cancellation = CancellationToken::new();
        let cancel = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            cancel.cancel();
        });
        let error = client(&server, 10, 4096)
            .extract_chunk("system", &chunk("text"), 1, 1, &cancellation)
            .await
            .expect_err("request should be cancelled");
        assert_eq!(error_code(error), "cancelled");

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let cancellation = CancellationToken::new();
        let cancel = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            cancel.cancel();
        });
        let error = client(&server, 10, 4096)
            .extract_chunk("system", &chunk("text"), 1, 1, &cancellation)
            .await
            .expect_err("backoff should be cancellable");
        assert_eq!(error_code(error), "cancelled");
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    fn client(server: &MockServer, timeout_seconds: u64, max_tokens: u32) -> AiClient {
        let settings = Settings {
            base_url: format!("{}/v1", server.uri()),
            api_key: test_api_key(),
            timeout_seconds,
            max_tokens,
            ..Settings::default()
        };
        AiClient::new(&settings).unwrap()
    }

    fn test_api_key() -> String {
        ["hb", "-secret"].concat()
    }

    fn fast_client(server: &MockServer, timeout_seconds: u64) -> AiClient {
        client(server, timeout_seconds, 4096)
            .with_retry_delays([Duration::from_millis(1), Duration::from_millis(1)])
    }

    fn chunk(text: &str) -> DocumentChunk {
        DocumentChunk {
            index: 0,
            text: text.into(),
            prior_context: None,
        }
    }

    fn success(content: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": content}}]
        }))
    }

    fn error_code(error: crate::error::AppError) -> String {
        serde_json::to_value(error).unwrap()["code"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}
