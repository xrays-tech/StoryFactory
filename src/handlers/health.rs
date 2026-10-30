// src/handlers/health.rs — liveness / readiness（§5.7）
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Serialize;

use crate::state::AppState;

pub const APP_VERSION: &str = "4.0.0";
/// 导出格式版本（附录 C / D-03）；schema_version 运行时读取。
pub const FORMAT_VERSION: u64 = 2;

#[derive(Serialize)]
pub struct HealthBody {
    pub status: &'static str,
    pub app_version: &'static str,
    pub schema_version: u64,
    pub format_version: u64,
    pub db: &'static str,
    pub llm_configured: bool,
}

pub async fn healthz() -> Response {
    (StatusCode::OK, "ok").into_response()
}

pub async fn health(state: State<AppState>) -> Response {
    let db_ok = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();
    let fk_ok = sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
        .fetch_one(&state.pool)
        .await
        .map(|v| v == 1)
        .unwrap_or(false);
    let schema_version = crate::db::schema_version(&state.pool)
        .await
        .unwrap_or_default();

    // 缺失 API Key 不阻止启动（会话创建时才要求），但明确暴露 readiness 信号。
    let cfg = state.config.load();
    let llm_configured = cfg.llm.api_key.is_some() || cfg.llm.provider == "ollama";

    let healthy = db_ok && fk_ok && schema_version >= 1;
    let body = HealthBody {
        status: if healthy { "ok" } else { "degraded" },
        app_version: APP_VERSION,
        schema_version,
        format_version: FORMAT_VERSION,
        db: if db_ok && fk_ok { "ok" } else { "error" },
        llm_configured,
    };
    let status = if healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(body)).into_response()
}
