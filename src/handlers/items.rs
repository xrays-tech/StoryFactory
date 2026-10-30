// src/handlers/items.rs — /api/v1/novels/{id}/items 端点
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::extract::{AppJson, AppQuery};
use crate::models::dto::{CreateItemReq, UpdateItemReq};
use crate::services::item;
use crate::state::AppState;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PagingQ {
    #[serde(default)]
    pub page: i64,
    #[serde(default)]
    pub page_size: i64,
}

pub async fn create_item(
    state: State<AppState>,
    ids: Path<String>,
    body: AppJson<CreateItemReq>,
) -> Result<Response, AppError> {
    let d = item::create(&state.pool, &ids.0, body.0).await?;
    Ok(crate::handlers::response::created(
        &format!("/api/v1/novels/{}/items/{}", ids.0, d.id),
        d,
    ))
}

pub async fn list_items(
    state: State<AppState>,
    ids: Path<String>,
    params: AppQuery<PagingQ>,
) -> Result<Response, AppError> {
    let r = item::list(&state.pool, &ids.0, params.page, params.page_size).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn get_item(
    state: State<AppState>,
    ids: Path<(String, String)>,
) -> Result<Response, AppError> {
    let (nid, iid) = ids.0;
    let r = item::get(&state.pool, &nid, &iid).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn patch_item(
    state: State<AppState>,
    ids: Path<(String, String)>,
    body: AppJson<UpdateItemReq>,
) -> Result<Response, AppError> {
    let (nid, iid) = ids.0;
    let r = item::update(&state.pool, &nid, &iid, body.0).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn delete_item(
    state: State<AppState>,
    ids: Path<(String, String)>,
) -> Result<Response, AppError> {
    let (nid, iid) = ids.0;
    item::soft_delete(&state.pool, &nid, &iid).await?;
    Ok((StatusCode::NO_CONTENT, "").into_response())
}
