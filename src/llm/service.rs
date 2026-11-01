// src/llm/service.rs — 链 + 结构化 + usage 的统一编排（P5 入口）
use std::time::Instant;

use jsonschema::validator_for;
use serde_json::Value;
use sqlx::SqlitePool;

use crate::llm::budget::Budget;
use crate::llm::failover::{AttemptRecord, ProviderChain};
use crate::llm::structured::extract_json;
use crate::llm::usage;
use crate::llm::{CompletionReq, LlmError, LlmErrorKind, Message, Purpose, ResponseFormat};

pub struct LlmService {
    pub chain: ProviderChain,
    pub pool: SqlitePool,
    pub session_id: Option<String>,
}

/// 结构化输出的默认 max_tokens（沿用 v4.0 行为）。
pub const DEFAULT_STRUCTURED_MAX_TOKENS: u32 = 1500;

/// 截断后的一次性预算升级：至少不变，最多 16k。
fn escalated_tokens(requested: u32) -> u32 {
    requested.saturating_mul(4).min(16_000).max(requested)
}

impl LlmService {
    pub fn new(chain: ProviderChain, pool: SqlitePool, session_id: Option<String>) -> Self {
        Self {
            chain,
            pool,
            session_id,
        }
    }

    pub async fn complete_structured(
        &self,
        budget: &Budget,
        purpose: Purpose,
        messages: Vec<Message>,
        schema: Value,
        max_repairs: u32,
    ) -> Result<(Value, Vec<AttemptRecord>), LlmError> {
        self.complete_structured_with_tokens(
            budget,
            purpose,
            messages,
            schema,
            max_repairs,
            DEFAULT_STRUCTURED_MAX_TOKENS,
        )
        .await
    }

