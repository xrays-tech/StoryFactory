// src/llm/structured.rs — 结构化输出解析 + 本地 Schema 校验 + 修复回路（§7.4）
use std::time::Instant;

use jsonschema::validator_for;
use serde_json::Value;

use crate::llm::{
    CompletionReq, LlmError, LlmErrorKind, LlmProvider, Message, Purpose, ResponseFormat,
    StructuredMode,
};

pub fn extract_json(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    let without_fence = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let without_fence = without_fence
        .strip_suffix("```")
        .unwrap_or(without_fence)
        .trim();
    serde_json::from_str::<Value>(without_fence).ok()
}

fn apply_response_format(provider: &dyn LlmProvider, req: &mut CompletionReq, schema: &Value) {
    match provider.structured_mode() {
        StructuredMode::JsonSchema => {
            req.response_format = Some(ResponseFormat::JsonSchema {
                name: "output".to_string(),
                schema: schema.clone(),
            });
        }
        StructuredMode::JsonObject => {
            req.response_format = Some(ResponseFormat::JsonObject);
            let summary = crate::llm::schemas::compact_schema(schema);
            req.messages.push(Message::system(format!(
                "输出必须严格满足以下 JSON Schema（只输出 JSON 对象）：\n{summary}"
            )));
        }
    }
}

/// 调用 Provider，解析 JSON 并校验；失败时最多回喂修复 max_repairs 轮。
pub async fn complete_json(
    provider: &dyn LlmProvider,
    mut req: CompletionReq,
    schema: Value,
    max_repairs: u32,
) -> Result<Value, LlmError> {
    apply_response_format(provider, &mut req, &schema);
    let validator = validator_for(&schema)
        .map_err(|e| LlmError::new(LlmErrorKind::InvalidOutput, format!("Schema 编译失败: {e}")))?;

    let mut last_error = String::from("未产生输出");
    for attempt in 0..=max_repairs {
        if Instant::now() >= req.deadline {
            return Err(LlmError::new(LlmErrorKind::Timeout, "结构化输出预算耗尽"));
        }
        let resp = provider.complete(req.clone()).await?;
        match extract_json(&resp.content) {
            Some(value) => match validator.validate(&value) {
                Ok(()) => return Ok(value),
                Err(e) => last_error = format!("Schema 校验失败: {e}"),
            },
            None => last_error = "输出不是合法 JSON 对象".to_string(),
        }
        if attempt == max_repairs {
            break;
        }
        req.messages.push(Message::assistant(resp.content));
        req.messages.push(Message::user(format!(
            "上一次输出不符合要求：{last_error}\n请只返回修正后的 JSON 对象，不要任何解释或 Markdown。"
        )));
        req.purpose = Purpose::Repair;
        req.temperature = 0.2;
    }
    Err(LlmError::new(LlmErrorKind::InvalidOutput, last_error))
}
