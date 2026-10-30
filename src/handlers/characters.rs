// src/handlers/characters.rs — /api/v1/novels/{id}/characters + /relations（D-10）
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::extract::{AppJson, AppQuery};
use crate::models::dto::{CreateCharacterReq, PutRelationsReq, UpdateCharacterReq};
use crate::services::character;
use crate::state::AppState;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PagingQ {
    #[serde(default)]
    pub page: i64,
    #[serde(default)]
    pub page_size: i64,
}

pub async fn create_character(
    state: State<AppState>,
    ids: Path<String>,
    body: AppJson<CreateCharacterReq>,
) -> Result<Response, AppError> {
    let d = character::create(&state.pool, &ids.0, body.0).await?;
    Ok(crate::handlers::response::created(
        &format!("/api/v1/novels/{}/characters/{}", ids.0, d.id),
        d,
    ))
}

pub async fn list_characters(
    state: State<AppState>,
    ids: Path<String>,
    params: AppQuery<PagingQ>,
) -> Result<Response, AppError> {
    let page = character::list(&state.pool, &ids.0, params.page, params.page_size).await?;
    Ok((StatusCode::OK, Json(page)).into_response())
}

pub async fn get_character(
    state: State<AppState>,
    ids: Path<(String, String)>,
) -> Result<Response, AppError> {
    let (nid, cid) = ids.0;
    let r = character::get(&state.pool, &nid, &cid).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn patch_character(
    state: State<AppState>,
    ids: Path<(String, String)>,
    body: AppJson<UpdateCharacterReq>,
) -> Result<Response, AppError> {
    let (nid, cid) = ids.0;
    let r = character::update(&state.pool, &nid, &cid, body.0).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn delete_character(
    state: State<AppState>,
    ids: Path<(String, String)>,
) -> Result<Response, AppError> {
    let (nid, cid) = ids.0;
    character::soft_delete(&state.pool, &nid, &cid).await?;
    Ok((StatusCode::NO_CONTENT, "").into_response())
}

pub async fn get_relations(
    state: State<AppState>,
    ids: Path<String>,
) -> Result<Response, AppError> {
    let r = character::get_relations(&state.pool, &ids.0).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn put_relations(
    state: State<AppState>,
    ids: Path<String>,
    body: AppJson<PutRelationsReq>,
) -> Result<Response, AppError> {
    let r = character::put_relations(&state.pool, &ids.0, body.0).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}