    /// 结构化输出，可指定单次响应的 `max_tokens`。
    ///
    /// 推理模型（DeepSeek-V4.1 等）的 reasoning tokens 与正文共享这一预算，
    /// 长输任务必须显式放大，否则会以 `finish_reason = length` 截断。
    pub async fn complete_structured_with_tokens(
        &self,
        budget: &Budget,
        purpose: Purpose,
        messages: Vec<Message>,
        schema: Value,
        max_repairs: u32,
        max_tokens: u32,
    ) -> Result<(Value, Vec<AttemptRecord>), LlmError> {
        let mut req = CompletionReq::new(messages, budget.hard_deadline(), purpose);
        req.max_tokens = max_tokens;
        req.response_format = Some(ResponseFormat::JsonObject);
        req.messages.push(Message::system(format!(
            "输出必须严格满足以下 JSON Schema（只输出 JSON 对象）：\n{}",
            crate::llm::schemas::compact_schema(&schema)
        )));

        let validator = validator_for(&schema).map_err(|e| {
            LlmError::new(LlmErrorKind::InvalidOutput, format!("Schema 编译失败: {e}"))
        })?;
        let mut collected: Vec<AttemptRecord> = Vec::new();
        let mut last_error: String;
        let mut repairs = 0u32;
        let mut escalated = false;

        loop {
            if Instant::now() >= req.deadline {
                return Err(LlmError::new(LlmErrorKind::Timeout, "结构化输出预算耗尽"));
            }
            match self.chain.complete_tracked(req.clone()).await {
                Ok(outcome) => {
                    let content = outcome.resp.content.clone();
                    self.record_attempts(&outcome.attempts).await;
                    collected.extend(outcome.attempts);
                    match extract_json(&content) {
                        Some(value) => match validator.validate(&value) {
                            Ok(()) => return Ok((value, collected)),
                            Err(e) => last_error = format!("Schema 校验失败: {e}"),
                        },
                        None => last_error = "输出不是合法 JSON 对象".to_string(),
                    }
                    if repairs == max_repairs {
                        break;
                    }
                    req.messages.push(Message::assistant(content));
                    req.messages.push(Message::user(format!(
                        "上一次输出不符合要求：{last_error}\n请只返回修正后的 JSON 对象，不要任何解释或 Markdown。"
                    )));
                    req.purpose = Purpose::Repair;
                    req.temperature = 0.2;
                    repairs += 1;
                }
                Err((err, attempts)) => {
                    self.record_attempts(&attempts).await;
                    collected.extend(attempts);
                    // 首次截断由 service 升级预算重试，不占用 repair 配额；
                    // 升级后仍截断则交回上层（如 novel_import 的二分逻辑）。
                    if err.kind == LlmErrorKind::Truncated && !escalated && repairs == 0 {
                        escalated = true;
                        let requested = req.max_tokens;
                        let next = escalated_tokens(requested);
                        tracing::info!(
                            requested,
                            escalated = next,
                            "结构化输出被截断，提升 max_tokens 后重试一次"
                        );
                        req.max_tokens = next;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
        Err(LlmError::new(LlmErrorKind::InvalidOutput, last_error))
    }

    pub async fn complete_text(
        &self,
        budget: &Budget,
        purpose: Purpose,
        messages: Vec<Message>,
        max_tokens: u32,
    ) -> Result<String, LlmError> {
        self.complete_text_with(budget, purpose, messages, max_tokens, None)
            .await
    }

    /// 文本输出，可指定推理强度；`None` 表示与 `complete_text` 行为一致。
    pub async fn complete_text_with(
        &self,
        budget: &Budget,
        purpose: Purpose,
        messages: Vec<Message>,
        max_tokens: u32,
        reasoning_effort: Option<&'static str>,
    ) -> Result<String, LlmError> {
        let mut req = CompletionReq::new(messages, budget.hard_deadline(), purpose);
        req.max_tokens = max_tokens;
        req.temperature = 0.4;
        req.reasoning_effort = reasoning_effort;
        match self.chain.complete_tracked(req.clone()).await {
            Ok(outcome) => {
                self.record_attempts(&outcome.attempts).await;
                Ok(outcome.resp.content)
            }
            Err((err, attempts)) => {
                self.record_attempts(&attempts).await;
                if err.kind != LlmErrorKind::Truncated {
                    return Err(err);
                }
                let requested = req.max_tokens;
                let escalated = escalated_tokens(requested);
                tracing::info!(
                    requested,
                    escalated,
                    "文本输出被截断，提升 max_tokens 后重试一次"
                );
                req.max_tokens = escalated;
                match self.chain.complete_tracked(req).await {
                    Ok(outcome) => {
                        self.record_attempts(&outcome.attempts).await;
                        Ok(outcome.resp.content)
                    }
                    Err((err, attempts)) => {
                        self.record_attempts(&attempts).await;
                        Err(err)
                    }
                }
            }
        }
    }

    async fn record_attempts(&self, attempts: &[AttemptRecord]) {
        for attempt in attempts {
            let cost = if attempt.success {
                usage::cost_usd(
                    &self.pool,
                    &attempt.provider,
                    &attempt.model,
                    attempt.prompt_tokens,
                    attempt.completion_tokens,
                )
                .await
            } else {
                0.0
            };
            if let Err(e) = usage::record(
                &self.pool,
                self.session_id.as_deref(),
                &attempt.provider,
                &attempt.model,
                attempt.purpose,
                attempt.prompt_tokens,
                attempt.completion_tokens,
                cost,
                attempt.latency_ms as i64,
                attempt.success,
                attempt.error_kind,
            )
            .await
            {
                tracing::warn!(error = %e, "llm_usage 写入失败");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::escalated_tokens;

    #[test]
    fn escalated_tokens_is_bounded() {
        assert_eq!(escalated_tokens(0), 0);
        assert_eq!(escalated_tokens(500), 2_000);
        assert_eq!(escalated_tokens(1_800), 7_200);
        assert_eq!(escalated_tokens(4_000), 16_000);
        assert_eq!(escalated_tokens(16_000), 16_000);
    }
}
