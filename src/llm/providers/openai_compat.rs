// src/llm/providers/openai_compat.rs — OpenAI 兼容 / DeepSeek Provider（§7.1 / 7.3 / 7.4）
use std::time::{Duration, Instant};

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{json, Value};

use crate::llm::{
    client::build_http_client, CompletionReq, CompletionResp, LlmError, LlmErrorKind, LlmProvider,
    ResponseFormat, StructuredMode,
};

pub struct OpenAiCompatProvider {
    name: String,
    base_url: String,
    model: String,
    api_key: SecretString,
    mode: StructuredMode,
    client: reqwest::Client,
}

impl OpenAiCompatProvider {
    pub fn deepseek(
        base_url: impl Into<String>,
        model: impl Into<String>,
        key: SecretString,
    ) -> Self {
        Self::new("deepseek", base_url, model, key, StructuredMode::JsonObject)
    }

    pub fn openai(
        base_url: impl Into<String>,
        model: impl Into<String>,
        key: SecretString,
    ) -> Self {
        Self::new("openai", base_url, model, key, StructuredMode::JsonSchema)
    }

    pub fn new(
        name: impl Into<String>,
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: SecretString,
        mode: StructuredMode,
    ) -> Self {
        Self {
            name: name.into(),
            base_url: base_url.into(),
            model: model.into(),
            api_key,
            mode,
            client: build_http_client(),
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn map_transport_error(e: reqwest::Error) -> LlmError {
    if e.is_timeout() {
        LlmError::new(LlmErrorKind::Timeout, e.to_string())
    } else {
        LlmError::new(LlmErrorKind::Network, e.to_string())
    }
}

/// 纯构造函数：把 `CompletionReq` 映射为 OpenAI 兼容请求体，便于序列化测试。
fn build_body(model: &str, req: &CompletionReq) -> Value {
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| json!({ "role": m.role.as_str(), "content": m.content }))
        .collect();

    let mut body = json!({
        "model": model,
        "messages": messages,
        "max_tokens": req.max_tokens,
        "temperature": req.temperature,
    });
    match &req.response_format {
        Some(ResponseFormat::JsonObject) => {
            body["response_format"] = json!({ "type": "json_object" });
        }
        Some(ResponseFormat::JsonSchema { name, schema }) => {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": { "name": name, "schema": schema }
            });
        }
        None => {}
    }
    // 仅显式设置时透传；供应商不支持时由 HTTP 400 映射为 BadRequest，不静默吞掉。
    if let Some(effort) = req.reasoning_effort {
        body["reasoning_effort"] = json!(effort);
    }
    if req.disable_thinking {
        body["thinking"] = json!({ "type": "disabled" });
    }
    body
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn structured_mode(&self) -> StructuredMode {
        self.mode
    }

    async fn complete(&self, req: CompletionReq) -> Result<CompletionResp, LlmError> {
        let mut body = build_body(&self.model, &req);

        let deadline: Instant = req.deadline;
        let endpoint = self.endpoint();
        let api_key = self.api_key.expose_secret().to_string();
        let client = self.client.clone();
        let mut allow_downgrade =
            matches!(req.response_format, Some(ResponseFormat::JsonSchema { .. }));

        let fut = async move {
            let response = loop {
                let response = client
                    .post(&endpoint)
                    .bearer_auth(&api_key)
                    .header("x-request-id", uuid::Uuid::new_v4().to_string())
                    .json(&body)
                    .send()
                    .await
                    .map_err(map_transport_error)?;
                if response.status().as_u16() == 400 && allow_downgrade {
                    // M2：兼容端点不支持 json_schema 时降级为 json_object 重试一次。
                    allow_downgrade = false;
                    body["response_format"] = json!({ "type": "json_object" });
                    continue;
                }
                break response;
            };
            let status = response.status();
            if !status.is_success() {
                let retry_after = parse_retry_after(response.headers());
                let text = response.text().await.unwrap_or_default();
                let message: String = text.chars().take(500).collect();
                let kind = match status.as_u16() {
                    401 | 403 => LlmErrorKind::Auth,
                    429 => LlmErrorKind::RateLimited,
                    400 | 422 => LlmErrorKind::BadRequest,
                    500..=599 => LlmErrorKind::Server,
                    _ => LlmErrorKind::BadRequest,
                };
                return Err(
                    LlmError::new(kind, format!("HTTP {}: {message}", status.as_u16()))
                        .with_retry_after(retry_after),
                );
            }
            let value: Value = response.json().await.map_err(map_transport_error)?;
            let choice = &value["choices"][0];
            let finish_reason = choice["finish_reason"].as_str().unwrap_or("");
            let content = match choice["message"]["content"].as_str() {
                Some(text) if finish_reason == "length" => {
                    tracing::warn!(
                        completion_tokens = value["usage"]["completion_tokens"].as_u64().unwrap_or(0),
                        reasoning_tokens = value["usage"]["completion_tokens_details"]["reasoning_tokens"].as_u64().unwrap_or(0),
                        content_chars = text.chars().count(),
                        content_head = %text.chars().take(120).collect::<String>(),
                        "输出被截断，记录用量与开头"
                    );
                    return Err(LlmError::new(
                        LlmErrorKind::Truncated,
                        "输出在 max_tokens 处被截断（推理模型的 reasoning tokens 也计入该预算）",
                    ));
                }
                Some(text) => text,
                None => {
                    if finish_reason == "length" {
                        tracing::warn!(
                            completion_tokens =
                                value["usage"]["completion_tokens"].as_u64().unwrap_or(0),
                            reasoning_tokens = value["usage"]["completion_tokens_details"]
                                ["reasoning_tokens"]
                                .as_u64()
                                .unwrap_or(0),
                            "输出被截断且无正文，预算全部被推理占用"
                        );
                        return Err(LlmError::new(
                            LlmErrorKind::Truncated,
                            "输出在 max_tokens 处被截断，未返回正文内容",
                        ));
                    }
                    return Err(LlmError::new(
                        LlmErrorKind::InvalidOutput,
                        "响应缺少 choices[0].message.content",
                    ));
                }
            };
            let prompt_tokens = value["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32;
            let completion_tokens =
                value["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32;
            Ok(CompletionResp {
                content: content.to_string(),
                prompt_tokens,
                completion_tokens,
            })
        };

        tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), fut)
            .await
            .map_err(|_| LlmError::new(LlmErrorKind::Timeout, "单次尝试超出预算"))?
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::build_body;
    use crate::llm::{CompletionReq, Message, Purpose};

    fn req() -> CompletionReq {
        CompletionReq::new(
            vec![Message::user("写一段正文")],
            Instant::now() + Duration::from_secs(30),
            Purpose::Prose,
        )
    }

    #[test]
    fn openai_compat_body_includes_reasoning_effort_when_set() {
        let body = build_body("mock-model", &req().with_reasoning_effort("minimal"));
        assert_eq!(body["reasoning_effort"], "minimal");
        assert_eq!(body["max_tokens"], 1500);
    }

    #[test]
    fn openai_compat_body_omits_reasoning_effort_by_default() {
        let body = build_body("mock-model", &req());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn openai_compat_body_disables_thinking_when_requested() {
        let mut request = req();
        request.disable_thinking = true;
        let body = build_body("mock-model", &request);
        assert_eq!(body["thinking"], serde_json::json!({ "type": "disabled" }));

        let plain = build_body("mock-model", &req());
        assert!(plain.get("thinking").is_none());
    }
}
