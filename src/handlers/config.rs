// src/handlers/config.rs — 运行期配置读写与成本报表（§6.2 / §7.6）
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::error::AppError;
use crate::extract::{AppJson, AppQuery};
use crate::state::AppState;

pub async fn get_config(state: State<AppState>) -> Response {
    let cfg = state.config.load();
    Json(cfg.as_ref().clone()).into_response()
}

pub async fn patch_config(
    state: State<AppState>,
    body: AppJson<crate::config::RuntimePatch>,
) -> Result<Response, AppError> {
    let patch = body.0;
    let mut cfg = state.config.load().as_ref().clone();
    patch.apply(&mut cfg).map_err(|msg| {
        AppError::validation("配置校验失败", json!([{"field":"config","message":msg}]))
    })?;
    crate::config::persist_runtime_patch(&state.pool, &patch)
        .await
        .map_err(crate::services::util::db_err)?;
    state.config.store(Arc::new(cfg.clone()));
    Ok(Json(cfg).into_response())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageQ {
    pub month: Option<String>,
}

pub async fn get_usage(
    state: State<AppState>,
    query: AppQuery<UsageQ>,
) -> Result<Response, AppError> {
    let month = query.month.clone().unwrap_or_else(current_month);
    let by_day: Vec<(String, f64, i64)> = sqlx::query_as(
        "SELECT strftime('%Y-%m-%d', created_at, 'unixepoch') AS day,
                CAST(COALESCE(SUM(cost_usd),0) AS REAL), COUNT(*)
         FROM llm_usage
         WHERE strftime('%Y-%m', created_at, 'unixepoch') = ?
         GROUP BY day ORDER BY day",
    )
    .bind(&month)
    .fetch_all(&state.pool)
    .await
    .map_err(crate::services::util::db_err)?;
    let by_purpose: Vec<(String, i64)> = sqlx::query_as(
        "SELECT purpose, COUNT(*) FROM llm_usage
         WHERE strftime('%Y-%m', created_at, 'unixepoch') = ?
         GROUP BY purpose ORDER BY COUNT(*) DESC",
    )
    .bind(&month)
    .fetch_all(&state.pool)
    .await
    .map_err(crate::services::util::db_err)?;
    let total_cost: f64 = by_day.iter().map(|(_, cost, _)| *cost).sum();
    let total_calls: i64 = by_day.iter().map(|(_, _, calls)| *calls).sum();
    Ok(Json(json!({
        "month": month,
        "total_cost_usd": total_cost,
        "total_calls": total_calls,
        "by_day": by_day.into_iter().map(|(date, cost, calls)| json!({"date":date,"cost_usd":cost,"calls":calls})).collect::<Vec<_>>(),
        "by_purpose": by_purpose.into_iter().map(|(purpose, calls)| json!({"purpose":purpose,"calls":calls})).collect::<Vec<_>>(),
    }))
    .into_response())
}

fn current_month() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // 粗略 UTC 月份（报表口径允许）
    let days = now / 86400;
    let (y, m, _) = civil_from_days(days);
    format!("{y:04}-{m:02}")
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

pub async fn client_error(body: Bytes) -> Response {
    if body.len() > 32 * 1024 {
        return AppError::too_large().into_response();
    }
    let text = String::from_utf8_lossy(&body);
    tracing::warn!(payload = %text.chars().take(2000).collect::<String>(), "client error report");
    axum::http::StatusCode::NO_CONTENT.into_response()
}
