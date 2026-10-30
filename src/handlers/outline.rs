// src/handlers/outline::rs — volumes/chapters 端点（§6.2）
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::extract::{AppJson, AppQuery};
use crate::models::dto::{
    CreateChapterReq, CreateVolumeReq, MoveReq, UpdateChapterReq, UpdateVolumeReq,
};
use crate::services::outline;
use crate::state::AppState;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PagingQ {
    #[serde(default)]
    pub page: i64,
    #[serde(default)]
    pub page_size: i64,
}

pub async fn list_volumes(state: State<AppState>, ids: Path<String>) -> Result<Response, AppError> {
    let r = outline::list_volumes(&state.pool, &ids.0).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn create_volume(
    state: State<AppState>,
    ids: Path<String>,
    body: AppJson<CreateVolumeReq>,
) -> Result<Response, AppError> {
    let d = outline::create_volume(&state.pool, &ids.0, body.0).await?;
    Ok(crate::handlers::response::created(
        &format!("/api/v1/novels/{}/volumes/{}", ids.0, d.id),
        d,
    ))
}

pub async fn get_volume(
    state: State<AppState>,
    ids: Path<(String, String)>,
) -> Result<Response, AppError> {
    let (nid, vid) = ids.0;
    let r = outline::get_volume(&state.pool, &nid, &vid).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn patch_volume(
    state: State<AppState>,
    ids: Path<(String, String)>,
    body: AppJson<UpdateVolumeReq>,
) -> Result<Response, AppError> {
    let (nid, vid) = ids.0;
    let r = outline::update_volume(&state.pool, &nid, &vid, body.0).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn move_volume(
    state: State<AppState>,
    ids: Path<(String, String)>,
    body: AppJson<MoveReq>,
) -> Result<Response, AppError> {
    let (nid, vid) = ids.0;
    let r = outline::move_volume(&state.pool, &nid, &vid, body.0).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn delete_volume(
    state: State<AppState>,
    ids: Path<(String, String)>,
) -> Result<Response, AppError> {
    let (nid, vid) = ids.0;
    outline::soft_delete_volume(&state.pool, &nid, &vid).await?;
    Ok((StatusCode::NO_CONTENT, "").into_response())
}

pub async fn list_chapters(
    state: State<AppState>,
    ids: Path<(String, String)>,
    params: AppQuery<PagingQ>,
) -> Result<Response, AppError> {
    let (nid, vid) = ids.0;
    let r = outline::list_chapters(&state.pool, &nid, &vid, params.page, params.page_size).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn create_chapter(
    state: State<AppState>,
    ids: Path<(String, String)>,
    body: AppJson<CreateChapterReq>,
) -> Result<Response, AppError> {
    let (nid, vid) = ids.0;
    let d = outline::create_chapter(&state.pool, &nid, &vid, body.0).await?;
    Ok(crate::handlers::response::created(
        &format!("/api/v1/novels/{nid}/chapters/{}", d.id),
        d,
    ))
}

pub async fn get_chapter(
    state: State<AppState>,
    ids: Path<(String, String)>,
) -> Result<Response, AppError> {
    let (nid, cid) = ids.0;
    let r = outline::get_chapter(&state.pool, &nid, &cid).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn patch_chapter(
    state: State<AppState>,
    ids: Path<(String, String)>,
    body: AppJson<UpdateChapterReq>,
) -> Result<Response, AppError> {
    let (nid, cid) = ids.0;
    let r = outline::update_chapter(&state.pool, &nid, &cid, body.0).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn delete_chapter(
    state: State<AppState>,
    ids: Path<(String, String)>,
) -> Result<Response, AppError> {
    let (nid, cid) = ids.0;
    outline::soft_delete_chapter(&state.pool, &nid, &cid).await?;
    Ok((StatusCode::NO_CONTENT, "").into_response())
}
