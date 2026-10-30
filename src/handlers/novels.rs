// src/handlers/novels.rs — /api/v1/novels 端点（§6.2 / D-09）
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::extract::{AppJson, AppQuery};
use crate::models::dto::{CreateNovelReq, UpdateNovelReq};
use crate::models::enums::NovelStatus;
use crate::services::novel;
use crate::state::AppState;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NovelListQ {
    pub q: Option<String>,
    pub status: Option<NovelStatus>,
    #[serde(default)]
    pub page: i64,
    #[serde(default)]
    pub page_size: i64,
}

pub async fn create_novel(
    state: State<AppState>,
    body: AppJson<CreateNovelReq>,
) -> Result<Response, AppError> {
    let d = novel::create(&state.pool, body.0).await?;
    Ok(crate::handlers::response::created(
        &format!("/api/v1/novels/{}", d.id),
        d,
    ))
}

pub async fn list_novels(
    state: State<AppState>,
    params: AppQuery<NovelListQ>,
) -> Result<Response, AppError> {
    let page = novel::list(
        &state.pool,
        params.q.as_deref(),
        params.status,
        params.page,
        params.page_size,
    )
    .await?;
    Ok((StatusCode::OK, Json(page)).into_response())
}

pub async fn get_novel(state: State<AppState>, ids: Path<String>) -> Result<Response, AppError> {
    let r = novel::get(&state.pool, &ids.0).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn patch_novel(
    state: State<AppState>,
    ids: Path<String>,
    body: AppJson<UpdateNovelReq>,
) -> Result<Response, AppError> {
    let r = novel::update(&state.pool, &ids.0, body.0).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn delete_novel(state: State<AppState>, ids: Path<String>) -> Result<Response, AppError> {
    novel::soft_delete(&state.pool, &ids.0).await?;
    Ok((StatusCode::NO_CONTENT, "").into_response())
}
