// src/llm/usage.rs — llm_usage 落库与成本计算（§7.6）
use sqlx::sqlite::SqlitePool;

use crate::llm::Purpose;

#[allow(clippy::too_many_arguments)]
pub async fn record(
    pool: &SqlitePool,
    session_id: Option<&str>,
    provider: &str,
    model: &str,
    purpose: Purpose,
    prompt_tokens: u32,
    completion_tokens: u32,
    cost_usd: f64,
    latency_ms: i64,
    success: bool,
    error_kind: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO llm_usage(session_id, provider, model, purpose, prompt_tokens, completion_tokens, cost_usd, latency_ms, success, error_kind) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(session_id)
    .bind(provider)
    .bind(model)
    .bind(purpose.as_str())
    .bind(prompt_tokens as i64)
    .bind(completion_tokens as i64)
    .bind(cost_usd)
    .bind(latency_ms)
    .bind(if success { 1i64 } else { 0i64 })
    .bind(error_kind)
    .execute(pool)
    .await?;
    Ok(())
}

/// 读取 app_config.llm_pricing：{"provider/model":{"input":0.27,"output":1.10,"currency_per":1e6}}
pub async fn price_for(pool: &SqlitePool, provider: &str, model: &str) -> Option<(f64, f64, f64)> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT value FROM app_config WHERE key = 'llm_pricing'")
            .fetch_optional(pool)
            .await
            .ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw?).ok()?;
    let entry = &value[format!("{provider}/{model}")];
    let input = entry["input"].as_f64()?;
    let output = entry["output"].as_f64()?;
    let per = entry["currency_per"].as_f64().unwrap_or(1_000_000.0);
    Some((input, output, per))
}

pub async fn cost_usd(
    pool: &SqlitePool,
    provider: &str,
    model: &str,
    prompt_tokens: u32,
    completion_tokens: u32,
) -> f64 {
    match price_for(pool, provider, model).await {
        Some((input, output, per)) => {
            (prompt_tokens as f64 / per) * input + (completion_tokens as f64 / per) * output
        }
        None => 0.0,
    }
}
