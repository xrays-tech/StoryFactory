// src/handlers/search.rs — GET /api/v1/search（D-22 护栏: |q|>=2）
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::extract::AppQuery;
use crate::services::search as search_svc;
use crate::state::AppState;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchQ {
    pub q: String,
    #[serde(rename = "type")]
    pub entity_type: Option<String>,
    pub novel_id: Option<String>,
    #[serde(default)]
    pub page: i64,
    #[serde(default)]
    pub page_size: i64,
}

pub async fn search_handler(
    state: State<AppState>,
    params: AppQuery<SearchQ>,
) -> Result<Response, AppError> {
    let min_len = state.config.load().fts_min_query_length;
    let r = search_svc::search(
        &state.pool,
        &params.q,
        params.entity_type.as_deref(),
        params.novel_id.as_deref(),
        params.page,
        params.page_size,
        min_len,
    )
    .await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}
