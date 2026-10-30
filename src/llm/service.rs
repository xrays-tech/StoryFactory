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
        let mut req = CompletionReq::new(messages, budget.hard_deadline(), purpose);
        req.response_format = Some(ResponseFormat::JsonObject);
        req.messages.push(Message::system(format!(
            "输出必须严格满足以下 JSON Schema（只输出 JSON 对象）：\n{}",
            crate::llm::schemas::compact_schema(&schema)
        )));

        let validator = validator_for(&schema).map_err(|e| {
            LlmError::new(LlmErrorKind::InvalidOutput, format!("Schema 编译失败: {e}"))
        })?;
        let mut collected: Vec<AttemptRecord> = Vec::new();
        let mut last_error = String::from("未产生输出");

        for attempt in 0..=max_repairs {
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
                    if attempt == max_repairs {
                        break;
                    }
                    req.messages.push(Message::assistant(content));
                    req.messages.push(Message::user(format!(
                        "上一次输出不符合要求：{last_error}\n请只返回修正后的 JSON 对象，不要任何解释或 Markdown。"
                    )));
                    req.purpose = Purpose::Repair;
                    req.temperature = 0.2;
                }
                Err((err, attempts)) => {
                    self.record_attempts(&attempts).await;
                    collected.extend(attempts);
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
        let mut req = CompletionReq::new(messages, budget.hard_deadline(), purpose);
        req.max_tokens = max_tokens;
        req.temperature = 0.4;
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
